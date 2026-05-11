
use async_trait::async_trait;
use serde_json::Value;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct LsHandler {
    all: bool, // include devDependencies
}

struct InstalledPkg {
    name: String,
    wanted: String,  // version constraint from package.json
    installed: Option<String>, // actual version in node_modules
}

#[async_trait]
impl CommandHandler for LsHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        for arg in args {
            match arg.as_str() {
                "--all" | "-a" | "--dev" => self.all = true,
                _ => {}
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let raw = std::fs::read_to_string("./package.json")
            .map_err(CommandError::FailedToReadFile)?;
        let json: Value = serde_json::from_str(&raw).map_err(CommandError::ParsingFailed)?;

        let project_name = json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        let project_version = json
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0");

        println!("{}@{}", project_name, project_version);

        let mut deps = collect_deps(&json, "dependencies", false);
        if self.all {
            deps.extend(collect_deps(&json, "devDependencies", true));
        }

        if deps.is_empty() {
            println!("  (no dependencies)");
            return Ok(());
        }

        let total = deps.len();
        let mut missing = 0usize;

        for (i, pkg) in deps.iter().enumerate() {
            let is_last = i == total - 1;
            let branch = if is_last { "└──" } else { "├──" };

            match &pkg.installed {
                Some(ver) => println!("  {} {}@{} (wanted {})", branch, pkg.name, ver, pkg.wanted),
                None => {
                    println!("  {} {}@MISSING (wanted {})", branch, pkg.name, pkg.wanted);
                    missing += 1;
                }
            }
        }

        if missing > 0 {
            println!("\n{} package(s) missing — run `oxide install` to fix.", missing);
        }

        Ok(())
    }
}

fn collect_deps(json: &Value, field: &str, is_dev: bool) -> Vec<InstalledPkg> {
    let Some(deps) = json.get(field).and_then(|d| d.as_object()) else {
        return Vec::new();
    };

    let mut result: Vec<InstalledPkg> = deps
        .iter()
        .map(|(name, constraint)| {
            let wanted = if is_dev {
                format!("{} (dev)", constraint.as_str().unwrap_or("*"))
            } else {
                constraint.as_str().unwrap_or("*").to_string()
            };
            let installed = read_installed_version(name);
            InstalledPkg {
                name: name.clone(),
                wanted,
                installed,
            }
        })
        .collect();

    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

fn read_installed_version(package_name: &str) -> Option<String> {
    if !crate::util::is_safe_path_component(package_name) {
        return None;
    }
    let path = format!("./node_modules/{}/package.json", package_name);
    let raw = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&raw).ok()?;
    json.get("version")?.as_str().map(|s| s.to_string())
}
