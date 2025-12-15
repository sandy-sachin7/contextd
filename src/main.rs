mod api;
mod config;
mod daemon;
mod indexer;
mod storage;

use config::Config;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("contextd starting...");

    let config_path = Path::new("contextd.toml");
    let config = if config_path.exists() {
        println!("Loading config from {}", config_path.display());
        Config::load(config_path)?
    } else {
        println!("Config not found, using defaults");
        Config::default()
    };

    daemon::run(config).await
}
