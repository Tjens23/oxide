use async_trait::async_trait;
use serde_json::Value;

use crate::{
    constants::PACKAGE_JSON,
    errors::{CommandError, ParseError},
    workspace,
};

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct ForeachHandler {
    script: String,
    script_args: Vec<String>,
    filter: Option<String>,
    bail: bool,
}

#[async_trait]
impl CommandHandler for ForeachHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        let mut rest: Vec<String> = args.collect();

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
                "--bail" => {
                    self.bail = true;
                    rest.remove(i);
                }
                "--" => {
                    self.script_args = rest[i + 1..].to_vec();
                    rest.truncate(i);
                    break;
                }
                _ => i += 1,
            }
        }

        let mut pos = rest.into_iter();
        let first = pos
            .next()
            .ok_or_else(|| ParseError::MissingArgument("<script>".to_string()))?;

        if first == "run" {
            self.script = pos
                .next()
                .ok_or_else(|| ParseError::MissingArgument("<script>".to_string()))?;
        } else {
            self.script = first;
        }

        self.script_args.extend(pos);

        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let root = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
        let packages = workspace::discover(&root)?;

        let matched: Vec<_> = match &self.filter {
            Some(f) => workspace::apply_filter(&packages, f),
            None => packages.iter().collect(),
        };

        if matched.is_empty() {
            if self.filter.is_some() {
                println!(
                    "No workspace packages matched filter '{}'.",
                    self.filter.as_deref().unwrap_or("")
                );
            } else {
                println!("No workspace packages found.");
            }
            return Ok(());
        }

        let total = matched.len();
        let mut failures = 0usize;

        for pkg in &matched {
            let pkg_json_raw = std::fs::read_to_string(pkg.path.join(PACKAGE_JSON))
                .map_err(CommandError::FailedToReadFile)?;
            let pkg_json: Value =
                serde_json::from_str(&pkg_json_raw).map_err(CommandError::ParsingFailed)?;

            let script_cmd = pkg_json
                .get("scripts")
                .and_then(|s| s.get(&self.script))
                .and_then(|v| v.as_str());

            let Some(cmd) = script_cmd else {
                println!(
                    "\n[{}] script '{}' not found — skipping",
                    pkg.name, self.script
                );
                continue;
            };

            println!("\n[{}] $ {}", pkg.name, cmd);

            let status = workspace::run_script_in_dir(&pkg.path, cmd, &self.script_args)?;

            if !status.success() {
                let code = status.code().unwrap_or(1);
                println!("[{}] exited with code {}", pkg.name, code);
                failures += 1;
                if self.bail {
                    return Err(CommandError::ProcessFailed(format!(
                        "'{}' failed in '{}' (exit {})",
                        self.script, pkg.name, code
                    )));
                }
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
