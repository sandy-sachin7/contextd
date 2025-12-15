use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.port, 3030);
        assert_eq!(config.storage.db_path, PathBuf::from("contextd.db"));
    }

    #[test]
    fn test_load_config() -> Result<()> {
        let mut file = NamedTempFile::new()?;
        writeln!(
            file,
            r#"
[server]
host = "0.0.0.0"
port = 8080

[storage]
db_path = "test.db"
model_path = "models"

[watch]
paths = ["/tmp"]

[plugins]
test = ["echo"]
"#
        )?;

        let config = Config::load(file.path())?;
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.storage.db_path, PathBuf::from("test.db"));
        assert_eq!(config.watch.paths[0], PathBuf::from("/tmp"));
        assert!(config.plugins.contains_key("test"));

        Ok(())
    }
}
