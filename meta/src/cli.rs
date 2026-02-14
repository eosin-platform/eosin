use anyhow::Result;
use uuid::Uuid;

use crate::args::{
    CreateDatasetArgs, CreateSlideArgs, DeleteDatasetArgs, DeleteSlideArgs, GetDatasetArgs,
    GetSlideArgs, ListDatasetsArgs, ListSlidesArgs, UpdateDatasetArgs, UpdateSlideArgs,
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

/// Run the create dataset CLI command.
pub async fn run_create_dataset(args: CreateDatasetArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let id: Uuid = args.id.parse()?;
    let dataset = client
        .create_dataset(id, &args.name, args.description.as_deref(), args.metadata.as_ref())
        .await?;

    println!("Created dataset:");
    println!("  ID:   {}", dataset.id);
    println!("  Name: {}", dataset.name);
    println!("  Description: {}", dataset.description.unwrap_or_default());

    Ok(())
}

/// Run the get dataset CLI command.
pub async fn run_get_dataset(args: GetDatasetArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let id: Uuid = args.id.parse()?;
    match client.get_dataset(id).await? {
        Some(dataset) => {
            println!("Dataset:");
            println!("  ID:   {}", dataset.id);
            println!("  Name: {}", dataset.name);
            println!("  Description: {}", dataset.description.unwrap_or_default());
        }
        None => {
            println!("Dataset {} not found", id);
        }
    }

    Ok(())
}

/// Run the update dataset CLI command.
pub async fn run_update_dataset(args: UpdateDatasetArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let id: Uuid = args.id.parse()?;
    match client
        .update_dataset(
            id,
            args.name.as_deref(),
            args.description.as_deref(),
            args.metadata.as_ref(),
        )
        .await?
    {
        Some(dataset) => {
            println!("Updated dataset:");
            println!("  ID:   {}", dataset.id);
            println!("  Name: {}", dataset.name);
            println!("  Description: {}", dataset.description.unwrap_or_default());
        }
        None => {
            println!("Dataset {} not found", id);
        }
    }

    Ok(())
}

/// Run the delete dataset CLI command.
pub async fn run_delete_dataset(args: DeleteDatasetArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let id: Uuid = args.id.parse()?;
    if client.delete_dataset(id).await? {
        println!("Deleted dataset {}", id);
    } else {
        println!("Dataset {} not found", id);
    }

    Ok(())
}

/// Run the list datasets CLI command.
pub async fn run_list_datasets(args: ListDatasetsArgs) -> Result<()> {
    let endpoint = args.endpoint.unwrap_or_else(default_endpoint);
    let client = MetaClient::new(&endpoint);

    let response = client.list_datasets(args.offset, args.limit).await?;

    println!(
        "Datasets (offset: {}, limit: {}, total: {}, truncated: {}):",
        response.offset, response.limit, response.full_count, response.truncated
    );

    if response.items.is_empty() {
        println!("  (no datasets)");
    } else {
        for dataset in &response.items {
            println!("{},{},{}", dataset.id, dataset.name, dataset.description.clone().unwrap_or_default());
        }
    }

    Ok(())
}
