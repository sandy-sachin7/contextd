use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

pub async fn run_parser(cmd: &[String], file_path: &Path) -> Result<String> {
    if cmd.is_empty() {
        return Err(anyhow::anyhow!("Empty plugin command"));
    }

    let program = &cmd[0];
    let args = &cmd[1..];

    // Prepare command
    let mut command = Command::new(program);
    command.args(args);
    command.arg(file_path);

    // Execute with timeout
    let output_result = timeout(Duration::from_secs(30), command.output())
        .await
        .context("Plugin execution timed out after 30 seconds")?;

    let output = output_result.context("Failed to execute plugin command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Plugin command failed with status {}: {}",
            output.status,
            stderr
        ));
    }

    let stdout = String::from_utf8(output.stdout).context("Plugin output is not valid UTF-8")?;

    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_parser_echo() {
        let cmd = vec!["echo".to_string(), "-n".to_string(), "hello".to_string()];
        let output = run_parser(&cmd, Path::new("dummy.txt"))
            .await
            .expect("Failed to run echo");
        assert!(output.contains("hello"));
    }

    #[tokio::test]
    async fn test_run_parser_fail() {
        let cmd = vec!["false".to_string()];
        let result = run_parser(&cmd, Path::new("dummy.txt")).await;
        assert!(result.is_err());
    }
}
