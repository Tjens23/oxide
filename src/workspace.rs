use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::errors::CommandError;


#[derive(Debug, Clone)]
pub struct WorkspacePackage {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub scripts: Vec<String>,
}

pub fn discover(root: &Path) -> Result<Vec<WorkspacePackage>, CommandError> {
    let raw = std::fs::read_to_string(root.join("package.json"))
        .map_err(CommandError::FailedToReadFile)?;
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
        let dir = root.join(prefix);
        return std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let p = e.path();
                p.is_dir() && p.join("package.json").exists()
            })
            .map(|e| e.path())
            .collect();
    }
    if !pattern.contains('*') {
        let path = root.join(pattern);
        if path.is_dir() && path.join("package.json").exists() {
            return vec![path];
        }
    }
    Vec::new()
}

fn read_package(dir: &Path) -> Result<WorkspacePackage, CommandError> {
    let raw = std::fs::read_to_string(dir.join("package.json"))
        .map_err(CommandError::FailedToReadFile)?;
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
            if let Some(prefix) = pattern.strip_suffix("/*").or_else(|| pattern.strip_suffix('*')) {
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
    let local_bin = dir.join("node_modules").join(".bin");
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
        .map_err(|e| CommandError::GitFailed(format!("failed to spawn shell: {e}")))?;

    #[cfg(not(windows))]
    let status = std::process::Command::new("/bin/sh")
        .args(["-c", &full_cmd])
        .env("PATH", &new_path)
        .current_dir(dir)
        .status()
        .map_err(|e| CommandError::GitFailed(format!("failed to spawn shell: {e}")))?;

    Ok(status)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn make_pkg(name: &str, path: &str) -> WorkspacePackage {
        WorkspacePackage {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            path: PathBuf::from(path),
            scripts: vec!["build".to_string(), "test".to_string()],
        }
    }

    fn sample_packages() -> Vec<WorkspacePackage> {
        vec![
            make_pkg("@myorg/ui", "packages/ui"),
            make_pkg("@myorg/api", "packages/api"),
            make_pkg("cli-tool", "apps/cli"),
            make_pkg("backend", "apps/backend"),
        ]
    }

    // ── apply_filter ──────────────────────────────────────────────────────────

    #[test]
    fn filter_exact_name() {
        let pkgs = sample_packages();
        let result = apply_filter(&pkgs, "@myorg/ui");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "@myorg/ui");
    }

    #[test]
    fn filter_scope_wildcard() {
        let pkgs = sample_packages();
        let result = apply_filter(&pkgs, "@myorg/*");
        assert_eq!(result.len(), 2);
        let names: Vec<_> = result.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"@myorg/ui"));
        assert!(names.contains(&"@myorg/api"));
    }

    #[test]
    fn filter_path_substring() {
        let pkgs = sample_packages();
        let result = apply_filter(&pkgs, "apps");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_trailing_ellipsis_stripped() {
        let pkgs = sample_packages();
        // "@myorg/ui..." should match the same as "@myorg/ui"
        let result = apply_filter(&pkgs, "@myorg/ui...");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "@myorg/ui");
    }

    #[test]
    fn filter_no_match_returns_empty() {
        let pkgs = sample_packages();
        let result = apply_filter(&pkgs, "nonexistent");
        assert!(result.is_empty());
    }

    #[test]
    fn filter_empty_packages_returns_empty() {
        let pkgs: Vec<WorkspacePackage> = vec![];
        let result = apply_filter(&pkgs, "@myorg/ui");
        assert!(result.is_empty());
    }

    // ── discover ──────────────────────────────────────────────────────────────

    fn write_pkg_json(dir: &std::path::Path, content: &str) {
        fs::write(dir.join("package.json"), content).unwrap();
    }

    #[test]
    fn discover_resolves_glob_pattern() {
        let tmp = std::env::temp_dir().join(format!(
            "oxide-ws-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(tmp.join("packages/alpha")).unwrap();
        fs::create_dir_all(tmp.join("packages/beta")).unwrap();

        write_pkg_json(
            &tmp,
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        );
        write_pkg_json(
            &tmp.join("packages/alpha"),
            r#"{"name":"@mono/alpha","version":"1.0.0"}"#,
        );
        write_pkg_json(
            &tmp.join("packages/beta"),
            r#"{"name":"@mono/beta","version":"2.0.0"}"#,
        );

        let packages = discover(&tmp).unwrap();
        assert_eq!(packages.len(), 2);

        let names: Vec<_> = packages.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"@mono/alpha"));
        assert!(names.contains(&"@mono/beta"));

        // Sorted by name
        assert!(packages[0].name <= packages[1].name);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn discover_returns_error_without_workspaces_field() {
        let tmp = std::env::temp_dir().join(format!(
            "oxide-ws-nofield-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap();
        write_pkg_json(&tmp, r#"{"name":"root","version":"1.0.0"}"#);

        let result = discover(&tmp);
        assert!(result.is_err());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn discover_yarn_classic_workspaces_object() {
        let tmp = std::env::temp_dir().join(format!(
            "oxide-ws-yarn-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(tmp.join("pkgs/core")).unwrap();
        write_pkg_json(
            &tmp,
            r#"{"name":"root","workspaces":{"packages":["pkgs/*"]}}"#,
        );
        write_pkg_json(
            &tmp.join("pkgs/core"),
            r#"{"name":"core","version":"0.1.0"}"#,
        );

        let packages = discover(&tmp).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "core");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn discover_packages_include_scripts() {
        let tmp = std::env::temp_dir().join(format!(
            "oxide-ws-scripts-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(tmp.join("packages/lib")).unwrap();
        write_pkg_json(&tmp, r#"{"name":"root","workspaces":["packages/*"]}"#);
        write_pkg_json(
            &tmp.join("packages/lib"),
            r#"{"name":"lib","version":"1.0.0","scripts":{"build":"tsc","test":"jest"}}"#,
        );

        let packages = discover(&tmp).unwrap();
        assert_eq!(packages.len(), 1);
        assert!(packages[0].scripts.contains(&"build".to_string()));
        assert!(packages[0].scripts.contains(&"test".to_string()));

        fs::remove_dir_all(&tmp).unwrap();
    }
}
