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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_parser_echo() {
        let cmd = vec!["echo".to_string(), "-n".to_string(), "hello".to_string()];
        // We ignore the file path for echo, but we need to pass something
        let output = run_parser(&cmd, Path::new("dummy.txt")).expect("Failed to run echo");
        // echo with file path arg usually prints the args.
        // "echo -n hello dummy.txt" -> "hello dummy.txt"
        assert!(output.contains("hello"));
    }

    #[test]
    fn test_run_parser_fail() {
        let cmd = vec!["false".to_string()];
        let result = run_parser(&cmd, Path::new("dummy.txt"));
        assert!(result.is_err());
    }
}
