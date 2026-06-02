use async_trait::async_trait;
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::errors::{CommandError, ParseError};
use crate::http::HTTPRequest;

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct OutdatedHandler;

struct PackageStatus {
    name: String,
    current: Option<String>,
    latest: String,
}

#[async_trait]
impl CommandHandler for OutdatedHandler {
    fn parse(&mut self, _args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let content =
            std::fs::read_to_string("./package.json").map_err(CommandError::FailedToReadFile)?;
        let json: Value = serde_json::from_str(&content).map_err(CommandError::ParsingFailed)?;

        let mut dep_names: Vec<String> = Vec::new();
        for key in ["dependencies", "devDependencies"] {
            if let Some(deps) = json.get(key).and_then(|d| d.as_object()) {
                dep_names.extend(deps.keys().cloned());
            }
        }

        if dep_names.is_empty() {
            println!("No dependencies found in package.json.");
            return Ok(());
        }

        let client = reqwest::Client::new();
        let latest_str = "latest".to_string();

        let tasks: Vec<JoinHandle<Option<PackageStatus>>> = dep_names
            .into_iter()
            .map(|name| {
                let client = client.clone();
                let latest_str = latest_str.clone();
                tokio::spawn(async move {
                    let current = read_installed_version(&name);
                    match HTTPRequest::version_data(client, &name, &latest_str).await {
                        Ok(data) => {
                            let latest = data.version;
                            let is_outdated = current.as_deref() != Some(latest.as_str());
                            if is_outdated {
                                Some(PackageStatus {
                                    name,
                                    current,
                                    latest,
                                })
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    }
                })
            })
            .collect();

        let mut outdated: Vec<PackageStatus> = Vec::new();
        for task in tasks {
            if let Ok(Some(status)) = task.await {
                outdated.push(status);
            }
        }

        if outdated.is_empty() {
            println!("All packages are up to date.");
            return Ok(());
        }

        outdated.sort_by(|a, b| a.name.cmp(&b.name));

        println!("{:<30} {:<15} Latest", "Package", "Current");
        println!("{}", "-".repeat(60));

        for status in &outdated {
            let current = status.current.as_deref().unwrap_or("—");
            println!("{:<30} {:<15} {}", status.name, current, status.latest);
        }

        println!("\n{} package(s) outdated.", outdated.len());

        Ok(())
    }
}

fn read_installed_version(package_name: &str) -> Option<String> {
    if !crate::util::is_safe_path_component(package_name) {
        return None;
    }
    let path = format!("./node_modules/{}/package.json", package_name);
    let content = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&content).ok()?;
    json.get("version")?.as_str().map(|s| s.to_string())
}
