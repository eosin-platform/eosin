use clap::{Parser, Subcommand};
use eosin_common::args::NatsArgs;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Run the cache miss consumer
    Consumer(ConsumerArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct ConsumerArgs {
    #[command(flatten)]
    pub nats: NatsArgs,

    /// Name of the JetStream stream to consume from
    #[arg(long, env = "STREAM_NAME", default_value = "eosin")]
    pub stream_name: String,

    /// Consumer name for durable consumption
    #[arg(long, env = "CONSUMER_NAME", default_value = "intake")]
    pub consumer_name: String,
}
