//! Database operations for slide annotations.

use anyhow::{Context, Result, bail};
use deadpool_postgres::Pool;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::annotation_models::{
    AnnotationKind, AnnotationResponse, AnnotationSet, ListAnnotationsQuery,
    ListAnnotationsResponse, ListAnnotationSetsResponse, Metadata, PolygonPath,
};
use crate::bitmask::Bitmask;

/// Initialize annotation tables in the database schema.
pub async fn init_annotation_schema(pool: &Pool) -> Result<()> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Create annotation_sets table
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS annotation_sets (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                slide_id UUID NOT NULL REFERENCES slides(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                task_type TEXT NOT NULL DEFAULT 'other',
                created_by TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                locked BOOLEAN NOT NULL DEFAULT FALSE,
                metadata JSONB NOT NULL DEFAULT '{}'::jsonb
            )
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_sets table")?;

    // Create indexes for annotation_sets
    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_annotation_sets_slide_id 
            ON annotation_sets (slide_id)
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_sets slide_id index")?;

    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_annotation_sets_slide_id_created_at 
            ON annotation_sets (slide_id, created_at)
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_sets slide_id_created_at index")?;

    // Create annotations table
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS annotations (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                annotation_set_id UUID NOT NULL REFERENCES annotation_sets(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                label_id TEXT NOT NULL DEFAULT '',
                created_by TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ,
                metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
                source TEXT
            )
            "#,
            &[],
        )
        .await
        .context("failed to create annotations table")?;

    // Create indexes for annotations
    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_annotations_annotation_set_id 
            ON annotations (annotation_set_id)
            "#,
            &[],
        )
        .await
        .context("failed to create annotations annotation_set_id index")?;

    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_annotations_annotation_set_id_label_id 
            ON annotations (annotation_set_id, label_id)
            "#,
            &[],
        )
        .await
        .context("failed to create annotations annotation_set_id_label_id index")?;

    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_annotations_created_by 
            ON annotations (created_by)
            "#,
            &[],
        )
        .await
        .context("failed to create annotations created_by index")?;

    // Create annotation_points table
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS annotation_points (
                annotation_id UUID PRIMARY KEY REFERENCES annotations(id) ON DELETE CASCADE,
                x_level0 DOUBLE PRECISION NOT NULL,
                y_level0 DOUBLE PRECISION NOT NULL
            )
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_points table")?;

    // Create spatial index for points
    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_annotation_points_coords 
            ON annotation_points (x_level0, y_level0)
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_points coords index")?;

    // Create annotation_polygons table
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS annotation_polygons (
                annotation_id UUID PRIMARY KEY REFERENCES annotations(id) ON DELETE CASCADE,
                path JSONB NOT NULL
            )
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_polygons table")?;

    // Create annotation_ellipses table
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS annotation_ellipses (
                annotation_id UUID PRIMARY KEY REFERENCES annotations(id) ON DELETE CASCADE,
                cx_level0 DOUBLE PRECISION NOT NULL,
                cy_level0 DOUBLE PRECISION NOT NULL,
                radius_x DOUBLE PRECISION NOT NULL,
                radius_y DOUBLE PRECISION NOT NULL,
                rotation_radians DOUBLE PRECISION NOT NULL DEFAULT 0.0
            )
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_ellipses table")?;

    // Create spatial index for ellipse centers
    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_annotation_ellipses_center 
            ON annotation_ellipses (cx_level0, cy_level0)
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_ellipses center index")?;

    // Create annotation_masks table
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS annotation_masks (
                annotation_id UUID PRIMARY KEY REFERENCES annotations(id) ON DELETE CASCADE,
                x0_level0 DOUBLE PRECISION NOT NULL,
                y0_level0 DOUBLE PRECISION NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                encoding TEXT NOT NULL DEFAULT 'bitmask',
                data BYTEA NOT NULL
            )
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_masks table")?;

    // Create spatial index for mask bounding boxes
    client
        .execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_annotation_masks_bbox 
            ON annotation_masks (x0_level0, y0_level0)
            "#,
            &[],
        )
        .await
        .context("failed to create annotation_masks bbox index")?;

    tracing::info!("annotation database schema initialized");
    Ok(())
}

// =============================================================================
// Annotation Sets
// =============================================================================

/// Create a new annotation set.
pub async fn create_annotation_set(
    pool: &Pool,
    slide_id: Uuid,
    name: &str,
    task_type: &str,
    created_by: Option<&str>,
    locked: bool,
    metadata: &Metadata,
) -> Result<AnnotationSet> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_one(
            r#"
            INSERT INTO annotation_sets (slide_id, name, task_type, created_by, locked, metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, slide_id, name, task_type, created_by, created_at, locked, metadata
            "#,
            &[&slide_id, &name, &task_type, &created_by, &locked, metadata],
        )
        .await
        .context("failed to insert annotation set")?;

    Ok(row_to_annotation_set(&row))
}

/// Get an annotation set by ID.
pub async fn get_annotation_set(pool: &Pool, id: Uuid) -> Result<Option<AnnotationSet>> {
    let client = pool.get().await.context("failed to get db connection")?;

    let row = client
        .query_opt(
            r#"
            SELECT id, slide_id, name, task_type, created_by, created_at, locked, metadata
            FROM annotation_sets
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to query annotation set")?;

    Ok(row.map(|r| row_to_annotation_set(&r)))
}

/// List annotation sets for a slide.
pub async fn list_annotation_sets(pool: &Pool, slide_id: Uuid) -> Result<ListAnnotationSetsResponse> {
    let client = pool.get().await.context("failed to get db connection")?;

    let rows = client
        .query(
            r#"
            SELECT id, slide_id, name, task_type, created_by, created_at, locked, metadata
            FROM annotation_sets
            WHERE slide_id = $1
            ORDER BY created_at ASC
            "#,
            &[&slide_id],
        )
        .await
        .context("failed to list annotation sets")?;

    let items: Vec<AnnotationSet> = rows.iter().map(row_to_annotation_set).collect();

    Ok(ListAnnotationSetsResponse { items })
}

/// Update an annotation set.
pub async fn update_annotation_set(
    pool: &Pool,
    id: Uuid,
    name: Option<&str>,
    task_type: Option<&str>,
    locked: Option<bool>,
    metadata: Option<&Metadata>,
) -> Result<Option<AnnotationSet>> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Build dynamic update query
    let mut set_clauses = Vec::new();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref n) = name {
        set_clauses.push(format!("name = ${}", param_idx));
        params.push(n);
        param_idx += 1;
    }
    if let Some(ref t) = task_type {
        set_clauses.push(format!("task_type = ${}", param_idx));
        params.push(t);
        param_idx += 1;
    }
    if let Some(ref l) = locked {
        set_clauses.push(format!("locked = ${}", param_idx));
        params.push(l);
        param_idx += 1;
    }
    if let Some(ref m) = metadata {
        set_clauses.push(format!("metadata = ${}", param_idx));
        params.push(m);
        param_idx += 1;
    }

    if set_clauses.is_empty() {
        return get_annotation_set(pool, id).await;
    }

    let query = format!(
        r#"
        UPDATE annotation_sets 
        SET {} 
        WHERE id = ${} 
        RETURNING id, slide_id, name, task_type, created_by, created_at, locked, metadata
        "#,
        set_clauses.join(", "),
        param_idx
    );
    params.push(&id);

    let row = client
        .query_opt(&query, &params)
        .await
        .context("failed to update annotation set")?;

    Ok(row.map(|r| row_to_annotation_set(&r)))
}

/// Delete an annotation set (hard delete, cascades to annotations).
pub async fn delete_annotation_set(pool: &Pool, id: Uuid) -> Result<bool> {
    let client = pool.get().await.context("failed to get db connection")?;

    let rows_affected = client
        .execute(
            r#"
            DELETE FROM annotation_sets
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to delete annotation set")?;

    Ok(rows_affected > 0)
}

fn row_to_annotation_set(row: &tokio_postgres::Row) -> AnnotationSet {
    AnnotationSet {
        id: row.get("id"),
        slide_id: row.get("slide_id"),
        name: row.get("name"),
        task_type: row.get("task_type"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
        locked: row.get("locked"),
        metadata: row.get("metadata"),
    }
}

// =============================================================================
// Annotations
// =============================================================================

/// Create a new point annotation.
pub async fn create_point_annotation(
    pool: &Pool,
    annotation_set_id: Uuid,
    label_id: &str,
    created_by: Option<&str>,
    metadata: &Metadata,
    source: Option<&str>,
    x_level0: f64,
    y_level0: f64,
) -> Result<AnnotationResponse> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Start transaction
    let tx = client;

    // Insert the annotation
    let ann_row = tx
        .query_one(
            r#"
            INSERT INTO annotations (annotation_set_id, kind, label_id, created_by, metadata, source)
            VALUES ($1, 'point', $2, $3, $4, $5)
            RETURNING id, annotation_set_id, kind, label_id, created_by, created_at, updated_at, metadata, source
            "#,
            &[&annotation_set_id, &label_id, &created_by, metadata, &source],
        )
        .await
        .context("failed to insert point annotation")?;

    let annotation_id: Uuid = ann_row.get("id");

    // Insert the point geometry
    tx.execute(
        r#"
        INSERT INTO annotation_points (annotation_id, x_level0, y_level0)
        VALUES ($1, $2, $3)
        "#,
        &[&annotation_id, &x_level0, &y_level0],
    )
    .await
    .context("failed to insert point geometry")?;

    let geometry = serde_json::json!({
        "x_level0": x_level0,
        "y_level0": y_level0
    });

    Ok(row_to_annotation_response(&ann_row, geometry))
}

/// Create a new polygon or polyline annotation.
pub async fn create_polygon_annotation(
    pool: &Pool,
    annotation_set_id: Uuid,
    kind: AnnotationKind,
    label_id: &str,
    created_by: Option<&str>,
    metadata: &Metadata,
    source: Option<&str>,
    path: &PolygonPath,
) -> Result<AnnotationResponse> {
    let client = pool.get().await.context("failed to get db connection")?;

    let kind_str = kind.as_str();
    let path_json: JsonValue = serde_json::to_value(path).context("failed to serialize path")?;

    // Insert the annotation
    let ann_row = client
        .query_one(
            r#"
            INSERT INTO annotations (annotation_set_id, kind, label_id, created_by, metadata, source)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, annotation_set_id, kind, label_id, created_by, created_at, updated_at, metadata, source
            "#,
            &[&annotation_set_id, &kind_str, &label_id, &created_by, metadata, &source],
        )
        .await
        .context("failed to insert polygon annotation")?;

    let annotation_id: Uuid = ann_row.get("id");

    // Insert the polygon geometry
    client
        .execute(
            r#"
            INSERT INTO annotation_polygons (annotation_id, path)
            VALUES ($1, $2)
            "#,
            &[&annotation_id, &path_json],
        )
        .await
        .context("failed to insert polygon geometry")?;

    let geometry = serde_json::json!({
        "path": path.0
    });

    Ok(row_to_annotation_response(&ann_row, geometry))
}

/// Create a new ellipse annotation.
pub async fn create_ellipse_annotation(
    pool: &Pool,
    annotation_set_id: Uuid,
    label_id: &str,
    created_by: Option<&str>,
    metadata: &Metadata,
    source: Option<&str>,
    cx_level0: f64,
    cy_level0: f64,
    radius_x: f64,
    radius_y: f64,
    rotation_radians: f64,
) -> Result<AnnotationResponse> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Insert the annotation
    let ann_row = client
        .query_one(
            r#"
            INSERT INTO annotations (annotation_set_id, kind, label_id, created_by, metadata, source)
            VALUES ($1, 'ellipse', $2, $3, $4, $5)
            RETURNING id, annotation_set_id, kind, label_id, created_by, created_at, updated_at, metadata, source
            "#,
            &[&annotation_set_id, &label_id, &created_by, metadata, &source],
        )
        .await
        .context("failed to insert ellipse annotation")?;

    let annotation_id: Uuid = ann_row.get("id");

    // Insert the ellipse geometry
    client
        .execute(
            r#"
            INSERT INTO annotation_ellipses (annotation_id, cx_level0, cy_level0, radius_x, radius_y, rotation_radians)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            &[&annotation_id, &cx_level0, &cy_level0, &radius_x, &radius_y, &rotation_radians],
        )
        .await
        .context("failed to insert ellipse geometry")?;

    let geometry = serde_json::json!({
        "cx_level0": cx_level0,
        "cy_level0": cy_level0,
        "radius_x": radius_x,
        "radius_y": radius_y,
        "rotation_radians": rotation_radians
    });

    Ok(row_to_annotation_response(&ann_row, geometry))
}

/// Create a new mask patch annotation.
pub async fn create_mask_annotation(
    pool: &Pool,
    annotation_set_id: Uuid,
    label_id: &str,
    created_by: Option<&str>,
    metadata: &Metadata,
    source: Option<&str>,
    x0_level0: f64,
    y0_level0: f64,
    bitmask: &Bitmask,
) -> Result<AnnotationResponse> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Insert the annotation
    let ann_row = client
        .query_one(
            r#"
            INSERT INTO annotations (annotation_set_id, kind, label_id, created_by, metadata, source)
            VALUES ($1, 'mask_patch', $2, $3, $4, $5)
            RETURNING id, annotation_set_id, kind, label_id, created_by, created_at, updated_at, metadata, source
            "#,
            &[&annotation_set_id, &label_id, &created_by, metadata, &source],
        )
        .await
        .context("failed to insert mask annotation")?;

    let annotation_id: Uuid = ann_row.get("id");

    // Insert the mask geometry
    client
        .execute(
            r#"
            INSERT INTO annotation_masks (annotation_id, x0_level0, y0_level0, width, height, encoding, data)
            VALUES ($1, $2, $3, $4, $5, 'bitmask', $6)
            "#,
            &[
                &annotation_id,
                &x0_level0,
                &y0_level0,
                &bitmask.width,
                &bitmask.height,
                &bitmask.data,
            ],
        )
        .await
        .context("failed to insert mask geometry")?;

    let geometry = serde_json::json!({
        "x0_level0": x0_level0,
        "y0_level0": y0_level0,
        "width": bitmask.width,
        "height": bitmask.height,
        "encoding": "bitmask",
        "data_base64": bitmask.to_base64()
    });

    Ok(row_to_annotation_response(&ann_row, geometry))
}

/// Get a single annotation by ID with its geometry.
pub async fn get_annotation(pool: &Pool, id: Uuid) -> Result<Option<AnnotationResponse>> {
    let client = pool.get().await.context("failed to get db connection")?;

    let ann_row = client
        .query_opt(
            r#"
            SELECT id, annotation_set_id, kind, label_id, created_by, created_at, updated_at, metadata, source
            FROM annotations
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to query annotation")?;

    let ann_row = match ann_row {
        Some(row) => row,
        None => return Ok(None),
    };

    let kind: String = ann_row.get("kind");
    let annotation_id: Uuid = ann_row.get("id");

    let geometry = fetch_geometry(&client, annotation_id, &kind, true).await?;

    Ok(Some(row_to_annotation_response(&ann_row, geometry)))
}

/// List annotations for an annotation set with optional filters.
pub async fn list_annotations(
    pool: &Pool,
    annotation_set_id: Uuid,
    query: &ListAnnotationsQuery,
) -> Result<ListAnnotationsResponse> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Build query with optional filters
    let mut where_clauses = vec!["annotation_set_id = $1".to_string()];
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![&annotation_set_id];
    let mut param_idx = 2;

    if let Some(ref label) = query.label_id {
        where_clauses.push(format!("label_id = ${}", param_idx));
        params.push(label);
        param_idx += 1;
    }

    if let Some(ref kind) = query.kind {
        where_clauses.push(format!("kind = ${}", param_idx));
        params.push(kind);
        // param_idx += 1; // Uncomment when adding more filter params
    }

    let sql = format!(
        r#"
        SELECT id, annotation_set_id, kind, label_id, created_by, created_at, updated_at, metadata, source
        FROM annotations
        WHERE {}
        ORDER BY created_at ASC
        "#,
        where_clauses.join(" AND ")
    );

    let rows = client
        .query(&sql, &params)
        .await
        .context("failed to list annotations")?;

    let include_mask_data = query.include_mask_data.unwrap_or(false);
    let has_bbox_filter = query.x_min.is_some()
        || query.y_min.is_some()
        || query.x_max.is_some()
        || query.y_max.is_some();

    let bbox = if has_bbox_filter {
        Some((
            query.x_min.unwrap_or(f64::MIN),
            query.y_min.unwrap_or(f64::MIN),
            query.x_max.unwrap_or(f64::MAX),
            query.y_max.unwrap_or(f64::MAX),
        ))
    } else {
        None
    };

    let mut items = Vec::new();

    for row in rows {
        let kind: String = row.get("kind");
        let annotation_id: Uuid = row.get("id");

        let geometry = fetch_geometry(&client, annotation_id, &kind, include_mask_data).await?;

        // Apply bbox filter if specified
        if let Some((x_min, y_min, x_max, y_max)) = bbox {
            if !geometry_intersects_bbox(&kind, &geometry, x_min, y_min, x_max, y_max) {
                continue;
            }
        }

        items.push(row_to_annotation_response(&row, geometry));
    }

    Ok(ListAnnotationsResponse { items })
}

/// Update an annotation.
pub async fn update_annotation(
    pool: &Pool,
    id: Uuid,
    label_id: Option<&str>,
    metadata: Option<&Metadata>,
    geometry: Option<&serde_json::Value>,
) -> Result<Option<AnnotationResponse>> {
    let client = pool.get().await.context("failed to get db connection")?;

    // Get existing annotation
    let existing = client
        .query_opt(
            r#"
            SELECT id, annotation_set_id, kind, label_id, created_by, created_at, updated_at, metadata, source
            FROM annotations
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to query annotation")?;

    let existing = match existing {
        Some(row) => row,
        None => return Ok(None),
    };

    let kind: String = existing.get("kind");

    // Update annotation fields if provided
    if label_id.is_some() || metadata.is_some() {
        let mut set_clauses = vec!["updated_at = NOW()".to_string()];
        let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
        let mut param_idx = 1;

        if let Some(ref l) = label_id {
            set_clauses.push(format!("label_id = ${}", param_idx));
            params.push(l);
            param_idx += 1;
        }
        if let Some(ref m) = metadata {
            set_clauses.push(format!("metadata = ${}", param_idx));
            params.push(m);
            param_idx += 1;
        }

        let sql = format!(
            "UPDATE annotations SET {} WHERE id = ${}",
            set_clauses.join(", "),
            param_idx
        );
        params.push(&id);

        client
            .execute(&sql, &params)
            .await
            .context("failed to update annotation")?;
    }

    // Update geometry if provided
    if let Some(geom) = geometry {
        update_geometry(&client, id, &kind, geom).await?;
    }

    // Fetch and return updated annotation
    get_annotation(pool, id).await
}

/// Delete an annotation (hard delete, cascades to geometry).
pub async fn delete_annotation(pool: &Pool, id: Uuid) -> Result<bool> {
    let client = pool.get().await.context("failed to get db connection")?;

    let rows_affected = client
        .execute(
            r#"
            DELETE FROM annotations
            WHERE id = $1
            "#,
            &[&id],
        )
        .await
        .context("failed to delete annotation")?;

    Ok(rows_affected > 0)
}

fn row_to_annotation_response(row: &tokio_postgres::Row, geometry: JsonValue) -> AnnotationResponse {
    AnnotationResponse {
        id: row.get("id"),
        annotation_set_id: row.get("annotation_set_id"),
        kind: row.get("kind"),
        label_id: row.get("label_id"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        metadata: row.get("metadata"),
        source: row.get("source"),
        geometry,
    }
}

/// Fetch geometry for an annotation based on its kind.
async fn fetch_geometry(
    client: &deadpool_postgres::Object,
    annotation_id: Uuid,
    kind: &str,
    include_mask_data: bool,
) -> Result<JsonValue> {
    match kind {
        "point" => {
            let row = client
                .query_one(
                    "SELECT x_level0, y_level0 FROM annotation_points WHERE annotation_id = $1",
                    &[&annotation_id],
                )
                .await
                .context("failed to fetch point geometry")?;

            Ok(serde_json::json!({
                "x_level0": row.get::<_, f64>("x_level0"),
                "y_level0": row.get::<_, f64>("y_level0")
            }))
        }
        "polygon" | "polyline" => {
            let row = client
                .query_one(
                    "SELECT path FROM annotation_polygons WHERE annotation_id = $1",
                    &[&annotation_id],
                )
                .await
                .context("failed to fetch polygon geometry")?;

            let path: JsonValue = row.get("path");
            Ok(serde_json::json!({ "path": path }))
        }
        "ellipse" => {
            let row = client
                .query_one(
                    "SELECT cx_level0, cy_level0, radius_x, radius_y, rotation_radians FROM annotation_ellipses WHERE annotation_id = $1",
                    &[&annotation_id],
                )
                .await
                .context("failed to fetch ellipse geometry")?;

            Ok(serde_json::json!({
                "cx_level0": row.get::<_, f64>("cx_level0"),
                "cy_level0": row.get::<_, f64>("cy_level0"),
                "radius_x": row.get::<_, f64>("radius_x"),
                "radius_y": row.get::<_, f64>("radius_y"),
                "rotation_radians": row.get::<_, f64>("rotation_radians")
            }))
        }
        "mask_patch" => {
            let row = client
                .query_one(
                    "SELECT x0_level0, y0_level0, width, height, encoding, data FROM annotation_masks WHERE annotation_id = $1",
                    &[&annotation_id],
                )
                .await
                .context("failed to fetch mask geometry")?;

            let mut json = serde_json::json!({
                "x0_level0": row.get::<_, f64>("x0_level0"),
                "y0_level0": row.get::<_, f64>("y0_level0"),
                "width": row.get::<_, i32>("width"),
                "height": row.get::<_, i32>("height"),
                "encoding": row.get::<_, String>("encoding")
            });

            if include_mask_data {
                let data: Vec<u8> = row.get("data");
                let width: i32 = row.get("width");
                let height: i32 = row.get("height");
                let bitmask = Bitmask::from_data(width, height, data)
                    .context("failed to parse mask data")?;
                json["data_base64"] = serde_json::Value::String(bitmask.to_base64());
            }

            Ok(json)
        }
        _ => bail!("unknown annotation kind: {}", kind),
    }
}

/// Update geometry for an annotation based on its kind.
async fn update_geometry(
    client: &deadpool_postgres::Object,
    annotation_id: Uuid,
    kind: &str,
    geometry: &serde_json::Value,
) -> Result<()> {
    match kind {
        "point" => {
            let x = geometry["x_level0"]
                .as_f64()
                .context("x_level0 required for point")?;
            let y = geometry["y_level0"]
                .as_f64()
                .context("y_level0 required for point")?;

            client
                .execute(
                    "UPDATE annotation_points SET x_level0 = $2, y_level0 = $3 WHERE annotation_id = $1",
                    &[&annotation_id, &x, &y],
                )
                .await
                .context("failed to update point geometry")?;
        }
        "polygon" | "polyline" => {
            let path = &geometry["path"];
            if path.is_null() {
                bail!("path required for polygon/polyline");
            }

            client
                .execute(
                    "UPDATE annotation_polygons SET path = $2 WHERE annotation_id = $1",
                    &[&annotation_id, path],
                )
                .await
                .context("failed to update polygon geometry")?;
        }
        "ellipse" => {
            let cx = geometry["cx_level0"]
                .as_f64()
                .context("cx_level0 required for ellipse")?;
            let cy = geometry["cy_level0"]
                .as_f64()
                .context("cy_level0 required for ellipse")?;
            let rx = geometry["radius_x"]
                .as_f64()
                .context("radius_x required for ellipse")?;
            let ry = geometry["radius_y"]
                .as_f64()
                .context("radius_y required for ellipse")?;
            let rot = geometry["rotation_radians"]
                .as_f64()
                .context("rotation_radians required for ellipse")?;

            client
                .execute(
                    "UPDATE annotation_ellipses SET cx_level0 = $2, cy_level0 = $3, radius_x = $4, radius_y = $5, rotation_radians = $6 WHERE annotation_id = $1",
                    &[&annotation_id, &cx, &cy, &rx, &ry, &rot],
                )
                .await
                .context("failed to update ellipse geometry")?;
        }
        "mask_patch" => {
            let x0 = geometry["x0_level0"]
                .as_f64()
                .context("x0_level0 required for mask")?;
            let y0 = geometry["y0_level0"]
                .as_f64()
                .context("y0_level0 required for mask")?;
            let width = geometry["width"]
                .as_i64()
                .context("width required for mask")? as i32;
            let height = geometry["height"]
                .as_i64()
                .context("height required for mask")? as i32;
            let data_base64 = geometry["data_base64"]
                .as_str()
                .context("data_base64 required for mask")?;

            let bitmask = Bitmask::from_base64(width, height, data_base64)
                .context("invalid bitmask data")?;

            client
                .execute(
                    "UPDATE annotation_masks SET x0_level0 = $2, y0_level0 = $3, width = $4, height = $5, data = $6 WHERE annotation_id = $1",
                    &[&annotation_id, &x0, &y0, &width, &height, &bitmask.data],
                )
                .await
                .context("failed to update mask geometry")?;
        }
        _ => bail!("unknown annotation kind: {}", kind),
    }

    Ok(())
}

/// Check if a geometry intersects with a bounding box.
fn geometry_intersects_bbox(
    kind: &str,
    geometry: &JsonValue,
    x_min: f64,
    y_min: f64,
    x_max: f64,
    y_max: f64,
) -> bool {
    match kind {
        "point" => {
            let x = geometry["x_level0"].as_f64().unwrap_or(0.0);
            let y = geometry["y_level0"].as_f64().unwrap_or(0.0);
            x >= x_min && x <= x_max && y >= y_min && y <= y_max
        }
        "polygon" | "polyline" => {
            // Compute bounding box of polygon path
            if let Some(path) = geometry["path"].as_array() {
                let mut px_min = f64::MAX;
                let mut py_min = f64::MAX;
                let mut px_max = f64::MIN;
                let mut py_max = f64::MIN;

                for point in path {
                    if let Some(arr) = point.as_array() {
                        if arr.len() >= 2 {
                            let x = arr[0].as_f64().unwrap_or(0.0);
                            let y = arr[1].as_f64().unwrap_or(0.0);
                            px_min = px_min.min(x);
                            py_min = py_min.min(y);
                            px_max = px_max.max(x);
                            py_max = py_max.max(y);
                        }
                    }
                }

                // Check bbox intersection
                px_max >= x_min && px_min <= x_max && py_max >= y_min && py_min <= y_max
            } else {
                true // If we can't parse, include it
            }
        }
        "ellipse" => {
            let cx = geometry["cx_level0"].as_f64().unwrap_or(0.0);
            let cy = geometry["cy_level0"].as_f64().unwrap_or(0.0);
            let rx = geometry["radius_x"].as_f64().unwrap_or(0.0);
            let ry = geometry["radius_y"].as_f64().unwrap_or(0.0);
            let rot = geometry["rotation_radians"].as_f64().unwrap_or(0.0);

            // Compute ellipse bounding box
            let cos_t = rot.cos();
            let sin_t = rot.sin();
            let half_width = ((rx * cos_t).powi(2) + (ry * sin_t).powi(2)).sqrt();
            let half_height = ((rx * sin_t).powi(2) + (ry * cos_t).powi(2)).sqrt();

            let ex_min = cx - half_width;
            let ey_min = cy - half_height;
            let ex_max = cx + half_width;
            let ey_max = cy + half_height;

            // Check bbox intersection
            ex_max >= x_min && ex_min <= x_max && ey_max >= y_min && ey_min <= y_max
        }
        "mask_patch" => {
            let mx0 = geometry["x0_level0"].as_f64().unwrap_or(0.0);
            let my0 = geometry["y0_level0"].as_f64().unwrap_or(0.0);
            let mw = geometry["width"].as_f64().unwrap_or(0.0);
            let mh = geometry["height"].as_f64().unwrap_or(0.0);

            let mx_max = mx0 + mw;
            let my_max = my0 + mh;

            // Check bbox intersection
            mx_max >= x_min && mx0 <= x_max && my_max >= y_min && my0 <= y_max
        }
        _ => true, // Unknown kind, include it
    }
}
