use anyhow::{Context, Result, bail};
use async_nats::Client as NatsClient;
use deadpool_postgres::Pool;
use histion_common::streams::{SlideProgressEvent, topics};
use histion_storage::StorageClient;
use image::{ImageBuffer, Rgba, RgbaImage};
use openslide_rs::{OpenSlide, Size};
use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::db::{
    CHECKPOINT_INTERVAL, CHECKPOINT_MIN_TILES, clear_all_tile_checkpoints, get_tile_checkpoint,
    mark_level_complete, update_tile_checkpoint,
};
use crate::meta_client::MetaClient;

/// Tile size in pixels (width and height)
pub const TILE_SIZE: u32 = 512;

/// Slide metadata extracted from the TIF file
#[derive(Debug, Clone)]
pub struct SlideMetadata {
    pub width: u32,
    pub height: u32,
    pub level_count: u32,
    #[allow(dead_code)]
    pub level_dimensions: Vec<(u32, u32)>,
}

/// Open a slide and extract its metadata
#[allow(clippy::cast_possible_truncation)]
pub fn get_slide_metadata(path: &Path) -> Result<SlideMetadata> {
    let slide = OpenSlide::new(path).context("failed to open slide")?;

    let Size { w, h } = slide
        .get_level_dimensions(0)
        .context("failed to get slide dimensions")?;
    let level_count = slide
        .get_level_count()
        .context("failed to get level count")?;

    let mut level_dimensions = Vec::with_capacity(level_count as usize);
    for level in 0..level_count {
        let Size { w, h } = slide
            .get_level_dimensions(level)
            .context(format!("failed to get dimensions for level {level}"))?;
        level_dimensions.push((w, h));
    }

    Ok(SlideMetadata {
        width: w,
        height: h,
        level_count,
        level_dimensions,
    })
}

/// Process a slide file: extract all tiles at all MIP levels and upload to storage.
///
/// This function is memory-efficient: it processes one tile at a time, never holding
/// the entire slide in memory. Tile extraction and encoding is parallelized using rayon.
///
/// The `cancel` token allows for graceful shutdown - processing will stop at the next
/// opportunity when cancellation is requested.
///
/// If `pg_pool` is provided, checkpointing is enabled for levels with more than 128 tiles.
/// This allows resuming near where we left off on restart.
///
/// Progress is reported to the meta service every 10,000 tiles and published to NATS.
pub async fn process_slide(
    path: &Path,
    slide_id: Uuid,
    storage_client: &mut StorageClient,
    nats_client: &NatsClient,
    meta_client: &MetaClient,
    pg_pool: Option<&Pool>,
    cancel: CancellationToken,
) -> Result<SlideMetadata> {
    let slide = OpenSlide::new(path).context("failed to open slide")?;
    let metadata = get_slide_metadata(path)?;

    tracing::info!(
        width = metadata.width,
        height = metadata.height,
        levels = metadata.level_count,
        threads = rayon::current_num_threads(),
        "processing slide"
    );

    // Calculate maximum mip level needed (until dimensions < TILE_SIZE)
    let max_mip_level = calculate_max_mip_level(metadata.width, metadata.height);

    // Calculate total tiles across all levels
    let total_tiles = calculate_total_tiles(metadata.width, metadata.height, max_mip_level);

    tracing::info!(
        native_levels = metadata.level_count,
        max_mip_level = max_mip_level,
        total_tiles = total_tiles,
        "mip level configuration"
    );

    // Initialize progress: set total and report to meta
    let progress_total = total_tiles as i32;

    // Calculate how many tiles are already done from checkpoints so that
    // progress is not reset to 0 when the compiler restarts.
    // Completed levels have checkpoint rows with completed_up_to == total_tiles.
    // Levels that were never started have no row (returns 0).
    let mut tiles_already_done: usize = 0;
    if let Some(pool) = pg_pool {
        for level in 0..=max_mip_level {
            match get_tile_checkpoint(pool, slide_id, level).await {
                Ok(checkpoint) => {
                    tiles_already_done += checkpoint;
                }
                Err(e) => {
                    tracing::warn!(level = level, error = ?e, "failed to query checkpoint for initial progress");
                }
            }
        }
    }

    let initial_progress = tiles_already_done as i32;
    tracing::info!(
        tiles_already_done = tiles_already_done,
        total_tiles = total_tiles,
        "calculated initial progress from checkpoints"
    );

    if let Err(e) = meta_client
        .update_progress(slide_id, initial_progress, progress_total)
        .await
    {
        tracing::warn!(error = ?e, "failed to set initial progress");
    }

    // Publish initial progress to NATS
    let topic = topics::slide_progress(slide_id);
    let event = SlideProgressEvent {
        progress_steps: initial_progress,
        progress_total,
    };
    if let Err(e) = nats_client
        .publish(topic, bytes::Bytes::from(event.to_bytes().to_vec()))
        .await
    {
        tracing::warn!(error = %e, "failed to publish initial progress");
    }

    // Track global progress across all levels, starting from previously completed work
    let global_tiles_done = Arc::new(AtomicUsize::new(tiles_already_done));
    let last_reported_step = Arc::new(AtomicUsize::new(0));

    // Process each mip level from highest (lowest resolution) to 0 (full resolution)
    // This allows the slide to be viewable at low resolution while still processing
    for level in (0..=max_mip_level).rev() {
        // Check for cancellation between levels
        if cancel.is_cancelled() {
            tracing::info!("processing cancelled, stopping at level {}", level);
            bail!("processing cancelled");
        }
        process_level(
            &slide,
            &metadata,
            slide_id,
            level,
            storage_client,
            nats_client,
            meta_client,
            pg_pool,
            path,
            &cancel,
            progress_total,
            global_tiles_done.clone(),
            last_reported_step.clone(),
        )
        .await?;
    }

    // Clear all checkpoints for this slide since processing is complete
    if let Some(pool) = pg_pool {
        if let Err(e) = clear_all_tile_checkpoints(pool, slide_id).await {
            tracing::warn!(error = ?e, "failed to clear tile checkpoints after completion");
        }
    }

    // Report final progress: progress_steps = progress_total to signal 100%
    if let Err(e) = meta_client
        .update_progress(slide_id, progress_total, progress_total)
        .await
    {
        tracing::warn!(error = ?e, "failed to report final progress");
    }

    // Publish final progress to NATS
    let topic = topics::slide_progress(slide_id);
    let event = SlideProgressEvent {
        progress_steps: progress_total,
        progress_total,
    };
    if let Err(e) = nats_client
        .publish(topic, bytes::Bytes::from(event.to_bytes().to_vec()))
        .await
    {
        tracing::warn!(error = %e, "failed to publish final progress");
    }

    Ok(metadata)
}

/// Calculate total tiles across all mip levels
#[allow(clippy::cast_possible_truncation)]
fn calculate_total_tiles(width: u32, height: u32, max_mip_level: u32) -> usize {
    let mut total = 0usize;
    for level in 0..=max_mip_level {
        let scale = 1u32 << level;
        let level_width = width.div_ceil(scale);
        let level_height = height.div_ceil(scale);
        let tiles_x = level_width.div_ceil(TILE_SIZE);
        let tiles_y = level_height.div_ceil(TILE_SIZE);
        total += (tiles_x as usize) * (tiles_y as usize);
    }
    total
}

/// Calculate the maximum MIP level needed
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn calculate_max_mip_level(width: u32, height: u32) -> u32 {
    let max_dim = width.max(height);
    if max_dim <= TILE_SIZE {
        return 0;
    }
    // Calculate how many times we can halve until we're at or below TILE_SIZE
    (f64::from(max_dim) / f64::from(TILE_SIZE)).log2().ceil() as u32
}

/// Process a single MIP level using parallel tile extraction
async fn process_level(
    slide: &OpenSlide,
    metadata: &SlideMetadata,
    slide_id: Uuid,
    level: u32,
    storage_client: &mut StorageClient,
    nats_client: &NatsClient,
    meta_client: &MetaClient,
    pg_pool: Option<&Pool>,
    slide_path: &Path,
    cancel: &CancellationToken,
    progress_total: i32,
    global_tiles_done: Arc<AtomicUsize>,
    last_reported_step: Arc<AtomicUsize>,
) -> Result<()> {
    // Calculate dimensions at this MIP level
    let scale = 1u32 << level; // 2^level
    let level_width = metadata.width.div_ceil(scale);
    let level_height = metadata.height.div_ceil(scale);

    // Calculate tile grid
    let tiles_x = level_width.div_ceil(TILE_SIZE);
    let tiles_y = level_height.div_ceil(TILE_SIZE);
    let total_tiles = tiles_x as usize * tiles_y as usize;

    // Check if checkpointing is enabled for this level
    let use_checkpoint = pg_pool.is_some() && total_tiles > CHECKPOINT_MIN_TILES;

    // Get checkpoint (number of tiles already completed) if checkpointing is enabled
    let start_index = if use_checkpoint {
        match get_tile_checkpoint(pg_pool.unwrap(), slide_id, level).await {
            Ok(checkpoint) => {
                if checkpoint > 0 {
                    tracing::info!(
                        level = level,
                        checkpoint = checkpoint,
                        total_tiles = total_tiles,
                        "resuming from checkpoint"
                    );
                }
                checkpoint
            }
            Err(e) => {
                tracing::warn!(error = ?e, "failed to get checkpoint, starting from beginning");
                0
            }
        }
    } else {
        0
    };

    // If we've already completed all tiles, skip this level
    if start_index >= total_tiles {
        tracing::info!(
            level = level,
            total_tiles = total_tiles,
            "level already complete, skipping"
        );
        return Ok(());
    }

    tracing::info!(
        level = level,
        dimensions = format!("{level_width}x{level_height}"),
        tiles = format!("{tiles_x}x{tiles_y}"),
        total_tiles = total_tiles,
        start_index = start_index,
        checkpoint_enabled = use_checkpoint,
        "processing level"
    );

    // Check if this level exists natively in the slide
    let native_level = find_best_native_level(slide, metadata, level);

    // Collect all tile coordinates, skipping already-completed tiles
    let tile_coords: Vec<(u32, u32)> = (0..tiles_y)
        .flat_map(|ty| (0..tiles_x).map(move |tx| (tx, ty)))
        .skip(start_index)
        .collect();

    // Count remaining tiles before moving tile_coords
    let tiles_remaining = tile_coords.len();

    // Clone metadata and path for parallel processing
    let metadata_clone = metadata.clone();
    let path_owned = slide_path.to_path_buf();

    // Process tiles in parallel using rayon
    // Each thread will open its own OpenSlide handle via thread_local
    // Channel sends Option: None for skipped (empty) tiles, Some for real tile data
    let (tx_channel, rx_channel) = async_channel::bounded::<Option<(u32, u32, Vec<u8>)>>(64);

    // Shared flag to signal rayon threads to stop
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();

    // Spawn the parallel tile extraction in a blocking task
    let extraction_handle = tokio::task::spawn_blocking(move || {
        // Use thread_local to cache OpenSlide handles per thread
        thread_local! {
            static SLIDE_HANDLE: std::cell::RefCell<Option<OpenSlide>> = const { std::cell::RefCell::new(None) };
        }

        tile_coords.par_iter().try_for_each(|&(tx, ty)| {
            // Check for cancellation
            if cancelled_clone.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!("processing cancelled"));
            }

            // Get or create the OpenSlide handle for this thread
            SLIDE_HANDLE.with(|cell| {
                let mut handle_ref = cell.borrow_mut();
                if handle_ref.is_none() {
                    match OpenSlide::new(&path_owned) {
                        Ok(s) => *handle_ref = Some(s),
                        Err(e) => return Err(anyhow::anyhow!("failed to open slide: {e}")),
                    }
                }

                let slide = handle_ref.as_ref().unwrap();

                // Extract the tile
                let tile = extract_tile(slide, &metadata_clone, level, tx, ty, native_level)?;

                // Skip empty tiles (sparse TIF regions with no content)
                if is_tile_empty(&tile) {
                    tx_channel
                        .send_blocking(None)
                        .map_err(|e| anyhow::anyhow!("failed to send skip signal: {e}"))?;
                    return Ok(());
                }

                let webp_data = encode_tile_webp(&tile, level);

                // Send the encoded tile to the upload channel
                tx_channel
                    .send_blocking(Some((tx, ty, webp_data)))
                    .map_err(|e| anyhow::anyhow!("failed to send tile: {e}"))?;

                Ok::<(), anyhow::Error>(())
            })
        })
    });

    // Receive and upload tiles as they are processed
    let mut uploaded = 0usize;
    let mut skipped = 0usize;
    let mut last_progress = 0usize;
    let mut last_checkpoint = 0usize;

    loop {
        tokio::select! {
            biased;

            () = cancel.cancelled() => {
                tracing::info!(level = level, "cancellation requested, stopping tile upload");
                // Save checkpoint before stopping (if checkpointing is enabled)
                let processed = uploaded + skipped;
                if use_checkpoint && processed > 0 {
                    let absolute_progress = start_index + processed;
                    if let Err(e) = update_tile_checkpoint(
                        pg_pool.unwrap(),
                        slide_id,
                        level,
                        absolute_progress,
                        total_tiles,
                    )
                    .await
                    {
                        tracing::warn!(error = ?e, "failed to save checkpoint on cancellation");
                    } else {
                        tracing::info!(
                            level = level,
                            checkpoint = absolute_progress,
                            "saved checkpoint on cancellation"
                        );
                    }
                }
                // Signal rayon threads to stop
                cancelled.store(true, Ordering::Relaxed);
                // Close the channel to unblock any waiting senders
                rx_channel.close();
                // Wait for extraction task to finish
                let _ = extraction_handle.await;
                bail!("processing cancelled");
            }

            result = rx_channel.recv() => {
                match result {
                    Ok(None) => {
                        // Empty tile skipped (sparse TIF region with no content)
                        skipped += 1;

                        // Still update global progress counter for skipped tiles
                        let new_global = global_tiles_done.fetch_add(1, Ordering::Relaxed) + 1;

                        // Report progress every PROGRESS_INTERVAL tiles
                        const PROGRESS_INTERVAL_SKIP: usize = 1_000;
                        let current_step = new_global / PROGRESS_INTERVAL_SKIP;
                        let prev_step = last_reported_step.load(Ordering::Relaxed);
                        if current_step > prev_step {
                            last_reported_step.store(current_step, Ordering::Relaxed);
                            let progress_steps = (current_step * PROGRESS_INTERVAL_SKIP) as i32;

                            if let Err(e) = meta_client.update_progress(slide_id, progress_steps, progress_total).await {
                                tracing::warn!(error = ?e, "failed to update progress");
                            }

                            let progress_topic = topics::slide_progress(slide_id);
                            let event = SlideProgressEvent {
                                progress_steps,
                                progress_total,
                            };
                            if let Err(e) = nats_client.publish(progress_topic, bytes::Bytes::from(event.to_bytes().to_vec())).await {
                                tracing::warn!(error = %e, "failed to publish progress event");
                            }

                            tracing::info!(
                                progress_steps = progress_steps,
                                progress_total = progress_total,
                                pct = format!("{:.1}%", (progress_steps as f64 / progress_total as f64) * 100.0),
                                "reported progress"
                            );
                        }

                        // Update checkpoint for skipped tiles too
                        let processed = uploaded + skipped;
                        if use_checkpoint && processed - last_checkpoint >= CHECKPOINT_INTERVAL {
                            let absolute_progress = start_index + processed;
                            if let Err(e) = update_tile_checkpoint(
                                pg_pool.unwrap(),
                                slide_id,
                                level,
                                absolute_progress,
                                total_tiles,
                            )
                            .await
                            {
                                tracing::warn!(error = ?e, "failed to update checkpoint");
                            }
                            last_checkpoint = processed;
                        }

                        // Log progress
                        let total_done = start_index + processed;
                        let progress_pct = (total_done * 100) / total_tiles;
                        if progress_pct >= last_progress + 10 || processed == tiles_remaining {
                            tracing::info!(
                                level = level,
                                progress = format!("{total_done}/{total_tiles} ({progress_pct}%)"),
                                skipped = skipped,
                                "tiles processed"
                            );
                            last_progress = progress_pct;
                        }
                    }
                    Ok(Some((tx, ty, webp_data))) => {
                        storage_client
                            .put_tile(slide_id, tx, ty, level, webp_data)
                            .await
                            .context(format!(
                                "failed to upload tile ({tx}, {ty}) at level {level}"
                            ))?;

                        // Publish tile event to NATS (fire-and-forget, ignore errors)
                        // Payload: x (4 bytes LE) | y (4 bytes LE) | level (4 bytes LE)
                        let topic = topics::tile_data(slide_id);
                        let mut payload = [0u8; 12];
                        payload[0..4].copy_from_slice(&tx.to_le_bytes());
                        payload[4..8].copy_from_slice(&ty.to_le_bytes());
                        payload[8..12].copy_from_slice(&level.to_le_bytes());
                        if let Err(e) = nats_client.publish(topic, bytes::Bytes::from(payload.to_vec())).await {
                            tracing::warn!(error = %e, "failed to publish tile event");
                        }

                        uploaded += 1;

                        // Update global progress counter
                        let new_global = global_tiles_done.fetch_add(1, Ordering::Relaxed) + 1;

                        // Report progress every 10,000 tiles
                        const PROGRESS_INTERVAL: usize = 1_000;
                        let current_step = new_global / PROGRESS_INTERVAL;
                        let prev_step = last_reported_step.load(Ordering::Relaxed);
                        if current_step > prev_step {
                            last_reported_step.store(current_step, Ordering::Relaxed);
                            let progress_steps = (current_step * PROGRESS_INTERVAL) as i32;

                            // Update meta service
                            if let Err(e) = meta_client.update_progress(slide_id, progress_steps, progress_total).await {
                                tracing::warn!(error = ?e, "failed to update progress");
                            }

                            // Publish progress to NATS
                            let progress_topic = topics::slide_progress(slide_id);
                            let event = SlideProgressEvent {
                                progress_steps,
                                progress_total,
                            };
                            if let Err(e) = nats_client.publish(progress_topic, bytes::Bytes::from(event.to_bytes().to_vec())).await {
                                tracing::warn!(error = %e, "failed to publish progress event");
                            }

                            tracing::info!(
                                progress_steps = progress_steps,
                                progress_total = progress_total,
                                pct = format!("{:.1}%", (progress_steps as f64 / progress_total as f64) * 100.0),
                                "reported progress"
                            );
                        }

                        // Update checkpoint every CHECKPOINT_INTERVAL tiles
                        let processed = uploaded + skipped;
                        if use_checkpoint && processed - last_checkpoint >= CHECKPOINT_INTERVAL {
                            let absolute_progress = start_index + processed;
                            if let Err(e) = update_tile_checkpoint(
                                pg_pool.unwrap(),
                                slide_id,
                                level,
                                absolute_progress,
                                total_tiles,
                            )
                            .await
                            {
                                tracing::warn!(error = ?e, "failed to update checkpoint");
                            } else {
                                tracing::debug!(
                                    level = level,
                                    checkpoint = absolute_progress,
                                    "updated checkpoint"
                                );
                            }
                            last_checkpoint = processed;
                        }

                        // Log progress every 10%
                        let total_done = start_index + processed;
                        let progress_pct = (total_done * 100) / total_tiles;
                        if progress_pct >= last_progress + 10 || processed == tiles_remaining {
                            tracing::info!(
                                level = level,
                                progress = format!("{total_done}/{total_tiles} ({progress_pct}%)"),
                                skipped = skipped,
                                "tiles processed"
                            );
                            last_progress = progress_pct;
                        }
                    }
                    Err(_) => {
                        // Channel closed, extraction complete
                        break;
                    }
                }
            }
        }
    }

    // Wait for extraction to complete and check for errors
    extraction_handle
        .await
        .context("tile extraction task panicked")?
        .context("tile extraction failed")?;

    tracing::info!(
        level = level,
        uploaded = uploaded,
        skipped = skipped,
        total_tiles = total_tiles,
        "level complete"
    );

    // Mark this level as complete in the checkpoint table so that on restart
    // we can correctly count its tiles toward overall progress.
    if let Some(pool) = pg_pool {
        if let Err(e) = mark_level_complete(pool, slide_id, level, total_tiles).await {
            tracing::warn!(error = ?e, "failed to mark level as complete");
        }
    }

    Ok(())
}

/// Find the best native level in the slide that can be used to extract a tile at the given MIP level.
/// Returns the native level index and its scale relative to level 0.
fn find_best_native_level(
    slide: &OpenSlide,
    metadata: &SlideMetadata,
    target_level: u32,
) -> Option<(u32, f64)> {
    let target_scale = f64::from(1u32 << target_level);

    // Find a native level with scale <= target_scale (higher resolution than needed)
    // We prefer the level closest to our target to minimize downscaling work
    let mut best: Option<(u32, f64)> = None;

    for native_level in 0..metadata.level_count {
        if let Ok(downsample) = slide.get_level_downsample(native_level)
            && downsample <= target_scale
        {
            if let Some((_, best_downsample)) = best {
                if downsample > best_downsample {
                    best = Some((native_level, downsample));
                }
            } else {
                best = Some((native_level, downsample));
            }
        }
    }

    best
}

/// Extract a single tile at the given MIP level and tile coordinates.
///
/// If the native level doesn't match exactly, we read from the best available level
/// and downsample as needed.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn extract_tile(
    slide: &OpenSlide,
    metadata: &SlideMetadata,
    level: u32,
    tx: u32,
    ty: u32,
    native_level: Option<(u32, f64)>,
) -> Result<RgbaImage> {
    let target_scale = f64::from(1u32 << level);

    // Calculate the region in level-0 coordinates
    let x0 = i64::from(tx * TILE_SIZE) * (target_scale as i64);
    let y0 = i64::from(ty * TILE_SIZE) * (target_scale as i64);

    // Clamp to slide bounds
    let level_width = (f64::from(metadata.width) / target_scale).ceil() as u32;
    let level_height = (f64::from(metadata.height) / target_scale).ceil() as u32;
    let tile_width = TILE_SIZE.min(level_width.saturating_sub(tx * TILE_SIZE));
    let tile_height = TILE_SIZE.min(level_height.saturating_sub(ty * TILE_SIZE));

    if tile_width == 0 || tile_height == 0 {
        // Return empty tile
        return Ok(RgbaImage::new(tile_width, tile_height));
    }

    if let Some((native_idx, native_downsample)) = native_level {
        // Read from the native level
        let additional_scale = target_scale / native_downsample;

        // Size to read from native level
        let read_width = (f64::from(tile_width) * additional_scale).ceil() as u32;
        let read_height = (f64::from(tile_height) * additional_scale).ceil() as u32;

        // Position in level-0 coordinates
        let region = slide
            .read_region(&openslide_rs::Region {
                address: openslide_rs::Address {
                    x: x0 as u32,
                    y: y0 as u32,
                },
                level: native_idx,
                size: Size {
                    w: read_width,
                    h: read_height,
                },
            })
            .context("failed to read region from slide")?;

        // Convert to RgbaImage
        let img = rgba_buffer_from_openslide(&region, read_width, read_height)?;

        // Resize if needed
        if additional_scale > 1.0 + f64::EPSILON {
            Ok(resize_image(&img, tile_width, tile_height))
        } else {
            Ok(img)
        }
    } else {
        // No suitable native level - need to compute from level 0 and downsample
        tracing::warn!(
            level = level,
            tile = format!("({tx}, {ty})"),
            "no suitable native level, computing mip manually"
        );

        // Read from level 0 and downsample
        let read_width = (f64::from(tile_width) * target_scale).ceil() as u32;
        let read_height = (f64::from(tile_height) * target_scale).ceil() as u32;

        let region = slide
            .read_region(&openslide_rs::Region {
                address: openslide_rs::Address {
                    x: x0 as u32,
                    y: y0 as u32,
                },
                level: 0,
                size: Size {
                    w: read_width,
                    h: read_height,
                },
            })
            .context("failed to read region from slide")?;

        let img = rgba_buffer_from_openslide(&region, read_width, read_height)?;
        Ok(resize_image(&img, tile_width, tile_height))
    }
}

/// Convert `OpenSlide` region buffer to `RgbaImage`
fn rgba_buffer_from_openslide(buffer: &[u8], width: u32, height: u32) -> Result<RgbaImage> {
    // OpenSlide returns ARGB in native byte order, we need RGBA
    let expected_size = (width * height * 4) as usize;
    if buffer.len() < expected_size {
        bail!(
            "buffer size mismatch: expected {} bytes, got {}",
            expected_size,
            buffer.len()
        );
    }

    let mut img: RgbaImage = ImageBuffer::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            // OpenSlide format: ARGB (or BGRA on little-endian)
            // We need RGBA
            let pixel = Rgba([
                buffer[idx + 2], // R (from B position in BGRA)
                buffer[idx + 1], // G
                buffer[idx],     // B (from R position in BGRA)
                buffer[idx + 3], // A
            ]);
            img.put_pixel(x, y, pixel);
        }
    }

    Ok(img)
}

/// Resize an image using high-quality Lanczos3 filtering
fn resize_image(img: &RgbaImage, new_width: u32, new_height: u32) -> RgbaImage {
    image::imageops::resize(
        img,
        new_width,
        new_height,
        image::imageops::FilterType::Lanczos3,
    )
}

/// Check if a tile is entirely empty (all pixel channel values are zero).
/// Sparse TIF files have large regions with no content that produce all-zero tiles.
/// We skip storing these to save space and avoid rendering black squares in the client.
fn is_tile_empty(img: &RgbaImage) -> bool {
    // Sum all pixel components. If the sum is zero, the tile is empty.
    // Using u64 to avoid any overflow concerns.
    let sum: u64 = img.as_raw().iter().map(|&b| u64::from(b)).sum();
    sum == 0
}

/// Encode a tile as WebP
fn encode_tile_webp(img: &RgbaImage, level: u32) -> Vec<u8> {
    let encoder = webp::Encoder::from_rgba(img.as_raw(), img.width(), img.height());
    // Use quality 100 for level 0 (full resolution), 85 for other levels
    let quality = if level == 0 { 100.0 } else { 85.0 };
    let webp = encoder.encode(quality);
    webp.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_max_mip_level() {
        assert_eq!(calculate_max_mip_level(512, 512), 0);
        assert_eq!(calculate_max_mip_level(1024, 1024), 1);
        assert_eq!(calculate_max_mip_level(2048, 2048), 2);
        assert_eq!(calculate_max_mip_level(100000, 100000), 8);
    }
}
