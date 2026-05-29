use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::{env, fs};

pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    pub detect: fn() -> bool,
    pub config_paths: fn() -> Vec<PathBuf>,
    pub generate_config: fn(binary: &str) -> Value,
}

impl Tool {
    fn merge(&self, existing: &Value, binary: &str) -> Value {
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

const TOOLS: &[Tool] = &[
    Tool {
        id: "claude-desktop",
        name: "Claude Desktop",
        detect: || {
            if cfg!(target_os = "macos") {
                PathBuf::from("/Applications/Claude.app").exists()
            } else if cfg!(target_os = "windows") {
                let base = env::var("APPDATA").unwrap_or_default();
                PathBuf::from(base).join("Claude").exists()
            } else {
                PathBuf::from(&home_dir())
                    .join(".config")
                    .join("Claude")
                    .exists()
            }
        },
        config_paths: || {
            let home = home_dir();
            if cfg!(target_os = "macos") {
                vec![PathBuf::from(&home)
                    .join("Library/Application Support/Claude/claude_desktop_config.json")]
            } else if cfg!(target_os = "windows") {
                let appdata = env::var("APPDATA").unwrap_or_default();
                vec![PathBuf::from(appdata).join("Claude/claude_desktop_config.json")]
            } else {
                vec![PathBuf::from(&home).join(".config/Claude/claude_desktop_config.json")]
            }
        },
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "claude-code",
        name: "Claude Code",
        detect: || {
            binary_in_path("claude") || PathBuf::from(&home_dir()).join(".claude.json").exists()
        },
        config_paths: || {
            vec![
                PathBuf::from(".").join(".mcp.json"),
                PathBuf::from(&home_dir()).join(".claude.json"),
            ]
        },
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "cursor",
        name: "Cursor",
        detect: || binary_in_path("cursor") || PathBuf::from(&home_dir()).join(".cursor").exists(),
        config_paths: || {
            vec![
                PathBuf::from(".").join(".cursor/mcp.json"),
                PathBuf::from(&home_dir()).join(".cursor/mcp.json"),
            ]
        },
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "vscode-copilot",
        name: "GitHub Copilot (VSCode)",
        detect: || true,
        config_paths: || vec![PathBuf::from(".").join(".vscode/mcp.json")],
        generate_config: |binary| json!({ "servers": { "contextd": { "type": "stdio", "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "copilot-cli",
        name: "GitHub Copilot CLI",
        detect: || binary_in_path("github-copilot-cli") || binary_in_path("copilot"),
        config_paths: || vec![PathBuf::from(&home_dir()).join(".copilot/mcp-config.json")],
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "opencode",
        name: "OpenCode",
        detect: || {
            binary_in_path("opencode")
                || PathBuf::from(&home_dir()).join(".config/opencode").exists()
        },
        config_paths: || {
            vec![
                PathBuf::from(".").join("opencode.json"),
                PathBuf::from(".").join("opencode.jsonc"),
                PathBuf::from(&home_dir()).join(".config/opencode/config.json"),
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
        detect: || PathBuf::from(&home_dir()).join(".continue").exists(),
        config_paths: || {
            vec![
                PathBuf::from(&home_dir()).join(".continue/config.json"),
                PathBuf::from(".").join(".continuerc.json"),
            ]
        },
        generate_config: |binary| json!({ "mcpServers": [{ "name": "contextd", "command": binary, "args": ["mcp"] }] }),
    },
    Tool {
        id: "antigravity",
        name: "Antigravity (agy)",
        detect: || binary_in_path("agy"),
        config_paths: || vec![PathBuf::from(&home_dir()).join(".antigravity/plugins/mcp.json")],
        generate_config: |binary| json!({ "mcpServers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
    Tool {
        id: "codex",
        name: "Codex",
        detect: || {
            binary_in_path("codex")
                || PathBuf::from(&home_dir())
                    .join(".codex/config.toml")
                    .exists()
        },
        config_paths: || {
            vec![
                PathBuf::from(".").join(".codex/config.toml"),
                PathBuf::from(&home_dir()).join(".codex/config.toml"),
            ]
        },
        generate_config: |binary| json!({ "mcp_servers": { "contextd": { "command": binary, "args": ["mcp"] } } }),
    },
];

fn home_dir() -> String {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string())
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

pub async fn handle_connect(all: bool) -> Result<()> {
    let binary = tool_binary_path();

    let detected: Vec<&Tool> = TOOLS.iter().filter(|t| (t.detect)()).collect();

    if detected.is_empty() {
        println!("No compatible AI tools detected.");
        return Ok(());
    }

    let selected: Vec<&Tool> = if all {
        detected
    } else {
        println!("Detected AI tools:\n");
        for (i, tool) in detected.iter().enumerate() {
            println!("  {}. {}", i + 1, tool.name);
        }
        println!("\nRun `contextd connect --all` to configure all detected tools.");
        println!("Or configure individual tools via their config files.");
        return Ok(());
    };

    let mut configured = 0u32;
    for tool in selected {
        let paths = (tool.config_paths)();
        for cfg_path in &paths {
            if let Some(parent) = cfg_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {:?}", parent))?;
            }

            let is_toml = cfg_path.extension().and_then(|e| e.to_str()) == Some("toml");

            let existing: Value = match fs::read_to_string(cfg_path) {
                Ok(content) => {
                    if is_toml {
                        toml::from_str(&content)
                            .unwrap_or_else(|_| Value::Object(Default::default()))
                    } else {
                        serde_json::from_str(&content)
                            .unwrap_or_else(|_| Value::Object(Default::default()))
                    }
                }
                Err(_) => Value::Object(Default::default()),
            };

            let merged = tool.merge(&existing, &binary);

            let serialized = if is_toml {
                toml::to_string_pretty(&merged)
                    .with_context(|| format!("Failed to serialize TOML for {:?}", cfg_path))?
            } else {
                serde_json::to_string_pretty(&merged)
                    .with_context(|| format!("Failed to serialize JSON for {:?}", cfg_path))?
            };

            fs::write(cfg_path, &serialized)
                .with_context(|| format!("Failed to write config to {:?}", cfg_path))?;
            configured += 1;
            println!("  ✓ {} configured at {:?}", tool.name, cfg_path);
        }
    }

    println!(
        "\nDone. contextd MCP configured for {} tool config{}.",
        configured,
        if configured == 1 { "" } else { "s" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_trait_exists() {
        assert!(!TOOLS.is_empty());
        assert_eq!(TOOLS.len(), 9);
    }

    #[test]
    fn test_tool_ids_are_unique() {
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
    fn test_merge_array_replaces_existing() {
        let tool = &TOOLS[6]; // continue
        let existing = json!({
            "mcpServers": [
                { "name": "contextd", "command": "/old/path" },
                { "name": "other", "command": "foo" }
            ]
        });
        let merged = tool.merge(&existing, "/new/path");
        let arr = merged["mcpServers"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(
            arr.iter().find(|v| v["name"] == "contextd").unwrap()["command"],
            "/new/path"
        );
    }

    #[test]
    fn test_merge_preserves_other_keys() {
        let tool = &TOOLS[0]; // claude-desktop
        let existing = json!({
            "other_setting": "value",
            "mcpServers": {}
        });
        let merged = tool.merge(&existing, "contextd");
        assert_eq!(merged["other_setting"], "value");
    }

    #[test]
    fn test_generate_config_all_tools() {
        for tool in TOOLS {
            let config = (tool.generate_config)("contextd");
            assert!(
                config.is_object(),
                "{} generate_config must return object",
                tool.id
            );
        }
    }

    #[test]
    fn test_detect_no_false_positives() {
        // Detection functions should not panic for any tool
        for tool in TOOLS {
            let _ = (tool.detect)();
        }
    }

    #[test]
    fn test_binary_in_path() {
        // sh should always be in PATH on Unix
        if cfg!(unix) {
            assert!(binary_in_path("sh"));
        }
    }

    #[test]
    fn test_binary_in_path_not_found() {
        assert!(!binary_in_path("this_binary_does_not_exist_xyz123"));
    }
}
