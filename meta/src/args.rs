use clap::{Parser, Subcommand};
use eosin_common::args::{KeycloakArgs, PostgresArgs};

#[derive(Parser, Debug)]
#[command(name = "eosin-meta")]
#[command(about = "Metadata service for SWI images")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run the metadata server
    Server(ServerArgs),

    /// Slide operations
    Slide(SlideCommandArgs),

    /// Dataset operations
    Dataset(DatasetCommandArgs),

    /// Check service health
    Health(HealthArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct SlideCommandArgs {
    #[command(subcommand)]
    pub command: SlideCommands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SlideCommands {
    /// Create a new slide
    Create(CreateSlideArgs),
    /// Get a slide by ID
    Get(GetSlideArgs),
    /// Update a slide by ID
    Update(UpdateSlideArgs),
    /// Delete a slide by ID
    Delete(DeleteSlideArgs),
    /// List slides with pagination
    List(ListSlidesArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct DatasetCommandArgs {
    #[command(subcommand)]
    pub command: DatasetCommands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DatasetCommands {
    /// Create a new dataset
    Create(CreateDatasetArgs),
    /// Get a dataset by ID
    Get(GetDatasetArgs),
    /// Update a dataset by ID
    Update(UpdateDatasetArgs),
    /// Delete a dataset by ID
    Delete(DeleteDatasetArgs),
    /// List datasets with pagination
    List(ListDatasetsArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct ServerArgs {
    /// Internal port to listen on (no auth required)
    #[arg(long, env = "INTERNAL_PORT", required = true)]
    pub internal_port: u16,

    /// Public port to listen on (auth required)
    #[arg(long, env = "PUBLIC_PORT", required = true)]
    pub public_port: u16,

    #[clap(flatten)]
    pub postgres: PostgresArgs,

    #[clap(flatten)]
    pub kc: KeycloakArgs,
}

#[derive(Parser, Debug, Clone)]
pub struct CreateSlideArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Slide UUID (deterministic, based on S3 key hash)
    #[arg(long)]
    pub id: String,

    /// Dataset UUID grouping this slide
    #[arg(long)]
    pub dataset: String,

    /// Slide width in pixels
    #[arg(long)]
    pub width: i32,

    /// Slide height in pixels
    #[arg(long)]
    pub height: i32,

    /// S3 URL of the slide (s3://...)
    #[arg(long)]
    pub url: String,

    /// Original filename of the slide
    #[arg(long, default_value = "")]
    pub filename: String,

    /// Full size of the slide file in bytes
    #[arg(long)]
    pub full_size: Option<i64>,

    /// Arbitrary slide metadata as JSON string
    #[arg(long)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Parser, Debug, Clone)]
pub struct GetSlideArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Slide UUID
    #[arg(long)]
    pub id: String,
}

#[derive(Parser, Debug, Clone)]
pub struct UpdateSlideArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Slide UUID
    #[arg(long)]
    pub id: String,

    /// Dataset UUID grouping this slide
    #[arg(long)]
    pub dataset: Option<String>,

    /// New slide width in pixels
    #[arg(long)]
    pub width: Option<i32>,

    /// New slide height in pixels
    #[arg(long)]
    pub height: Option<i32>,

    /// New S3 URL of the slide (s3://...)
    #[arg(long)]
    pub url: Option<String>,

    /// New filename of the slide
    #[arg(long)]
    pub filename: Option<String>,

    /// Full size of the slide file in bytes
    #[arg(long)]
    pub full_size: Option<i64>,
}

#[derive(Parser, Debug, Clone)]
pub struct DeleteSlideArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Slide UUID
    #[arg(long)]
    pub id: String,
}

#[derive(Parser, Debug, Clone)]
pub struct ListSlidesArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Offset for pagination
    #[arg(long, default_value_t = 0)]
    pub offset: i64,

    /// Maximum number of slides to return
    #[arg(long, default_value_t = 100)]
    pub limit: i64,
}

#[derive(Parser, Debug, Clone)]
pub struct HealthArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct CreateDatasetArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Dataset UUID
    #[arg(long)]
    pub id: String,

    /// Dataset name
    #[arg(long)]
    pub name: String,

    /// Dataset description
    #[arg(long)]
    pub description: Option<String>,

    /// Arbitrary dataset metadata as JSON
    #[arg(long)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Parser, Debug, Clone)]
pub struct GetDatasetArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Dataset UUID
    #[arg(long)]
    pub id: String,
}

#[derive(Parser, Debug, Clone)]
pub struct UpdateDatasetArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Dataset UUID
    #[arg(long)]
    pub id: String,

    /// Dataset name
    #[arg(long)]
    pub name: Option<String>,

    /// Dataset description
    #[arg(long)]
    pub description: Option<String>,

    /// Arbitrary dataset metadata as JSON
    #[arg(long)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Parser, Debug, Clone)]
pub struct DeleteDatasetArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Dataset UUID
    #[arg(long)]
    pub id: String,
}

#[derive(Parser, Debug, Clone)]
pub struct ListDatasetsArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Offset for pagination
    #[arg(long, default_value_t = 0)]
    pub offset: i64,

    /// Maximum number of datasets to return
    #[arg(long, default_value_t = 100)]
    pub limit: i64,
}
