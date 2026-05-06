use std::env::Args;

use async_trait::async_trait;
use serde_json::Value;

use crate::errors::{CommandError, ParseError};
use crate::http::REGISTRY_URL;

use super::command_handler::CommandHandler;
use super::login::load_token;

#[derive(Default)]
pub struct DoctorHandler;

struct Check {
    label: &'static str,
    passed: bool,
    detail: String,
}

impl Check {
    fn pass(label: &'static str, detail: impl Into<String>) -> Self {
        Self { label, passed: true, detail: detail.into() }
    }
    fn fail(label: &'static str, detail: impl Into<String>) -> Self {
        Self { label, passed: false, detail: detail.into() }
    }
}

#[async_trait]
impl CommandHandler for DoctorHandler {
    fn parse(&mut self, _args: &mut Args) -> Result<(), ParseError> {
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let mut checks: Vec<Check> = Vec::new();

        // 1. package.json readable + valid JSON
        let pkg_json: Option<Value> = match std::fs::read_to_string("./package.json") {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(v) => {
                    checks.push(Check::pass("package.json", "found and valid"));
                    Some(v)
                }
                Err(e) => {
                    checks.push(Check::fail("package.json", format!("invalid JSON — {}", e)));
                    None
                }
            },
            Err(_) => {
                checks.push(Check::fail("package.json", "not found in current directory"));
                None
            }
        };

        // 2. node_modules exists
        if std::path::Path::new("./node_modules").exists() {
            checks.push(Check::pass("node_modules", "present"));
        } else {
            checks.push(Check::fail(
                "node_modules",
                "missing — run `oxide install <package>` to install dependencies",
            ));
        }

        // 3. All declared dependencies are installed
        if let Some(ref json) = pkg_json {
            let (dep_checks, missing) = verify_deps(json);
            checks.extend(dep_checks);

            if missing == 0 {
                checks.push(Check::pass("dependencies", "all installed"));
            } else {
                checks.push(Check::fail(
                    "dependencies",
                    format!("{} package(s) missing from node_modules", missing),
                ));
            }
        }

        // 4. Registry reachable
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(CommandError::HTTPFailed)?;

        match client.get(REGISTRY_URL).send().await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 404 => {
                checks.push(Check::pass("registry", format!("reachable ({})", REGISTRY_URL)));
            }
            Ok(resp) => {
                checks.push(Check::fail(
                    "registry",
                    format!("responded with HTTP {}", resp.status()),
                ));
            }
            Err(e) => {
                checks.push(Check::fail("registry", format!("unreachable — {}", e)));
            }
        }

        // 5. Auth token present
        match load_token() {
            Some(_) => checks.push(Check::pass("auth token", "found in credential store")),
            None => checks.push(Check::fail(
                "auth token",
                "not found — run `oxide login` if you need to publish or install private packages",
            )),
        }

        // Print results
        println!("{:<20} {:<8} {}", "Check", "Status", "Detail");
        println!("{}", "-".repeat(72));

        let mut failures = 0usize;
        for check in &checks {
            let status = if check.passed { "pass" } else { "FAIL" };
            println!("{:<20} {:<8} {}", check.label, status, check.detail);
            if !check.passed {
                failures += 1;
            }
        }

        println!();
        if failures == 0 {
            println!("All checks passed.");
        } else {
            println!("{} check(s) failed.", failures);
        }

        Ok(())
    }
}

/// Returns a list of per-package checks and the count of missing packages.
/// Only reports failures to keep output concise.
fn verify_deps(json: &Value) -> (Vec<Check>, usize) {
    let mut checks = Vec::new();
    let mut missing = 0usize;

    for field in ["dependencies", "devDependencies"] {
        let Some(deps) = json.get(field).and_then(|d| d.as_object()) else {
            continue;
        };
        for (name, _constraint) in deps {
            let path = format!("./node_modules/{}/package.json", name);
            if !std::path::Path::new(&path).exists() {
                checks.push(Check::fail(
                    "missing package",
                    format!("'{}' not installed ({})", name, field),
                ));
                missing += 1;
            }
        }
    }

    (checks, missing)
}
