use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{
    constants::{BIN_DIR, NODE_MODULES, PACKAGE_JSON},
    errors::CommandError,
};

#[derive(Debug, Clone)]
pub struct WorkspacePackage {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub scripts: Vec<String>,
}

pub fn discover(root: &Path) -> Result<Vec<WorkspacePackage>, CommandError> {
    let raw =
        std::fs::read_to_string(root.join(PACKAGE_JSON)).map_err(CommandError::FailedToReadFile)?;
    let json: Value = serde_json::from_str(&raw).map_err(CommandError::ParsingFailed)?;

    let patterns = extract_patterns(&json);
    if patterns.is_empty() {
        return Err(CommandError::GitFailed(
            "no 'workspaces' field found in package.json — are you in the monorepo root?"
                .to_string(),
        ));
    }

    let mut packages = Vec::new();
    for pattern in &patterns {
        for dir in expand_glob(root, pattern) {
            if let Ok(pkg) = read_package(&dir) {
                packages.push(pkg);
            }
        }
    }

    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packages)
}

fn extract_patterns(json: &Value) -> Vec<String> {
    if let Some(arr) = json.get("workspaces").and_then(|w| w.as_array()) {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }
    if let Some(pkgs) = json
        .get("workspaces")
        .and_then(|w| w.get("packages"))
        .and_then(|p| p.as_array())
    {
        return pkgs
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }
    Vec::new()
}

fn expand_glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    if let Some(prefix) = pattern.strip_suffix("/*") {
        if !crate::util::is_safe_path_component(prefix) {
            return Vec::new();
        }
        let dir = root.join(prefix);
        return std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let p = e.path();
                p.is_dir() && p.join(PACKAGE_JSON).exists()
            })
            .map(|e| e.path())
            .collect();
    }
    if !pattern.contains('*') {
        if !crate::util::is_safe_path_component(pattern) {
            return Vec::new();
        }
        let path = root.join(pattern);
        if path.is_dir() && path.join(PACKAGE_JSON).exists() {
            return vec![path];
        }
    }
    Vec::new()
}

fn read_package(dir: &Path) -> Result<WorkspacePackage, CommandError> {
    let raw =
        std::fs::read_to_string(dir.join(PACKAGE_JSON)).map_err(CommandError::FailedToReadFile)?;
    let json: Value = serde_json::from_str(&raw).map_err(CommandError::ParsingFailed)?;

    let name = json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("(unnamed)")
        .to_string();
    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();
    let scripts: Vec<String> = json
        .get("scripts")
        .and_then(|s| s.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();

    Ok(WorkspacePackage {
        name,
        version,
        path: dir.to_path_buf(),
        scripts,
    })
}

pub fn apply_filter<'a>(
    packages: &'a [WorkspacePackage],
    filter: &str,
) -> Vec<&'a WorkspacePackage> {
    let pattern = filter.trim_end_matches("...");

    packages
        .iter()
        .filter(|pkg| {
            if pkg.name == pattern {
                return true;
            }
            if let Some(prefix) = pattern
                .strip_suffix("/*")
                .or_else(|| pattern.strip_suffix('*'))
            {
                if pkg.name.starts_with(prefix) {
                    return true;
                }
                if pkg
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains(prefix.trim_end_matches('/'))
                {
                    return true;
                }
            }
            if pkg
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .contains(pattern)
            {
                return true;
            }
            false
        })
        .collect()
}

pub fn run_script_in_dir(
    dir: &Path,
    script_cmd: &str,
    extra_args: &[String],
) -> Result<std::process::ExitStatus, CommandError> {
    let local_bin = dir.join(NODE_MODULES).join(BIN_DIR);
    let path_env = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ";" } else { ":" };
    let new_path = if local_bin.exists() {
        format!("{}{}{}", local_bin.to_string_lossy(), sep, path_env)
    } else {
        path_env
    };

    let full_cmd = if extra_args.is_empty() {
        script_cmd.to_string()
    } else {
        format!("{} {}", script_cmd, extra_args.join(" "))
    };

    #[cfg(windows)]
    let status = std::process::Command::new("cmd")
        .args(["/C", &full_cmd])
        .env("PATH", &new_path)
        .current_dir(dir)
        .status()
        .map_err(|e| CommandError::ProcessFailed(format!("failed to spawn shell: {e}")))?;

    #[cfg(not(windows))]
    let status = std::process::Command::new("/bin/sh")
        .args(["-c", &full_cmd])
        .env("PATH", &new_path)
        .current_dir(dir)
        .status()
        .map_err(|e| CommandError::ProcessFailed(format!("failed to spawn shell: {e}")))?;

    Ok(status)
}
