use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

pub const DEFAULT_PLUGIN_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_PLUGIN_OUTPUT_BYTES: usize = 10 * 1024 * 1024; // 10 MiB

pub async fn run_parser(cmd: &[String], file_path: &Path) -> Result<String> {
    run_parser_with_options(
        cmd,
        file_path,
        DEFAULT_PLUGIN_TIMEOUT,
        DEFAULT_MAX_PLUGIN_OUTPUT_BYTES,
    )
    .await
}

pub async fn run_parser_with_options(
    cmd: &[String],
    file_path: &Path,
    timeout_dur: Duration,
    max_output_bytes: usize,
) -> Result<String> {
    if cmd.is_empty() {
        return Err(anyhow!("Empty plugin command"));
    }

    let program = &cmd[0];
    let args = &cmd[1..];

    let resolved_path =
        std::fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());

    let mut command = Command::new(program);
    command.args(args);
    // Use `--` separator to prevent flag injection when filenames start with `-`
    command.arg("--");
    command.arg(&resolved_path);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.stdin(Stdio::null());
    command.kill_on_drop(true);

    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to spawn plugin command '{}'", program))?;

    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to capture plugin stdout"))?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("Failed to capture plugin stderr"))?;

    let run_future = async {
        let stdout_task = async {
            let mut buf = Vec::new();
            let mut limited = (&mut stdout_pipe).take((max_output_bytes as u64).saturating_add(1));
            limited.read_to_end(&mut buf).await?;
            Ok::<Vec<u8>, std::io::Error>(buf)
        };
        let stderr_task = async {
            let mut buf = Vec::new();
            let mut limited = (&mut stderr_pipe).take(64 * 1024);
            limited.read_to_end(&mut buf).await?;
            Ok::<Vec<u8>, std::io::Error>(buf)
        };

        let (stdout_res, stderr_res) = tokio::join!(stdout_task, stderr_task);
        let stdout_bytes = stdout_res.context("Failed reading plugin stdout")?;
        let stderr_bytes = stderr_res.unwrap_or_default();

        if stdout_bytes.len() > max_output_bytes {
            let _ = child.kill().await;
            return Err(anyhow!(
                "Plugin output exceeded maximum limit of {} bytes",
                max_output_bytes
            ));
        }

        let status = child.wait().await.context("Failed waiting for plugin")?;
        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr_bytes);
            return Err(anyhow!(
                "Plugin command failed with status {}: {}",
                status,
                stderr
            ));
        }

        let stdout = String::from_utf8(stdout_bytes).context("Plugin output is not valid UTF-8")?;
        Ok(stdout)
    };

    timeout(timeout_dur, run_future)
        .await
        .map_err(|_| anyhow!("Plugin execution timed out after {:?}", timeout_dur))?
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
        let cmd = vec!["sh".to_string(), "-c".to_string(), "sleep 10".to_string()];
        let result = run_parser_with_options(
            &cmd,
            Path::new("dummy.txt"),
            Duration::from_millis(200),
            DEFAULT_MAX_PLUGIN_OUTPUT_BYTES,
        )
        .await;
        assert!(result.is_err(), "Should timeout promptly");
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            err_msg.contains("timeout") || err_msg.contains("timed out"),
            "Error should mention timeout, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_plugin_output_size_cap() {
        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            "yes test | head -n 10000".to_string(),
        ];
        let temp_file = NamedTempFile::new().unwrap();
        let result = run_parser_with_options(
            &cmd,
            temp_file.path(),
            Duration::from_secs(5),
            1024, // 1 KB cap
        )
        .await;
        assert!(result.is_err(), "Should reject output over cap");
        assert!(result.unwrap_err().to_string().contains("exceeded"));
    }

    #[tokio::test]
    async fn test_plugin_large_output() {
        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            "yes test | head -n 50000".to_string(),
        ];
        let temp_file = NamedTempFile::new().unwrap();
        let result = run_parser(&cmd, temp_file.path()).await;
        assert!(result.is_ok(), "Should handle large output within cap");
        assert!(result.unwrap().len() > 100_000);
    }

    #[tokio::test]
    async fn test_plugin_flag_injection_protected() {
        // File starting with `-n` should not be interpreted as a flag by `cat`
        let dir = tempfile::tempdir().unwrap();
        let weird_file = dir.path().join("-n");
        fs::write(&weird_file, "safe content").unwrap();
        let cmd = vec!["cat".to_string()];
        let out = run_parser(&cmd, &weird_file).await.unwrap();
        assert_eq!(out, "safe content");
    }

    #[tokio::test]
    async fn test_plugin_binary_output() {
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), b"\xFF\xFE\x00\x01").unwrap();

        let cmd = vec!["cat".to_string()];
        let result = run_parser(&cmd, temp_file.path()).await;

        assert!(result.is_err(), "Should fail on non-UTF8 output");
        assert!(result.unwrap_err().to_string().contains("UTF-8"));
    }

    #[tokio::test]
    async fn test_plugin_missing() {
        let cmd = vec!["this_command_definitely_does_not_exist_12345".to_string()];
        let result = run_parser(&cmd, Path::new("dummy.txt")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_plugin_empty_command() {
        let cmd = vec![];
        let result = run_parser(&cmd, Path::new("dummy.txt")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty"));
    }

    #[tokio::test]
    async fn test_plugin_with_stderr() {
        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            "echo error >&2 && echo output".to_string(),
        ];

        let temp_file = NamedTempFile::new().unwrap();
        let result = run_parser(&cmd, temp_file.path()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("output"));
    }

    #[tokio::test]
    async fn test_plugin_nonexistent_file() {
        let cmd = vec!["cat".to_string()];
        let result = run_parser(&cmd, Path::new("/nonexistent/path/to/file.txt")).await;
        assert!(result.is_err());
    }
}
