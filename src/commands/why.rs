
use async_trait::async_trait;
use serde_json::Value;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct WhyHandler {
    package_name: String,
}

#[async_trait]
impl CommandHandler for WhyHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        self.package_name = args
            .next()
            .ok_or_else(|| ParseError::MissingArgument("<package>".to_string()))?;
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let target = &self.package_name;

        // Read the project's package.json
        let pkg_raw = std::fs::read_to_string("./package.json")
            .map_err(CommandError::FailedToReadFile)?;
        let pkg_json: Value =
            serde_json::from_str(&pkg_raw).map_err(CommandError::ParsingFailed)?;

        let in_deps = pkg_json
            .get("dependencies")
            .and_then(|d| d.get(target))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let in_dev_deps = pkg_json
            .get("devDependencies")
            .and_then(|d| d.get(target))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Check whether the package is actually installed
        let installed_version = read_installed_version(target);
        if installed_version.is_none() && in_deps.is_none() && in_dev_deps.is_none() {
            println!("'{}' is not installed and not listed in package.json.", target);
            return Ok(());
        }

        // Print direct dependency status
        if let Some(ref constraint) = in_deps {
            println!("'{}' is a direct dependency  ({})", target, constraint);
        }
        if let Some(ref constraint) = in_dev_deps {
            println!("'{}' is a dev dependency     ({})", target, constraint);
        }

        if let Some(ref ver) = installed_version {
            println!("Installed version: {}", ver);
        } else {
            println!("Not found in node_modules — run `oxide install`.");
        }

        // Scan node_modules to find transitive dependents
        let nm = std::path::Path::new("./node_modules");
        if !nm.exists() {
            return Ok(());
        }

        let mut dependents: Vec<(String, String)> = Vec::new(); // (package, version_constraint)

        for entry in std::fs::read_dir(nm).map_err(CommandError::FailedToReadFile)? {
            let entry = entry.map_err(CommandError::FailedDirectoryEntry)?;
            let path = entry.path();
            let dir_name = entry.file_name().to_string_lossy().to_string();

            // Handle scoped packages (@scope/pkg)
            let pkg_jsons: Vec<std::path::PathBuf> = if dir_name.starts_with('@') && path.is_dir() {
                match std::fs::read_dir(&path) {
                    Ok(inner) => inner
                        .flatten()
                        .map(|e| e.path().join("package.json"))
                        .collect(),
                    Err(_) => continue,
                }
            } else {
                vec![path.join("package.json")]
            };

            for pkg_json_path in pkg_jsons {
                let Ok(raw) = std::fs::read_to_string(&pkg_json_path) else {
                    continue;
                };
                let Ok(json) = serde_json::from_str::<Value>(&raw) else {
                    continue;
                };

                let owner_name = json
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();

                if owner_name == *target {
                    continue; // skip the package itself
                }

                if let Some(constraint) = json
                    .get("dependencies")
                    .and_then(|d| d.get(target))
                    .and_then(|v| v.as_str())
                {
                    dependents.push((owner_name, constraint.to_string()));
                }
            }
        }

        if dependents.is_empty() {
            if in_deps.is_none() && in_dev_deps.is_none() {
                println!(
                    "\nNo installed package depends on '{}'. It may be a ghost install.",
                    target
                );
            }
        } else {
            println!("\nRequired by:");
            for (name, constraint) in &dependents {
                println!("  {} ({})", name, constraint);
            }
        }

        Ok(())
    }
}

fn read_installed_version(package_name: &str) -> Option<String> {
    let path = format!("./node_modules/{}/package.json", package_name);
    let raw = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&raw).ok()?;
    json.get("version")?.as_str().map(|s| s.to_string())
}
