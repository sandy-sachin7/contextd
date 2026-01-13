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
    use std::fs;
    use tempfile::NamedTempFile;

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

    #[tokio::test]
    async fn test_plugin_timeout() {
        // Use a shell script that sleeps, ignoring the file argument
        let cmd = vec!["sh".to_string(), "-c".to_string(), "sleep 35".to_string()];
        let result = run_parser(&cmd, Path::new("dummy.txt")).await;
        assert!(result.is_err(), "Should timeout after 30 seconds");
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            err_msg.contains("timeout") || err_msg.contains("timed out"),
            "Error should mention timeout, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_plugin_large_output() {
        // Use 'yes' command to generate lots of output, but head to limit it
        // This tests that we can handle large outputs
        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            "yes test | head -n 100000".to_string(),
        ];

        let temp_file = NamedTempFile::new().unwrap();
        let result = run_parser(&cmd, temp_file.path()).await;
        assert!(result.is_ok(), "Should handle large output gracefully");
        let output = result.unwrap();
        // Output should be large but manageable
        assert!(output.len() > 100000, "Output should be large");
    }

    #[tokio::test]
    async fn test_plugin_binary_output() {
        // Create a temp file with binary content
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), b"\xFF\xFE\x00\x01").unwrap();

        // Try to cat a binary file (will produce non-UTF8 output)
        let cmd = vec!["cat".to_string()];
        let result = run_parser(&cmd, temp_file.path()).await;

        // Should fail with UTF-8 error
        assert!(result.is_err(), "Should fail on non-UTF8 output");
        assert!(
            result.unwrap_err().to_string().contains("UTF-8"),
            "Error should mention UTF-8"
        );
    }

    #[tokio::test]
    async fn test_plugin_missing() {
        // Try to run a command that doesn't exist
        let cmd = vec!["this_command_definitely_does_not_exist_12345".to_string()];
        let result = run_parser(&cmd, Path::new("dummy.txt")).await;
        assert!(result.is_err(), "Should fail when plugin doesn't exist");
    }

    #[tokio::test]
    async fn test_plugin_empty_command() {
        let cmd = vec![];
        let result = run_parser(&cmd, Path::new("dummy.txt")).await;
        assert!(result.is_err(), "Should fail on empty command");
        assert!(
            result.unwrap_err().to_string().contains("Empty"),
            "Error should mention empty command"
        );
    }

    #[tokio::test]
    async fn test_plugin_with_stderr() {
        // Command that writes to stderr but succeeds
        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            "echo error >&2 && echo output".to_string(),
        ];

        let temp_file = NamedTempFile::new().unwrap();
        let result = run_parser(&cmd, temp_file.path()).await;
        assert!(result.is_ok(), "Should succeed despite stderr output");
        assert!(result.unwrap().contains("output"), "Should capture stdout");
    }

    #[tokio::test]
    async fn test_plugin_nonexistent_file() {
        // Plugin that tries to read a file that doesn't exist
        let cmd = vec!["cat".to_string()];
        let result = run_parser(&cmd, Path::new("/nonexistent/path/to/file.txt")).await;
        assert!(result.is_err(), "Should fail when file doesn't exist");
    }
}
