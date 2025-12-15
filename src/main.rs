mod api;
mod config;
mod daemon;
mod indexer;
mod storage;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("contextd starting...");
    daemon::run().await
}
```
