use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub watch: WatchConfig,
    #[serde(default)]
    pub plugins: HashMap<String, Vec<String>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize, Debug, Clone)]
pub struct StorageConfig {
    pub db_path: PathBuf,
    pub model_path: PathBuf,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WatchConfig {
    pub paths: Vec<PathBuf>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3030,
            },
            storage: StorageConfig {
                db_path: PathBuf::from("contextd.db"),
                model_path: PathBuf::from("models"),
            },
            watch: WatchConfig {
                paths: vec![PathBuf::from(".")],
            },
            plugins: HashMap::new(),
        }
    }
}
