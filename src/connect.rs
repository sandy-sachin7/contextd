use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectScope {
    /// Configure user-global AI tool settings (default)
    #[default]
    User,
    /// Configure project-local AI tool settings in the current directory
    Project,
}

pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    pub detect: fn() -> bool,
    /// Candidate config paths for user-global scope (first existing wins, else first path).
    pub user_config_paths: fn() -> Vec<PathBuf>,
    /// Candidate config paths for project-local scope (empty if tool is user-global only).
    pub project_config_paths: fn() -> Vec<PathBuf>,
    pub generate_config: fn(binary: &str) -> Value,
}

impl Tool {
    pub fn preferred_path(&self, scope: ConnectScope) -> Option<PathBuf> {
        let candidates = match scope {
            ConnectScope::User => (self.user_config_paths)(),
            ConnectScope::Project => (self.project_config_paths)(),
        };
        if candidates.is_empty() {
            return None;
        }
        // Prefer an existing config file; otherwise pick the primary (first) candidate
        if let Some(existing) = candidates.iter().find(|p| p.exists()) {
            return Some(existing.clone());
        }
        candidates.into_iter().next()
    }

    pub fn merge(&self, existing: &Value, binary: &str) -> Value {
        match self.id {
            "continue" => self.merge_array(existing, binary),
            "opencode" => self.merge_opencode(existing, binary),
            "vscode-copilot" => self.merge_key(existing, "servers", binary),
            "codex" => self.merge_key(existing, "mcp_servers", binary),
            _ => self.merge_key(existing, "mcpServers", binary),
        }
    }

    fn merge_key(&self, existing: &Value, key: &str, binary: &str) -> Value {
        let mut map = existing.as_object().cloned().unwrap_or_default();
        let servers = map
            .entry(key)
            .or_insert_with(|| json!({}))
            .as_object()
            .cloned()
            .unwrap_or_default();
        let mut servers_map: BTreeMap<String, Value> = servers.into_iter().collect();
        let generated = (self.generate_config)(binary);
        let entry = generated
            .get(key)
            .and_then(|v| v.get("contextd"))
            .cloned()
            .unwrap_or_else(|| json!({ "command": binary, "args": ["mcp"] }));
        servers_map.insert("contextd".to_string(), entry);
        map.insert(
            key.to_string(),
            Value::Object(servers_map.into_iter().collect()),
        );
        Value::Object(map)
    }

    fn merge_array(&self, existing: &Value, binary: &str) -> Value {
        let mut map = existing.as_object().cloned().unwrap_or_default();
        let mut arr = map
            .remove("mcpServers")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        arr.retain(|v| v.get("name") != Some(&json!("contextd")));
        arr.push(json!({
            "name": "contextd",
            "command": binary,
            "args": ["mcp"],
        }));
        map.insert("mcpServers".to_string(), Value::Array(arr));
        Value::Object(map)
    }

    fn merge_opencode(&self, existing: &Value, binary: &str) -> Value {
        let mut map = existing.as_object().cloned().unwrap_or_default();
        let mut mcp = map
            .remove("mcp")
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        mcp.insert(
            "contextd".to_string(),
            json!({ "type": "local", "command": [binary, "mcp"] }),
        );
        map.insert("mcp".to_string(), Value::Object(mcp));
        if !map.contains_key("$schema") {
            map.insert(
                "$schema".to_string(),
                json!("https://opencode.ai/config.json"),
            );
        }
        Value::Object(map)
    }
}

pub const TOOLS: &[Tool] = &[
    Tool {
        id: "claude-desktop",
        name: "Claude Desktop",
        detect: || {
            if cfg!(target_os = "macos") {
                PathBuf::from("/Applications/Claude.app").exists()
            } else if cfg!(target_os = "windows") {
                dirs::config_dir()
                    .map(|d| d.join("Claude").exists())
                    .unwrap_or(false)
            } else {
                home_dir().join(".config/Claude").exists()
            }
        },
        user_config_paths: || {
            if cfg!(target_os = "macos") {
                vec![home_dir()
                    .join("Library/Application Support/Claude/claude_desktop_config.json")]
            } else if cfg!(target_os = "windows") {
                vec![dirs::config_dir()
                    .unwrap_or_else(home_dir)
                    .join("Claude/claude_desktop_config.json")]
            } else {
                vec![home_dir().join(".config/Claude/claude_desktop_config.json")]
            }
        },
        project_config_paths: Vec::new,
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "claude-code",
        name: "Claude Code",
        detect: || binary_in_path("claude") || home_dir().join(".claude.json").exists(),
        user_config_paths: || vec![home_dir().join(".claude.json")],
        project_config_paths: || vec![PathBuf::from(".mcp.json")],
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "cursor",
        name: "Cursor",
        detect: || binary_in_path("cursor") || home_dir().join(".cursor").exists(),
        user_config_paths: || vec![home_dir().join(".cursor/mcp.json")],
        project_config_paths: || vec![PathBuf::from(".cursor/mcp.json")],
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "vscode-copilot",
        name: "GitHub Copilot (VSCode)",
        detect: || {
            binary_in_path("code")
                || binary_in_path("code-insiders")
                || home_dir().join(".vscode").exists()
                || vscode_user_dir().exists()
                || Path::new(".vscode").exists()
        },
        user_config_paths: || vec![vscode_user_dir().join("mcp.json")],
        project_config_paths: || vec![PathBuf::from(".vscode/mcp.json")],
        generate_config: |binary| json!({ "servers": { "contextd": { "type": "stdio", "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "copilot-cli",
        name: "GitHub Copilot CLI",
        detect: || binary_in_path("github-copilot-cli") || binary_in_path("copilot"),
        user_config_paths: || vec![home_dir().join(".copilot/mcp-config.json")],
        project_config_paths: Vec::new,
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "opencode",
        name: "OpenCode",
        detect: || binary_in_path("opencode") || home_dir().join(".config/opencode").exists(),
        user_config_paths: || {
            vec![
                home_dir().join(".config/opencode/opencode.json"),
                home_dir().join(".config/opencode/opencode.jsonc"),
                home_dir().join(".config/opencode/config.json"),
            ]
        },
        project_config_paths: || {
            vec![
                PathBuf::from("opencode.json"),
                PathBuf::from("opencode.jsonc"),
            ]
        },
        generate_config: |binary| {
            json!({
                "$schema": "https://opencode.ai/config.json",
                "mcp": { "contextd": { "type": "local", "command": [binary, "mcp"] } }
            })
        },
    },
    Tool {
        id: "continue",
        name: "Continue",
        detect: || home_dir().join(".continue").exists(),
        user_config_paths: || vec![home_dir().join(".continue/config.json")],
        project_config_paths: || vec![PathBuf::from(".continuerc.json")],
        generate_config: |binary| json!({ "mcpServers": [{ "name": "contextd", "command": binary, "args": ["mcp"] }] }),
    },
    Tool {
        id: "antigravity",
        name: "Antigravity (agy)",
        detect: || binary_in_path("agy"),
        user_config_paths: || vec![home_dir().join(".antigravity/plugins/mcp.json")],
        project_config_paths: Vec::new,
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "codex",
        name: "Codex",
        detect: || binary_in_path("codex") || home_dir().join(".codex/config.toml").exists(),
        user_config_paths: || vec![home_dir().join(".codex/config.toml")],
        project_config_paths: || vec![PathBuf::from(".codex/config.toml")],
        generate_config: |binary| json!({ "mcp_servers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
];

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn vscode_user_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir().join("Library/Application Support/Code/User")
    } else if cfg!(target_os = "windows") {
        dirs::config_dir()
            .unwrap_or_else(home_dir)
            .join("Code/User")
    } else {
        home_dir().join(".config/Code/User")
    }
}

fn binary_in_path(name: &str) -> bool {
    let path = env::var("PATH").unwrap_or_default();
    for dir in env::split_paths(&path) {
        let exe = if cfg!(target_os = "windows") {
            dir.join(format!("{}.exe", name))
        } else {
            dir.join(name)
        };
        if exe.exists() {
            return true;
        }
    }
    false
}

fn tool_binary_path() -> String {
    if let Ok(path) = env::current_exe() {
        path.to_string_lossy().to_string()
    } else {
        "contextd".to_string()
    }
}

/// Merge `contextd` into a TOML document using `toml_edit`, preserving comments & formatting.
fn merge_toml_preserving_comments(existing_str: &str, binary: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = if existing_str.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        existing_str
            .parse::<toml_edit::DocumentMut>()
            .context("Existing TOML config has syntax errors; refusing to overwrite")?
    };

    if !doc.contains_key("mcp_servers") {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc["mcp_servers"] = toml_edit::Item::Table(table);
    }

    let Some(mcp_table) = doc["mcp_servers"].as_table_mut() else {
        return Err(anyhow!(
            "Existing TOML key 'mcp_servers' is not a table; refusing to overwrite"
        ));
    };

    let mut entry = toml_edit::Table::new();
    entry["command"] = toml_edit::value(binary);
    let mut args = toml_edit::Array::new();
    args.push("mcp");
    entry["args"] = toml_edit::value(args);

    mcp_table.insert("contextd", toml_edit::Item::Table(entry));
    Ok(doc.to_string())
}

/// Atomically write `content` to `target_path`, creating a `.bak` backup if `target_path` exists.
fn atomic_write_with_backup(target_path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {:?}", parent))?;
        }
    }

    if target_path.exists() {
        let mut backup_name = target_path
            .file_name()
            .ok_or_else(|| anyhow!("Invalid config path {:?}", target_path))?
            .to_os_string();
        backup_name.push(".bak");
        let backup_path = target_path.with_file_name(backup_name);
        fs::copy(target_path, &backup_path).with_context(|| {
            format!(
                "Failed to create backup {:?} before updating {:?}",
                backup_path, target_path
            )
        })?;
    }

    let tmp_name = format!(
        ".{}.contextd.tmp.{}",
        target_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("config"),
        std::process::id()
    );
    let tmp_path = target_path.with_file_name(tmp_name);

    {
        let mut file = File::create(&tmp_path)
            .with_context(|| format!("Failed to create temp file {:?}", tmp_path))?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }

    fs::rename(&tmp_path, target_path).with_context(|| {
        let _ = fs::remove_file(&tmp_path);
        format!(
            "Failed to atomically rename {:?} to {:?}",
            tmp_path, target_path
        )
    })?;

    Ok(())
}

/// Update a single tool config file safely (JSON/JSONC via `json5`, TOML via `toml_edit`,
/// aborting on parse errors without modifying the existing file, creating `.bak` + atomic write).
pub fn configure_tool_at_path(tool: &Tool, cfg_path: &Path, binary: &str) -> Result<()> {
    let is_toml = cfg_path.extension().and_then(|e| e.to_str()) == Some("toml");

    let serialized = if is_toml {
        let existing_str = if cfg_path.exists() {
            fs::read_to_string(cfg_path)
                .with_context(|| format!("Failed to read {:?}", cfg_path))?
        } else {
            String::new()
        };
        merge_toml_preserving_comments(&existing_str, binary)
            .with_context(|| format!("Refusing to overwrite {:?}", cfg_path))?
    } else {
        let existing: Value = if cfg_path.exists() {
            let content = fs::read_to_string(cfg_path)
                .with_context(|| format!("Failed to read {:?}", cfg_path))?;
            if content.trim().is_empty() {
                Value::Object(Default::default())
            } else {
                // Use json5 so JSONC comments (//, /* */) and trailing commas parse cleanly.
                // NEVER silently fall back to empty `{}` on parse error!
                let parsed: Value = json5::from_str(&content).with_context(|| {
                    format!(
                        "Failed to parse JSON/JSONC config {:?}; refusing to overwrite existing file",
                        cfg_path
                    )
                })?;
                if !parsed.is_object() {
                    return Err(anyhow!(
                        "Expected top-level JSON object in {:?}; refusing to overwrite",
                        cfg_path
                    ));
                }
                parsed
            }
        } else {
            Value::Object(Default::default())
        };

        let merged = tool.merge(&existing, binary);
        let mut out = serde_json::to_string_pretty(&merged)
            .with_context(|| format!("Failed to serialize JSON for {:?}", cfg_path))?;
        out.push('\n');
        out
    };

    atomic_write_with_backup(cfg_path, &serialized)?;
    Ok(())
}

pub async fn handle_connect(
    all: bool,
    scope: ConnectScope,
    tool_filter: Option<&str>,
) -> Result<()> {
    let binary = tool_binary_path();

    let detected: Vec<&Tool> = if let Some(filter_id) = tool_filter {
        let found: Vec<&Tool> = TOOLS.iter().filter(|t| t.id == filter_id).collect();
        if found.is_empty() {
            let valid: Vec<&str> = TOOLS.iter().map(|t| t.id).collect();
            return Err(anyhow!(
                "Unknown tool '{}'. Valid tool IDs: {}",
                filter_id,
                valid.join(", ")
            ));
        }
        found
    } else {
        TOOLS.iter().filter(|t| (t.detect)()).collect()
    };

    if detected.is_empty() {
        println!("No compatible AI tools detected.");
        return Ok(());
    }

    if !all && tool_filter.is_none() {
        println!("Detected AI tools:\n");
        for (i, tool) in detected.iter().enumerate() {
            let target = tool
                .preferred_path(scope)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(user scope only)".to_string());
            println!("  {}. {} [{}] -> {}", i + 1, tool.name, tool.id, target);
        }
        println!("\nRun `contextd connect --all` to configure all detected tools (user scope).");
        println!("Use `--scope project` for project-local configs or `--tool <id>` for one tool.");
        return Ok(());
    }

    let mut configured = 0u32;
    let mut errors = Vec::new();

    for tool in detected {
        let Some(cfg_path) = tool.preferred_path(scope) else {
            println!(
                "  - {} skipped (not applicable for {:?} scope)",
                tool.name, scope
            );
            continue;
        };

        match configure_tool_at_path(tool, &cfg_path, &binary) {
            Ok(()) => {
                configured += 1;
                println!("  ✓ {} configured at {:?}", tool.name, cfg_path);
            }
            Err(err) => {
                eprintln!("  ✗ {} skipped ({:?}): {}", tool.name, cfg_path, err);
                errors.push((tool.name, err));
            }
        }
    }

    println!(
        "\nDone. contextd MCP configured for {} tool config{}.",
        configured,
        if configured == 1 { "" } else { "s" }
    );

    if !errors.is_empty() {
        return Err(anyhow!(
            "{} tool config(s) could not be updated safely (existing files were preserved)",
            errors.len()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_registry() {
        assert_eq!(TOOLS.len(), 9);
        let ids: Vec<&str> = TOOLS.iter().map(|t| t.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "Tool IDs must be unique");
    }

    #[test]
    fn test_merge_key_mcp_servers() {
        let tool = &TOOLS[0]; // claude-desktop
        let existing = json!({ "mcpServers": { "other": { "command": "foo" } } });
        let merged = tool.merge(&existing, "/usr/bin/contextd");
        assert_eq!(
            merged["mcpServers"]["contextd"]["command"],
            "/usr/bin/contextd"
        );
        assert_eq!(merged["mcpServers"]["other"]["command"], "foo");
    }

    #[test]
    fn test_merge_key_servers() {
        let tool = &TOOLS[3]; // vscode-copilot
        let merged = tool.merge(&Value::Object(Default::default()), "contextd");
        assert_eq!(merged["servers"]["contextd"]["command"], "contextd");
        assert_eq!(merged["servers"]["contextd"]["type"], "stdio");
    }

    #[test]
    fn test_merge_array_continue() {
        let tool = &TOOLS[6]; // continue
        let existing = json!({
            "mcpServers": [
                { "name": "other", "command": "foo" }
            ]
        });
        let merged = tool.merge(&existing, "contextd");
        let arr = merged["mcpServers"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().any(|v| v["name"] == "contextd"));
        assert!(arr.iter().any(|v| v["name"] == "other"));
    }

    #[test]
    fn test_merge_opencode() {
        let tool = &TOOLS[5]; // opencode
        let merged = tool.merge(&Value::Object(Default::default()), "contextd");
        assert_eq!(merged["mcp"]["contextd"]["type"], "local");
        assert_eq!(merged["mcp"]["contextd"]["command"][0], "contextd");
        assert_eq!(merged["$schema"], "https://opencode.ai/config.json");
    }

    #[test]
    fn test_jsonc_comments_and_trailing_commas_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("opencode.jsonc");
        let original = r#"{
  // User comment
  "theme": "dark",
  "mcp": {
    "other": { "type": "local", "command": ["other"], },
  },
}"#;
        fs::write(&cfg_path, original).unwrap();

        let opencode = TOOLS.iter().find(|t| t.id == "opencode").unwrap();
        configure_tool_at_path(opencode, &cfg_path, "/usr/local/bin/contextd").unwrap();

        // Backup must exist with original contents
        let bak_path = dir.path().join("opencode.jsonc.bak");
        assert!(bak_path.exists());
        assert_eq!(fs::read_to_string(&bak_path).unwrap(), original);

        // Merged config must preserve existing entries AND add contextd
        let updated: Value = serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
        assert_eq!(updated["theme"], "dark");
        assert_eq!(updated["mcp"]["other"]["command"][0], "other");
        assert_eq!(
            updated["mcp"]["contextd"]["command"][0],
            "/usr/local/bin/contextd"
        );
    }

    #[test]
    fn test_corrupt_json_aborts_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("mcp.json");
        let corrupt = "{ not valid json at all";
        fs::write(&cfg_path, corrupt).unwrap();

        let cursor = TOOLS.iter().find(|t| t.id == "cursor").unwrap();
        let res = configure_tool_at_path(cursor, &cfg_path, "/usr/bin/contextd");
        assert!(res.is_err());
        // Existing file MUST remain untouched
        assert_eq!(fs::read_to_string(&cfg_path).unwrap(), corrupt);
    }

    #[test]
    fn test_toml_preserves_comments_and_aborts_on_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("config.toml");
        let original =
            "# My Codex config\nmodel = \"o3\"\n\n[mcp_servers.existing]\ncommand = \"foo\"\n";
        fs::write(&cfg_path, original).unwrap();

        let codex = TOOLS.iter().find(|t| t.id == "codex").unwrap();
        configure_tool_at_path(codex, &cfg_path, "/usr/bin/contextd").unwrap();

        let updated = fs::read_to_string(&cfg_path).unwrap();
        assert!(updated.contains("# My Codex config"));
        assert!(updated.contains("model = \"o3\""));
        assert!(updated.contains("[mcp_servers.existing]"));
        assert!(updated.contains("[mcp_servers.contextd]"));
        assert!(updated.contains("/usr/bin/contextd"));

        // Corrupt TOML must fail without modifying file
        let bad_path = dir.path().join("bad.toml");
        let bad_toml = "[broken = ";
        fs::write(&bad_path, bad_toml).unwrap();
        assert!(configure_tool_at_path(codex, &bad_path, "/usr/bin/contextd").is_err());
        assert_eq!(fs::read_to_string(&bad_path).unwrap(), bad_toml);
    }

    #[test]
    fn test_binary_in_path() {
        if cfg!(unix) {
            assert!(binary_in_path("sh"));
        }
        assert!(!binary_in_path("this_binary_does_not_exist_xyz123"));
    }
}
