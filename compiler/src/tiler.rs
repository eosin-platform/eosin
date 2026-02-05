use anyhow::{Context, Result, bail};
use histion_storage::StorageClient;
use image::{ImageBuffer, Rgba, RgbaImage};
use openslide_rs::{OpenSlide, Size};
use std::path::Path;
use uuid::Uuid;

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
    
    let Size { w, h } = slide.get_level_dimensions(0).context("failed to get slide dimensions")?;
    let level_count = slide.get_level_count().context("failed to get level count")?;
    
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
/// the entire slide in memory.
pub async fn process_slide(
    path: &Path,
    slide_id: Uuid,
    storage_client: &mut StorageClient,
) -> Result<SlideMetadata> {
    let slide = OpenSlide::new(path).context("failed to open slide")?;
    let metadata = get_slide_metadata(path)?;
    
    tracing::info!(
        width = metadata.width,
        height = metadata.height,
        levels = metadata.level_count,
        "processing slide"
    );
    
    // Calculate maximum mip level needed (until dimensions < TILE_SIZE)
    let max_mip_level = calculate_max_mip_level(metadata.width, metadata.height);
    
    tracing::info!(
        native_levels = metadata.level_count,
        max_mip_level = max_mip_level,
        "mip level configuration"
    );
    
    // Process each mip level
    for level in 0..=max_mip_level {
        process_level(&slide, &metadata, slide_id, level, storage_client).await?;
    }
    
    Ok(metadata)
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

/// Process a single MIP level
async fn process_level(
    slide: &OpenSlide,
    metadata: &SlideMetadata,
    slide_id: Uuid,
    level: u32,
    storage_client: &mut StorageClient,
) -> Result<()> {
    // Calculate dimensions at this MIP level
    let scale = 1u32 << level; // 2^level
    let level_width = metadata.width.div_ceil(scale);
    let level_height = metadata.height.div_ceil(scale);
    
    // Calculate tile grid
    let tiles_x = level_width.div_ceil(TILE_SIZE);
    let tiles_y = level_height.div_ceil(TILE_SIZE);
    
    tracing::info!(
        level = level,
        dimensions = format!("{level_width}x{level_height}"),
        tiles = format!("{tiles_x}x{tiles_y}"),
        "processing level"
    );
    
    // Check if this level exists natively in the slide
    let native_level = find_best_native_level(slide, metadata, level);
    
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let tile = extract_tile(slide, metadata, level, tx, ty, native_level)?;
            let webp_data = encode_tile_webp(&tile);
            
            storage_client
                .put_tile(slide_id, tx, ty, level, webp_data)
                .await
                .context(format!("failed to upload tile ({tx}, {ty}) at level {level}"))?;
        }
        
        // Log progress periodically
        if ty % 10 == 0 || ty == tiles_y - 1 {
            tracing::debug!(
                level = level,
                progress = format!("{}/{}", ty + 1, tiles_y),
                "tile rows processed"
            );
        }
    }
    
    Ok(())
}

/// Find the best native level in the slide that can be used to extract a tile at the given MIP level.
/// Returns the native level index and its scale relative to level 0.
fn find_best_native_level(slide: &OpenSlide, metadata: &SlideMetadata, target_level: u32) -> Option<(u32, f64)> {
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
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
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
                address: openslide_rs::Address { x: x0 as u32, y: y0 as u32 },
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
                address: openslide_rs::Address { x: x0 as u32, y: y0 as u32 },
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
    image::imageops::resize(img, new_width, new_height, image::imageops::FilterType::Lanczos3)
}

/// Encode a tile as WebP
fn encode_tile_webp(img: &RgbaImage) -> Vec<u8> {
    let encoder = webp::Encoder::from_rgba(img.as_raw(), img.width(), img.height());
    let webp = encoder.encode(85.0); // Quality 85
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
