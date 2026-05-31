use std::collections::HashMap;
use std::process::Command;

use async_trait::async_trait;
use serde::Deserialize;

use crate::{
    constants::{BIN_DIR, NODE_MODULES, PACKAGE_JSON},
    errors::{CommandError, ParseError},
    workspace,
};

use super::command_handler::CommandHandler;

#[derive(Deserialize)]
struct PackageJson {
    scripts: Option<HashMap<String, String>>,
}

#[derive(Default)]
pub struct RunHandler {
    script: Option<String>,
    script_args: Vec<String>,
    filter: Option<String>,
    recursive: bool,
}

#[async_trait]
impl CommandHandler for RunHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        let mut rest: Vec<String> = args.collect();

        // Pre-scan for workspace flags so they don't land in script_args
        let mut i = 0;
        while i < rest.len() {
            match rest[i].as_str() {
                "--filter" | "-F" => {
                    let pat = rest.get(i + 1).cloned().ok_or_else(|| {
                        ParseError::MissingArgument("--filter <pattern>".to_string())
                    })?;
                    self.filter = Some(pat);
                    rest.remove(i);
                    rest.remove(i);
                }
                "-r" | "--recursive" => {
                    self.recursive = true;
                    rest.remove(i);
                }
                _ => i += 1,
            }
        }

        let mut iter = rest.into_iter();
        while let Some(arg) = iter.next() {
            if self.script.is_none() {
                self.script = Some(arg);
            } else {
                self.script_args.push(arg);
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        if self.recursive || self.filter.is_some() {
            return self.execute_workspace().await;
        }

        let cwd = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
        let pkg_path = cwd.join(PACKAGE_JSON);

        let contents =
            std::fs::read_to_string(&pkg_path).map_err(|e| CommandError::FailedToReadFile(e))?;

        let pkg: PackageJson =
            serde_json::from_str(&contents).map_err(|e| CommandError::ParsingFailed(e))?;

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

        let local_bin = cwd.join(NODE_MODULES).join(BIN_DIR);
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

impl RunHandler {
    async fn execute_workspace(&self) -> Result<(), CommandError> {
        let root = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
        let packages = workspace::discover(&root)?;

        let matched: Vec<_> = match &self.filter {
            Some(f) => workspace::apply_filter(&packages, f),
            None => packages.iter().collect(),
        };

        if matched.is_empty() {
            println!("No workspace packages matched.");
            return Ok(());
        }

        let script_name = match &self.script {
            Some(s) => s.as_str(),
            None => {
                println!("Available scripts across workspaces:");
                for pkg in &matched {
                    let mut s = pkg.scripts.clone();
                    s.sort();
                    println!("  {} — {}", pkg.name, s.join(", "));
                }
                return Ok(());
            }
        };

        let total = matched.len();
        let mut failures = 0usize;

        for pkg in &matched {
            let pkg_json_raw = std::fs::read_to_string(pkg.path.join(PACKAGE_JSON))
                .map_err(CommandError::FailedToReadFile)?;
            let pkg_json: serde_json::Value =
                serde_json::from_str(&pkg_json_raw).map_err(CommandError::ParsingFailed)?;

            let cmd_str = pkg_json
                .get("scripts")
                .and_then(|s| s.get(script_name))
                .and_then(|v| v.as_str());

            let Some(cmd) = cmd_str else {
                println!(
                    "\n[{}] script '{}' not found — skipping",
                    pkg.name, script_name
                );
                continue;
            };

            println!("\n[{}] $ {}", pkg.name, cmd);

            let status = workspace::run_script_in_dir(&pkg.path, cmd, &self.script_args)?;

            if !status.success() {
                let code = status.code().unwrap_or(1);
                println!("[{}] exited with code {}", pkg.name, code);
                failures += 1;
            }
        }

        println!();
        if failures == 0 {
            println!("{}/{} packages succeeded.", total, total);
        } else {
            println!("{} of {} package(s) failed.", failures, total);
        }

        Ok(())
    }
}
