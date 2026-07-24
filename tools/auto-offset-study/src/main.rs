use anyhow::Result;
use clap::Parser;

use prpr_auto_offset_study::{config::Cli, run};

#[tokio::main]
async fn main() -> Result<()> {
    run(Cli::parse()).await
}
