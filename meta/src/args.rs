use clap::{Parser, Subcommand};
use histion_common::args::PostgresArgs;

#[derive(Parser, Debug)]
#[command(name = "histion-meta")]
#[command(about = "Metadata service for SWI images")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run the metadata server
    Server(ServerArgs),

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

    /// Check service health
    Health(HealthArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct ServerArgs {
    /// Port to listen on
    #[arg(long, env = "META_PORT", default_value_t = 8080)]
    pub port: u16,

    #[clap(flatten)]
    pub postgres: PostgresArgs,
}

#[derive(Parser, Debug, Clone)]
pub struct CreateSlideArgs {
    /// Meta service endpoint
    #[arg(long, env = "META_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Slide UUID (deterministic, based on S3 key hash)
    #[arg(long)]
    pub id: String,

    /// Slide width in pixels
    #[arg(long)]
    pub width: i32,

    /// Slide height in pixels
    #[arg(long)]
    pub height: i32,

    /// S3 URL of the slide (s3://...)
    #[arg(long)]
    pub url: String,
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

    /// New slide width in pixels
    #[arg(long)]
    pub width: Option<i32>,

    /// New slide height in pixels
    #[arg(long)]
    pub height: Option<i32>,

    /// New S3 URL of the slide (s3://...)
    #[arg(long)]
    pub url: Option<String>,
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
