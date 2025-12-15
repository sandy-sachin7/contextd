use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub fn run_parser(cmd: &[String], file_path: &Path) -> Result<String> {
    if cmd.is_empty() {
        return Err(anyhow::anyhow!("Empty plugin command"));
    }

    let program = &cmd[0];
    let args = &cmd[1..];

    // Prepare command
    let mut command = Command::new(program);
    command.args(args);
    command.arg(file_path);

    // Execute
    let output = command
        .output()
        .with_context(|| format!("Failed to execute plugin command: {:?}", cmd))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Plugin command failed with status {}: {}",
            output.status,
            stderr
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .context("Plugin output is not valid UTF-8")?;

    Ok(stdout)
}
