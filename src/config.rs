use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_FILE_SIZE: u64 = 2 * 1024 * 1024; // 2 MB
pub const DEFAULT_DEBOUNCE_MS: u64 = 500;
pub const DEFAULT_MAX_CHUNK_CHARS: usize = 1500;
pub const DEFAULT_CHUNK_OVERLAP: usize = 150;

#[derive(Deserialize, Debug, Clone, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub watch: WatchConfig,
    #[serde(default)]
    pub chunking: ChunkingConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub plugins: HashMap<String, Vec<String>>,
    /// Internal marker: true when loaded from a workspace-local `contextd.toml`.
    /// Workspace-local `[plugins]` are never executed unless explicitly trusted.
    #[serde(skip)]
    pub is_workspace_config: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub auth_token: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            auth_token: None,
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    3030
}

#[derive(Deserialize, Debug, Clone)]
pub struct StorageConfig {
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,
    #[serde(default = "default_model_path")]
    pub model_path: PathBuf,
    #[serde(default = "default_model_type")]
    pub model_type: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
            model_path: default_model_path(),
            model_type: default_model_type(),
        }
    }
}

pub fn default_config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("contextd"))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("contextd"))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_config_path() -> PathBuf {
    default_config_dir().join("contextd.toml")
}

pub fn default_db_path() -> PathBuf {
    default_data_dir().join("contextd.db")
}

pub fn default_model_path() -> PathBuf {
    default_data_dir().join("models")
}

pub fn default_model_type() -> String {
    "all-minilm-l6-v2".to_string()
}

#[derive(Deserialize, Debug, Clone)]
pub struct WatchConfig {
    #[serde(default = "default_watch_paths")]
    pub paths: Vec<PathBuf>,
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            paths: default_watch_paths(),
            debounce_ms: default_debounce_ms(),
            max_file_size: default_max_file_size(),
        }
    }
}

fn default_watch_paths() -> Vec<PathBuf> {
    vec![PathBuf::from(".")]
}

fn default_debounce_ms() -> u64 {
    DEFAULT_DEBOUNCE_MS
}

fn default_max_file_size() -> u64 {
    DEFAULT_MAX_FILE_SIZE
}

#[derive(Deserialize, Debug, Clone)]
pub struct ChunkingConfig {
    #[serde(default = "default_max_chunk_size")]
    pub max_chunk_size: usize,
    #[serde(default = "default_chunk_overlap")]
    pub overlap: usize,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            max_chunk_size: default_max_chunk_size(),
            overlap: default_chunk_overlap(),
        }
    }
}

fn default_max_chunk_size() -> usize {
    DEFAULT_MAX_CHUNK_CHARS
}

fn default_chunk_overlap() -> usize {
    DEFAULT_CHUNK_OVERLAP
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct SecurityConfig {
    #[serde(default)]
    pub allow_workspace_plugins: bool,
}

pub fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    path.to_path_buf()
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file {:?}", path))?;
        let mut config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML config {:?}", path))?;
        config.expand_paths();
        Ok(config)
    }

    /// Resolve config from explicit CLI flag, workspace-local `contextd.toml`,
    /// user-global XDG config, or platform defaults.
    pub fn load_resolved(
        explicit_path: Option<&Path>,
        trust_workspace_plugins: bool,
    ) -> Result<Self> {
        let mut config = if let Some(path) = explicit_path {
            let mut cfg = Self::load(path)?;
            // Treat an explicit path inside user config dir as trusted; otherwise workspace-local
            let canonical_cfg = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let canonical_user_dir =
                fs::canonicalize(default_config_dir()).unwrap_or_else(|_| default_config_dir());
            cfg.is_workspace_config = !canonical_cfg.starts_with(&canonical_user_dir);
            cfg
        } else {
            let local = PathBuf::from("contextd.toml");
            let user_cfg = default_config_path();
            if local.exists() {
                let mut cfg = Self::load(&local)?;
                cfg.is_workspace_config = true;
                // Check whether user-global config explicitly enables workspace plugins
                if !cfg.security.allow_workspace_plugins && user_cfg.exists() {
                    if let Ok(global_cfg) = Self::load(&user_cfg) {
                        if global_cfg.security.allow_workspace_plugins {
                            cfg.security.allow_workspace_plugins = true;
                        }
                    }
                }
                cfg
            } else if user_cfg.exists() {
                let mut cfg = Self::load(&user_cfg)?;
                cfg.is_workspace_config = false;
                cfg
            } else {
                Self::default()
            }
        };

        if trust_workspace_plugins {
            config.security.allow_workspace_plugins = true;
        }

        config.apply_env_overrides()?;
        config.expand_paths();
        Ok(config)
    }

    pub fn expand_paths(&mut self) {
        self.storage.db_path = expand_tilde(&self.storage.db_path);
        self.storage.model_path = expand_tilde(&self.storage.model_path);
        self.watch.paths = self.watch.paths.iter().map(|p| expand_tilde(p)).collect();
    }

    pub fn apply_env_overrides(&mut self) -> Result<()> {
        if let Ok(val) = std::env::var("CONTEXTD_DB_PATH") {
            if !val.trim().is_empty() {
                self.storage.db_path = PathBuf::from(val);
            }
        }
        if let Ok(val) = std::env::var("CONTEXTD_MODEL_PATH") {
            if !val.trim().is_empty() {
                self.storage.model_path = PathBuf::from(val);
            }
        }
        if let Ok(val) = std::env::var("CONTEXTD_MODEL_TYPE") {
            if !val.trim().is_empty() {
                self.storage.model_type = val;
            }
        }
        if let Ok(val) = std::env::var("CONTEXTD_HOST") {
            if !val.trim().is_empty() {
                self.server.host = val;
            }
        }
        if let Ok(val) = std::env::var("CONTEXTD_PORT") {
            if !val.trim().is_empty() {
                self.server.port = val
                    .parse::<u16>()
                    .map_err(|e| anyhow!("Invalid CONTEXTD_PORT '{}': {}", val, e))?;
            }
        }
        if let Ok(val) = std::env::var("CONTEXTD_AUTH_TOKEN") {
            if !val.trim().is_empty() {
                self.server.auth_token = Some(val);
            }
        }
        if let Ok(val) = std::env::var("CONTEXTD_WATCH_PATHS") {
            if !val.trim().is_empty() {
                let paths: Vec<PathBuf> = if val.contains(',') && !Path::new(&val).exists() {
                    val.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .collect()
                } else {
                    std::env::split_paths(&val)
                        .filter(|p| !p.as_os_str().is_empty())
                        .collect()
                };
                if !paths.is_empty() {
                    self.watch.paths = paths;
                }
            }
        }
        if let Ok(val) = std::env::var("CONTEXTD_MAX_FILE_SIZE") {
            if !val.trim().is_empty() {
                self.watch.max_file_size = val
                    .parse::<u64>()
                    .map_err(|e| anyhow!("Invalid CONTEXTD_MAX_FILE_SIZE '{}': {}", val, e))?;
            }
        }
        Ok(())
    }

    /// Returns whether plugin execution is allowed under the current trust policy.
    pub fn plugins_allowed(&self) -> bool {
        !self.is_workspace_config || self.security.allow_workspace_plugins
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
        assert_eq!(config.server.host, "127.0.0.1");
        assert!(config.storage.db_path.ends_with("contextd.db"));
        assert!(config.storage.model_path.ends_with("models"));
        assert_eq!(config.watch.max_file_size, DEFAULT_MAX_FILE_SIZE);
        assert!(!config.security.allow_workspace_plugins);
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

    #[test]
    fn test_load_partial_config() -> Result<()> {
        let mut file = NamedTempFile::new()?;
        writeln!(
            file,
            r#"
[storage]
db_path = "partial.db"
"#
        )?;

        let config = Config::load(file.path())?;
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 3030);
        assert_eq!(config.storage.db_path, PathBuf::from("partial.db"));
        Ok(())
    }

    #[test]
    fn test_workspace_plugin_trust_policy() {
        let mut cfg = Config {
            is_workspace_config: true,
            ..Default::default()
        };
        assert!(!cfg.plugins_allowed());

        cfg.security.allow_workspace_plugins = true;
        assert!(cfg.plugins_allowed());

        cfg.is_workspace_config = false;
        cfg.security.allow_workspace_plugins = false;
        assert!(cfg.plugins_allowed());
    }
}
