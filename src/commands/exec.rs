use std::env::Args;
use std::process::Command;

use async_trait::async_trait;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct ExecHandler {
    bin: Option<String>,
    bin_args: Vec<String>,
    shell_mode: bool,
}

#[async_trait]
impl CommandHandler for ExecHandler {
    fn parse(&mut self, args: &mut Args) -> Result<(), ParseError> {
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--shell-mode" | "-c" => self.shell_mode = true,
                _ if self.bin.is_none() => self.bin = Some(arg),
                _ => self.bin_args.push(arg),
            }
        }
        if self.bin.is_none() {
            return Err(ParseError::MissingArgument(
                "exec <command> [args...]".to_string(),
            ));
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let bin = self.bin.as_deref().unwrap();

        let cwd = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
        let local_bin = cwd.join("node_modules").join(".bin");

        let path_env = std::env::var("PATH").unwrap_or_default();
        let new_path = if local_bin.exists() {
            format!(
                "{}{}{}",
                local_bin.to_string_lossy(),
                if cfg!(windows) { ";" } else { ":" },
                path_env
            )
        } else {
            path_env
        };

        let status = if self.shell_mode {
            #[cfg(windows)]
            {
                let full_cmd = std::iter::once(bin)
                    .chain(self.bin_args.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                Command::new("cmd")
                    .args(["/C", &full_cmd])
                    .env("PATH", &new_path)
                    .current_dir(&cwd)
                    .status()
                    .map_err(|e| {
                        CommandError::GitFailed(format!("failed to spawn shell: {e}"))
                    })?
            }
            #[cfg(not(windows))]
            {
                let full_cmd = std::iter::once(bin)
                    .chain(self.bin_args.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                Command::new("/bin/sh")
                    .args(["-c", &full_cmd])
                    .env("PATH", &new_path)
                    .current_dir(&cwd)
                    .status()
                    .map_err(|e| {
                        CommandError::GitFailed(format!("failed to spawn shell: {e}"))
                    })?
            }
        } else {
            Command::new(bin)
                .args(&self.bin_args)
                .env("PATH", &new_path)
                .current_dir(&cwd)
                .status()
                .map_err(|e| {
                    CommandError::GitFailed(format!("failed to spawn '{}': {}", bin, e))
                })?
        };

        if !status.success() {
            let code = status.code().unwrap_or(1);
            return Err(CommandError::GitFailed(format!(
                "'{}' exited with status {}",
                bin, code
            )));
        }

        Ok(())
    }
}
