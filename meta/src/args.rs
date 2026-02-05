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
}

#[derive(Parser, Debug, Clone)]
pub struct ServerArgs {
    /// Port to listen on
    #[arg(long, env = "META_PORT", default_value_t = 8080)]
    pub port: u16,

    #[clap(flatten)]
    pub postgres: PostgresArgs,
}
