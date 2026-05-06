use std::{
    collections::HashMap,
    env::Args,
    process::Command,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    cache::{Cache, CACHE_DIRECTORY},
    errors::{CommandError, ParseError},
    installer::{InstallContext, Installer, PackageInfo},
    util::TaskAllocator,
    versions::Versions,
};

use super::command_handler::CommandHandler;

/// `oxide dlx <package[@version]> [binary] [args...]`
///
/// Downloads and runs a package binary without permanently adding it to package.json.
/// The package is cached so subsequent runs are fast.
#[derive(Default)]
pub struct DlxHandler {
    package_spec: String,       // e.g. "typescript" or "typescript@5.4.0"
    binary_override: Option<String>, // explicit binary name if different from package
    bin_args: Vec<String>,
}

#[async_trait]
impl CommandHandler for DlxHandler {
    fn parse(&mut self, args: &mut Args) -> Result<(), ParseError> {
        self.package_spec = args
            .next()
            .ok_or_else(|| ParseError::MissingArgument("<package>".to_string()))?;

        // Remaining args: optional explicit binary name + args to forward.
        // If first remaining arg doesn't start with '-' and the package has multiple
        // binaries, treat it as the binary name; otherwise everything goes to bin_args.
        let rest: Vec<String> = args.collect();
        if let Some(first) = rest.first() {
            if !first.starts_with('-') {
                // Peek whether the cached/to-be-installed package has a binary
                // with this exact name — we can't know yet, so we store it tentatively
                // and resolve at execute time.
                self.binary_override = Some(first.clone());
                self.bin_args = rest[1..].to_vec();
                return Ok(());
            }
        }
        self.bin_args = rest;
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        // Parse package name + optional version constraint
        let (package_name, semantic_version) =
            Versions::parse_semantic_package_details(self.package_spec.clone())
                .map_err(|e| CommandError::GitFailed(e.to_string()))?;

        let client = reqwest::Client::new();
        let full_version = Versions::resolve_full_version(semantic_version.as_ref());
        let full_version_ref = full_version.as_ref();

        // Install to cache (no-op if already cached)
        let (is_cached, cached_version) =
            Cache::exists(&package_name, full_version_ref, semantic_version.as_ref()).await?;

        let resolved_version = if is_cached {
            cached_version.expect("cached version must be known")
        } else {
            println!("Fetching {}…", self.package_spec);
            let version_data = Installer::get_version_data(
                client.clone(),
                &package_name,
                full_version_ref,
                semantic_version.as_ref(),
            )
            .await?;

            let version = version_data.version.clone();
            let stringified = Versions::stringify(&version_data.name, &version_data.version);

            let dep_map = Arc::new(Mutex::new(HashMap::new()));
            let ctx = InstallContext {
                client: client.clone(),
                dependency_map_mux: Arc::clone(&dep_map),
            };
            let pkg_info = PackageInfo {
                is_latest: Versions::is_latest(full_version_ref),
                stringified,
                version_data,
            };
            Installer::install_package(ctx, pkg_info, Arc::new(Mutex::new(Vec::new())))?;
            TaskAllocator::block_until_done();

            version
        };

        let stringified = Versions::stringify(&package_name, &resolved_version);
        let package_dir = format!("{}/{}/package", *CACHE_DIRECTORY, stringified);

        // Determine which binary to run
        let bin_path = resolve_binary(&package_dir, &package_name, self.binary_override.as_deref())?;

        let cwd = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
        let path_env = std::env::var("PATH").unwrap_or_default();

        let status = Command::new(&bin_path)
            .args(&self.bin_args)
            .env("PATH", &path_env)
            .current_dir(&cwd)
            .status()
            .map_err(|e| CommandError::GitFailed(format!("failed to spawn '{}': {}", bin_path, e)))?;

        if !status.success() {
            let code = status.code().unwrap_or(1);
            return Err(CommandError::GitFailed(format!(
                "process exited with code {}",
                code
            )));
        }

        Ok(())
    }
}

/// Reads the `bin` field from the package's own `package.json` and returns the
/// absolute path to the binary script to run.
fn resolve_binary(
    package_dir: &str,
    package_name: &str,
    binary_override: Option<&str>,
) -> Result<String, CommandError> {
    let pkg_json_path = format!("{}/package.json", package_dir);
    let raw = std::fs::read_to_string(&pkg_json_path)
        .map_err(CommandError::FailedToReadFile)?;
    let json: Value = serde_json::from_str(&raw).map_err(CommandError::ParsingFailed)?;

    // Derive the short name (without @scope prefix) for default bin lookup
    let short_name = package_name
        .split('/')
        .last()
        .unwrap_or(package_name);

    match json.get("bin") {
        // "bin": "path/to/cli"
        Some(Value::String(rel)) => {
            return Ok(format!("{}/{}", package_dir, rel.trim_start_matches("./")));
        }
        // "bin": { "cmd": "path/to/cmd", ... }
        Some(Value::Object(map)) => {
            let key = binary_override.unwrap_or(short_name);
            if let Some(Value::String(rel)) = map.get(key) {
                return Ok(format!("{}/{}", package_dir, rel.trim_start_matches("./")));
            }
            // fall back to first entry
            if let Some((_, Value::String(rel))) = map.iter().next() {
                return Ok(format!("{}/{}", package_dir, rel.trim_start_matches("./")));
            }
        }
        _ => {}
    }

    // Last resort: look for an executable matching the package short name
    let candidate = format!("{}/bin/{}", package_dir, short_name);
    if std::path::Path::new(&candidate).exists() {
        return Ok(candidate);
    }

    Err(CommandError::GitFailed(format!(
        "could not find a binary entry point in '{}'. \
         Try specifying the binary name: oxide dlx {} <binary>",
        pkg_json_path, package_name
    )))
}
