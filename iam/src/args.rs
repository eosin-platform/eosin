use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    Server(ServerArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct ServerArgs {
    #[command(flatten)]
    pub kc: eosin_common::args::KeycloakArgs,

    #[arg(long, env = "INTERNAL_PORT", required = true)]
    pub internal_port: u16,

    #[arg(long, env = "PUBLIC_PORT", required = true)]
    pub public_port: u16,
}
