use std::{
    collections::{HashSet, VecDeque},
    io::{BufRead, BufReader},
    path::Path,
    process::Command,
};

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    cache::CACHE_DIRECTORY,
    errors::{CommandError, ParseError},
    http::HTTPRequest,
    installer::Installer,
    types::VersionData,
    util,
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

        // Always resolve the exact version so we can name the isolated dir.
        let version_data = Installer::get_version_data(
            client.clone(),
            &package_name,
            full_version.as_ref(),
            semantic_version.as_ref(),
        )
        .await?;

        let resolved_version = version_data.version.clone();

        // Isolated dir layout:
        //   {CACHE}/dlx/{name}@{version}/
        //     node_modules/{name}/   ← package + all deps, flat
        //     .oxide-complete        ← written only after a clean install
        let dlx_root = format!(
            "{}/dlx/{}",
            *CACHE_DIRECTORY,
            Versions::stringify(&package_name, &resolved_version)
        );
        let modules_dir = format!("{}/node_modules", dlx_root);
        let complete_marker = format!("{}/.oxide-complete", dlx_root);

        if !Path::new(&complete_marker).exists() {
            println!("Fetching {}…", self.package_spec);
            let _ = std::fs::remove_dir_all(&dlx_root);
            std::fs::create_dir_all(&modules_dir).map_err(CommandError::FailedToCreateFile)?;
            install_flat(&client, version_data, &modules_dir).await?;
            std::fs::write(&complete_marker, "").map_err(CommandError::FailedToWriteFile)?;
        }

        let pkg_dir = format!("{}/{}", modules_dir, package_name);
        let bin_path =
            resolve_binary(&pkg_dir, &package_name, self.binary_override.as_deref())?;

        let cwd = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;

        // Prepend node_modules/.bin so scripts that exec other bins work.
        let bins_dir = format!("{}/node_modules/.bin", dlx_root);
        let path_sep = if cfg!(windows) { ";" } else { ":" };
        let path_env = format!(
            "{}{}{}",
            bins_dir,
            path_sep,
            std::env::var("PATH").unwrap_or_default()
        );

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
            .map_err(|e| {
                CommandError::GitFailed(format!("failed to spawn '{}': {}", bin_path, e))
            })?;

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


/// Install `root_version_data` and its full transitive dep tree into a flat
/// `{modules_dir}/{pkg_name}/` layout.  Node's module resolution walks up
/// the filesystem, so once every dep lands in the same `node_modules/` dir,
/// they're all visible to the running script with zero symlinking.
async fn install_flat(
    client: &reqwest::Client,
    root_version_data: VersionData,
    modules_dir: &str,
) -> Result<(), CommandError> {
    let mut to_install: VecDeque<(String, String)> = VecDeque::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Bootstrap with the root.
    download_and_extract(client, &root_version_data, modules_dir).await?;
    seen.insert(root_version_data.name.clone());
    enqueue_deps(root_version_data.dependencies, &mut to_install, &seen);

    while let Some((dep_name, dep_spec)) = to_install.pop_front() {
        if seen.contains(&dep_name) {
            continue;
        }
        seen.insert(dep_name.clone());

        let dep_dir = format!("{}/{}", modules_dir, dep_name);

        if Path::new(&dep_dir).exists() {
            // Already on disk — read deps from package.json, no network needed.
            enqueue_deps(read_pkg_json_deps(&dep_dir), &mut to_install, &seen);
            continue;
        }

        let req = match Versions::parse_semantic_version(&dep_spec) {
            Ok(r) => r,
            Err(_) => {
                eprintln!(
                    "Warning: skipping '{}' — unparseable version '{}'",
                    dep_name, dep_spec
                );
                continue;
            }
        };
        let full_v = Versions::resolve_full_version(Some(&req));

        let dep_vd = match Installer::get_version_data(
            client.clone(),
            &dep_name,
            full_v.as_ref(),
            Some(&req),
        )
        .await
        {
            Ok(vd) => vd,
            Err(e) => {
                eprintln!("Warning: failed to resolve '{}': {}", dep_name, e);
                continue;
            }
        };

        let sub_deps = dep_vd.dependencies.clone();
        download_and_extract(client, &dep_vd, modules_dir).await?;
        enqueue_deps(sub_deps, &mut to_install, &seen);
    }

    Ok(())
}

async fn download_and_extract(
    client: &reqwest::Client,
    version_data: &VersionData,
    modules_dir: &str,
) -> Result<(), CommandError> {
    let dest = format!("{}/{}", modules_dir, version_data.name);
    if Path::new(&dest).exists() {
        return Ok(());
    }

    let bytes =
        HTTPRequest::get_bytes(client.clone(), version_data.dist.tarball.clone()).await?;

    if !version_data.dist.verify(&bytes) {
        eprintln!("'{}': integrity check failed, skipping", version_data.name);
        return Ok(());
    }

    std::fs::create_dir_all(&dest).map_err(CommandError::FailedToCreateFile)?;
    util::extract_tarball_strip(bytes, &dest)?;
    println!("Installed '{}'", version_data.name);
    Ok(())
}

fn enqueue_deps(
    deps: Option<std::collections::HashMap<String, String>>,
    queue: &mut VecDeque<(String, String)>,
    seen: &HashSet<String>,
) {
    for (name, spec) in deps.unwrap_or_default() {
        if !seen.contains(&name) {
            queue.push_back((name, spec));
        }
    }
}

fn read_pkg_json_deps(
    pkg_dir: &str,
) -> Option<std::collections::HashMap<String, String>> {
    let raw = std::fs::read_to_string(format!("{}/package.json", pkg_dir)).ok()?;
    let json: Value = serde_json::from_str(&raw).ok()?;
    let map = json.get("dependencies")?.as_object()?;
    Some(
        map.iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect(),
    )
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


