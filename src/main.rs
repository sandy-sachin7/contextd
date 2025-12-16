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

    let args: Vec<String> = std::env::args().collect();
    let config_path = if args.len() > 1 {
        Path::new(&args[1])
    } else {
        Path::new("contextd.toml")
    };

    let config = if config_path.exists() {
        println!("Loading config from {}", config_path.display());
        Config::load(config_path)?
    } else {
        println!(
            "Config not found at {}, using defaults",
            config_path.display()
        );
        Config::default()
    };

    daemon::run(config).await
}
