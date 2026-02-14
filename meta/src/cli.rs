use anyhow::Result;
use uuid::Uuid;

use crate::args::{
    CreateSlideArgs, DeleteSlideArgs, GetSlideArgs, ListSlidesArgs, UpdateSlideArgs,
};
use crate::client::MetaClient;

fn default_endpoint() -> String {
    "http://localhost:80".to_string()
}

/// Run the create slide CLI command.
pub async fn run_create_slide(args: CreateSlideArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let id: Uuid = args.id.parse()?;
    let dataset: Uuid = args.dataset.parse()?;
    let slide = client
        .create_slide(
            id,
            dataset,
            args.width,
            args.height,
            &args.url,
            &args.filename,
            args.full_size.unwrap_or(0),
            args.metadata,
        )
        .await?;

    println!("Created slide:");
    println!("  ID:     {}", slide.id);
    println!("  Width:  {}", slide.width);
    println!("  Height: {}", slide.height);
    println!("  URL:    {}", slide.url);
    println!(
        "  Metadata: {}",
        serde_json::to_string(&slide.metadata).unwrap_or_default()
    );
    Ok(())
}

/// Run the get slide CLI command.
pub async fn run_get_slide(args: GetSlideArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let id: Uuid = args.id.parse()?;
    match client.get_slide(id).await? {
        Some(slide) => {
            println!("Slide:");
            println!("  ID:     {}", slide.id);
            println!("  Width:  {}", slide.width);
            println!("  Height: {}", slide.height);
            println!("  URL:    {}", slide.url);
        }
        None => {
            println!("Slide {} not found", id);
        }
    }

    Ok(())
}

/// Run the update slide CLI command.
pub async fn run_update_slide(args: UpdateSlideArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let id: Uuid = args.id.parse()?;
    let dataset = args.dataset.as_deref().map(str::parse).transpose()?;
    match client
        .update_slide(
            id,
            dataset,
            args.width,
            args.height,
            args.url,
            args.filename,
            args.full_size,
        )
        .await?
    {
        Some(slide) => {
            println!("Updated slide:");
            println!("  ID:     {}", slide.id);
            println!("  Width:  {}", slide.width);
            println!("  Height: {}", slide.height);
            println!("  URL:    {}", slide.url);
        }
        None => {
            println!("Slide {} not found", id);
        }
    }

    Ok(())
}

/// Run the delete slide CLI command.
pub async fn run_delete_slide(args: DeleteSlideArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let id: Uuid = args.id.parse()?;
    if client.delete_slide(id).await? {
        println!("Deleted slide {}", id);
    } else {
        println!("Slide {} not found", id);
    }

    Ok(())
}

/// Run the list slides CLI command.
pub async fn run_list_slides(args: ListSlidesArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let response = client.list_slides(args.offset, args.limit).await?;

    println!(
        "Slides (offset: {}, limit: {}, total: {}, truncated: {}):",
        response.offset, response.limit, response.full_count, response.truncated
    );

    if response.items.is_empty() {
        println!("  (no slides)");
    } else {
        for slide in &response.items {
            println!(
                "{},{},{},{}",
                slide.id, slide.filename, slide.width, slide.height,
            );
        }
    }

    Ok(())
}

/// Run the health check CLI command.
pub async fn run_health(endpoint: Option<String>) -> Result<()> {
    let endpoint = endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    client.health().await?;
    println!("OK");

    Ok(())
}
