use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
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

#[derive(Default)]
pub struct DlxHandler {
    package_spec: String,
    binary_override: Option<String>,
    bin_args: Vec<String>,
}

#[async_trait]
impl CommandHandler for DlxHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        self.package_spec = args
            .next()
            .ok_or_else(|| ParseError::MissingArgument("<package>".to_string()))?;

        let rest: Vec<String> = args.collect();
        if let Some(first) = rest.first() {
            if !first.starts_with('-') {
                self.binary_override = Some(first.clone());
                self.bin_args = rest[1..].to_vec();
                return Ok(());
            }
        }
        self.bin_args = rest;
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let (package_name, semantic_version) =
            Versions::parse_semantic_package_details(self.package_spec.clone())
                .map_err(|e| CommandError::GitFailed(e.to_string()))?;

        let client = reqwest::Client::new();
        let full_version = Versions::resolve_full_version(semantic_version.as_ref());
        let full_version_ref = full_version.as_ref();

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

        let bin_path = resolve_binary(&package_dir, &package_name, self.binary_override.as_deref())?;

        let cwd = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
        let path_env = std::env::var("PATH").unwrap_or_default();

        let is_js = bin_path.ends_with(".js")
            || bin_path.ends_with(".cjs")
            || bin_path.ends_with(".mjs");

        let mut cmd = if is_js {
            let interpreter = resolve_interpreter(&bin_path);
            let mut c = Command::new(&interpreter);
            c.arg(&bin_path);
            c
        } else {
            Command::new(&bin_path)
        };

        let status = cmd
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


pub fn resolve_binary(
    package_dir: &str,
    package_name: &str,
    binary_override: Option<&str>,
) -> Result<String, CommandError> {
    let pkg_json_path = format!("{}/package.json", package_dir);
    let raw = std::fs::read_to_string(&pkg_json_path)
        .map_err(CommandError::FailedToReadFile)?;
    let json: Value = serde_json::from_str(&raw).map_err(CommandError::ParsingFailed)?;

    let short_name = package_name
        .split('/')
        .last()
        .unwrap_or(package_name);

    match json.get("bin") {
        Some(Value::String(rel)) => {
            return Ok(format!("{}/{}", package_dir, rel.trim_start_matches("./")));
        }
        Some(Value::Object(map)) => {
            let key = binary_override.unwrap_or(short_name);
            if let Some(Value::String(rel)) = map.get(key) {
                return Ok(format!("{}/{}", package_dir, rel.trim_start_matches("./")));
            }
            if let Some((_, Value::String(rel))) = map.iter().next() {
                return Ok(format!("{}/{}", package_dir, rel.trim_start_matches("./")));
            }
        }
        _ => {}
    }

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

pub fn resolve_interpreter(bin_path: &str) -> String {
    if let Ok(file) = std::fs::File::open(bin_path) {
        let mut reader = BufReader::new(file);
        let mut first_line = String::new();
        if reader.read_line(&mut first_line).is_ok() {
            if let Some(interp) = parse_shebang(&first_line) {
                return interp;
            }
        }
    }
    "node".to_string()
}


pub fn parse_shebang(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with("#!") {
        return None;
    }
    let rest = line[2..].trim();
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let prog = if parts[0].ends_with("/env") || parts[0] == "env" {
        parts.get(1).copied()?
    } else {
        parts[0]
    };
    prog.split('/').last().map(|s| s.to_string())
}


