use std::{
    collections::HashMap,
    fs::{self as fs_sync},
    path::PathBuf,
    str::FromStr,
};

use lazy_static::lazy_static;
use semver::Version;
use tokio::fs;

use crate::{
    constants::{CACHE_SUBDIR, NODE_MODULES, OXIDE_LOCK},
    errors::CommandError,
    types::PackageLock,
    versions::{EMPTY_VERSION, LATEST, Versions},
};
use semver::VersionReq;

pub struct CachedVersion {
    pub version: String,
    pub is_latest: bool,
}

pub type CachedVersions = HashMap<String, CachedVersion>;

fn init_cache_dir() -> String {
    match dirs::cache_dir().and_then(|p| p.to_str().map(|s| format!("{}/{}", s, CACHE_SUBDIR))) {
        Some(dir) => dir,
        None => {
            eprintln!("Fatal: could not determine system cache directory");
            std::process::exit(1);
        }
    }
}

lazy_static! {
    pub static ref CACHE_DIRECTORY: String = init_cache_dir();
    pub static ref CACHED_VERSIONS: CachedVersions = Cache::get_cached_versions();
}

pub struct Cache;
impl Cache {
    pub fn get_cached_versions() -> CachedVersions {
        if let Err(e) = fs_sync::create_dir_all(CACHE_DIRECTORY.as_str()) {
            eprintln!("Warning: could not create cache directory: {e}");
            return HashMap::new();
        }

        let dir_contents = match fs_sync::read_dir(CACHE_DIRECTORY.as_str()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Warning: could not read cache directory: {e}");
                return HashMap::new();
            }
        };

        let mut all_entries: Vec<String> = Vec::new();
        let mut cached_versions = HashMap::new();

        for entry in dir_contents.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with('@') {
                let scope_dir = PathBuf::from(CACHE_DIRECTORY.as_str()).join(&filename);
                if let Ok(scope_contents) = fs_sync::read_dir(&scope_dir) {
                    for scope_entry in scope_contents.flatten() {
                        let pkg = scope_entry.file_name().to_string_lossy().to_string();
                        all_entries.push(format!("{}/{}", filename, pkg));
                    }
                }
            } else {
                all_entries.push(filename);
            }
        }

        for full_entry in all_entries {
            if !crate::util::is_safe_path_component(&full_entry) {
                continue;
            }
            let lock_path = PathBuf::from(CACHE_DIRECTORY.as_str())
                .join(&full_entry)
                .join("package")
                .join(OXIDE_LOCK);

            let is_latest = match fs_sync::read_to_string(&lock_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<PackageLock>(&raw).ok())
            {
                Some(lf) => lf.is_latest,
                None => continue,
            };

            let (name, version) = Versions::parse_raw_package_details(full_entry);
            cached_versions.insert(name, CachedVersion { version, is_latest });
        }

        cached_versions
    }

    pub async fn exists(
        package_name: &String,
        version: Option<&String>,
        semantic_version: Option<&VersionReq>,
    ) -> Result<(bool, Option<String>), CommandError> {
        if let Some(version) = version {
            if version == LATEST {
                let latest_version = Self::get_latest_version_in_cache(package_name);
                return Ok((latest_version.is_some(), latest_version));
            }

            return Ok((
                Self::is_in_cache(package_name, version),
                Some(version.to_string()),
            ));
        }

        let semantic_version = semantic_version.unwrap();

        if package_name.starts_with('@') {
            if let Some(slash_pos) = package_name.find('/') {
                let scope = &package_name[..slash_pos];
                let pkg_within_scope = &package_name[slash_pos + 1..];
                let scope_dir = PathBuf::from(CACHE_DIRECTORY.as_str()).join(scope);

                if let Ok(mut scope_entries) = fs::read_dir(&scope_dir).await {
                    while let Some(scope_entry) = scope_entries
                        .next_entry()
                        .await
                        .map_err(CommandError::FailedDirectoryEntry)?
                    {
                        let filename = scope_entry.file_name().to_string_lossy().to_string();
                        if !filename.starts_with(pkg_within_scope) {
                            continue;
                        }

                        let full_entry = format!("{}/{}", scope, filename);
                        let (_, entry_version) = Versions::parse_raw_package_details(full_entry);

                        let version =
                            &Version::from_str(entry_version.as_str()).unwrap_or(EMPTY_VERSION);
                        if semantic_version.matches(version) {
                            return Ok((true, Some(entry_version)));
                        }
                    }
                }
            }
            return Ok((false, None));
        }

        let mut cache_entries = fs::read_dir(CACHE_DIRECTORY.as_str())
            .await
            .map_err(CommandError::NoCacheDirectory)?;

        while let Some(cache_entry) = cache_entries
            .next_entry()
            .await
            .map_err(CommandError::FailedDirectoryEntry)?
        {
            let filename = cache_entry.file_name().to_string_lossy().to_string();
            if !filename.starts_with(package_name.as_str()) {
                continue;
            }

            let (_, entry_version) = Versions::parse_raw_package_details(filename);

            let version = &Version::from_str(entry_version.as_str()).unwrap_or(EMPTY_VERSION);
            if semantic_version.matches(version) {
                return Ok((true, Some(entry_version)));
            }
        }

        Ok((false, None))
    }

    pub fn is_in_cache(package: &String, version: &String) -> bool {
        let cached_version = CACHED_VERSIONS.get(package);
        match cached_version {
            Some(ver) if &ver.version == version => true,
            _ => false,
        }
    }

    pub fn get_latest_version_in_cache(package_name: &String) -> Option<String> {
        let cached_version = CACHED_VERSIONS.get(package_name);
        match cached_version {
            Some(ver) if ver.is_latest => Some(ver.version.to_string()),
            _ => None,
        }
    }

    pub fn load_cached_version(
        package: String,
        dest_root: &std::path::Path,
    ) -> Result<(), CommandError> {
        if !crate::util::is_safe_path_component(&package) {
            return Err(CommandError::GitFailed(format!(
                "unsafe package path component: {}",
                package
            )));
        }
        let lockfile_path = PathBuf::from(CACHE_DIRECTORY.as_str())
            .join(&package)
            .join("package")
            .join(OXIDE_LOCK);

        // If no lockfile exists (e.g. from a previously failed install), just link the package itself.
        let dependencies: Vec<String> = fs_sync::read_to_string(&lockfile_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<PackageLock>(&raw).ok())
            .map(|lf| lf.dependencies)
            .unwrap_or_default();

        // Create cache-level node_modules so Node.js can resolve deps from
        // the real (cache) path rather than the project's node_modules.
        let cache_root = PathBuf::from(CACHE_DIRECTORY.as_str());
        let cache_nm = cache_root.join(&package).join(NODE_MODULES);
        fs_sync::create_dir_all(&cache_nm).map_err(CommandError::FailedToCreateFile)?;
        for dep in &dependencies {
            if !crate::util::is_safe_path_component(dep) {
                continue;
            }
            let (dep_name, _) = Versions::parse_raw_package_details(dep.clone());
            let dep_src = cache_root.join(dep).join("package");
            let dep_dest = cache_nm.join(&dep_name);
            if let Some(parent) = dep_dest.parent() {
                fs_sync::create_dir_all(parent).map_err(CommandError::FailedToCreateFile)?;
            }
            match crate::util::create_dir_link(&dep_src, &dep_dest) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(CommandError::FailedToCreateFile(e)),
            }
        }

        // Link the package itself and all deps into dest_root (flat hoisting).
        let mut all_links = dependencies;
        all_links.push(package);

        for entry in all_links {
            if !crate::util::is_safe_path_component(&entry) {
                continue;
            }
            let (package_name, _) = Versions::parse_raw_package_details(entry.to_string());

            let src = cache_root.join(&entry).join("package");
            let dest = dest_root.join(&package_name);

            if let Some(parent) = dest.parent() {
                fs_sync::create_dir_all(parent).map_err(CommandError::FailedToCreateFile)?;
            }
            match crate::util::create_dir_link(&src, &dest) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(CommandError::FailedToCreateFile(e)),
            }
        }

        Ok(())
    }
}
