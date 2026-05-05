use std::collections::HashMap;
use std::env::Args;
use std::process::Command;

use async_trait::async_trait;
use serde::Deserialize;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

#[derive(Deserialize)]
struct PackageJson {
    scripts: Option<HashMap<String, String>>,
}

#[derive(Default)]
pub struct RunHandler {
    script: Option<String>,
    script_args: Vec<String>,
}

#[async_trait]
impl CommandHandler for RunHandler {
    fn parse(&mut self, args: &mut Args) -> Result<(), ParseError> {
        while let Some(arg) = args.next() {
            if self.script.is_none() {
                self.script = Some(arg);
            } else {
                self.script_args.push(arg);
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let cwd = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
        let pkg_path = cwd.join("package.json");

        let contents = std::fs::read_to_string(&pkg_path)
            .map_err(|e| CommandError::FailedToReadFile(e))?;

        let pkg: PackageJson = serde_json::from_str(&contents)
            .map_err(|e| CommandError::ParsingFailed(e))?;

        let scripts = pkg.scripts.unwrap_or_default();

        let script_name = match &self.script {
            Some(s) => s,
            None => {
                if scripts.is_empty() {
                    println!("No scripts defined in package.json");
                } else {
                    println!("Available scripts:");
                    for (name, cmd) in &scripts {
                        println!("  {:<20} {}", name, cmd);
                    }
                }
                return Ok(());
            }
        };

        let script_cmd = scripts.get(script_name).ok_or_else(|| {
            CommandError::GitFailed(format!(
                "script '{}' not found in package.json",
                script_name
            ))
        })?;

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

        // Append any extra args passed after the script name
        let full_cmd = if self.script_args.is_empty() {
            script_cmd.clone()
        } else {
            format!("{} {}", script_cmd, self.script_args.join(" "))
        };

        let status = {
            #[cfg(windows)]
            {
                Command::new("cmd")
                    .args(["/C", &full_cmd])
                    .env("PATH", &new_path)
                    .current_dir(&cwd)
                    .status()
                    .map_err(|e| CommandError::GitFailed(format!("failed to spawn shell: {e}")))?
            }
            #[cfg(not(windows))]
            {
                Command::new("/bin/sh")
                    .args(["-c", &full_cmd])
                    .env("PATH", &new_path)
                    .current_dir(&cwd)
                    .status()
                    .map_err(|e| CommandError::GitFailed(format!("failed to spawn shell: {e}")))?
            }
        };

        if !status.success() {
            let code = status.code().unwrap_or(1);
            return Err(CommandError::GitFailed(format!(
                "script '{}' exited with status {}",
                script_name, code
            )));
        }

        Ok(())
    }
}
