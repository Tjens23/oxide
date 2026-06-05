use std::{
    collections::{HashMap, HashSet},
    fs::{self as fs_sync},
    path::PathBuf,
    str::FromStr,
};

use bytes::Bytes;
use lazy_static::lazy_static;
use semver::Version;
use tokio::fs;

use crate::{
    constants::{CACHE_SUBDIR, NODE_MODULES, OXIDE_LOCK, STORE_SUBDIR, TARBALL_SUBDIR},
    errors::CommandError,
    types::{DependencyMap, PackageLock},
    versions::{EMPTY_VERSION, LATEST, Versions},
};
use semver::VersionReq;

pub struct CachedVersion {
    pub version: String,
    pub is_latest: bool,
}

pub type CachedVersions = HashMap<String, CachedVersion>;

fn init_subdir(subdir: &str) -> String {
    match dirs::cache_dir().and_then(|p| p.to_str().map(|s| format!("{}/{}", s, subdir))) {
        Some(dir) => dir,
        None => {
            eprintln!("Fatal: could not determine system cache directory");
            std::process::exit(1);
        }
    }
}

lazy_static! {
    pub static ref CACHE_DIRECTORY: String = init_subdir(CACHE_SUBDIR);
    pub static ref TARBALL_CACHE_DIR: String = init_subdir(TARBALL_SUBDIR);
    pub static ref FILE_STORE_DIR: String = init_subdir(STORE_SUBDIR);
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

        let semantic_version = semantic_version.ok_or(CommandError::InvalidVersion)?;

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
        matches!(cached_version, Some(ver) if &ver.version == version)
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
            return Err(CommandError::MalformedPackageId(package));
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


    /// Converts an npm integrity string (e.g. `sha512-abc+/=`) into a
    /// filesystem-safe filename by stripping the prefix and replacing `/` → `_`
    /// and `+` → `-` so the result is safe on all platforms.
    fn integrity_to_filename(integrity: &str) -> String {
        let hash = integrity.splitn(2, '-').nth(1).unwrap_or(integrity);
        hash.replace('/', "_").replace('+', "-")
    }

    fn tarball_path(integrity: &str) -> PathBuf {
        PathBuf::from(TARBALL_CACHE_DIR.as_str())
            .join(Self::integrity_to_filename(integrity))
    }

    /// Returns cached tarball bytes if the tarball for this integrity hash is
    /// already on disk, without re-verifying (was verified on write).
    pub fn get_tarball(integrity: &str) -> Option<Bytes> {
        fs_sync::read(Self::tarball_path(integrity))
            .ok()
            .map(Bytes::from)
    }

    /// Persists raw tarball bytes to the integrity-addressed tarball cache.
    /// Silently ignores I/O errors — the cache is a best-effort optimisation.
    pub fn store_tarball(integrity: &str, bytes: &Bytes) -> Result<(), CommandError> {
        fs_sync::create_dir_all(TARBALL_CACHE_DIR.as_str())
            .map_err(CommandError::FailedToCreateFile)?;
        fs_sync::write(Self::tarball_path(integrity), bytes.as_ref())
            .map_err(CommandError::FailedToWriteFile)
    }

    /// Returns the set of package names currently linked in `nm_path`
    /// (skips internal entries like `.bin` and `.oxide-state`).
    pub fn read_current_node_modules(nm_path: &std::path::Path) -> HashSet<String> {
        let mut set = HashSet::new();
        if let Ok(entries) = fs_sync::read_dir(nm_path) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name != ".bin" && name != ".oxide-state" {
                    set.insert(name);
                }
            }
        }
        set
    }

    /// Returns the set of package names that should be in `node_modules`
    /// according to the resolved lockfile.
    pub fn desired_node_modules(lockfile: &DependencyMap) -> HashSet<String> {
        lockfile
            .keys()
            .map(|pkg| Versions::parse_raw_package_details(pkg.clone()).0)
            .collect()
    }

    /// Removes stale symlinks/dirs from `nm_path` that are no longer in
    /// `desired`, then returns.  New packages are linked by `link_all_to_node_modules`.
    pub fn apply_node_modules_diff(
        current: &HashSet<String>,
        desired: &HashSet<String>,
        nm_path: &std::path::Path,
    ) -> Result<(), CommandError> {
        for name in current.difference(desired) {
            let path = nm_path.join(name);
            if path.is_symlink() || path.is_file() {
                let _ = fs_sync::remove_file(&path);
            } else if path.is_dir() {
                let _ = fs_sync::remove_dir_all(&path);
            }
        }
        Ok(())
    }

    /// Links every package in `dependency_map` from its cache location into
    /// `nm_path` (flat hoisting).  Existing correct links are silently skipped.
    pub fn link_all_to_node_modules(
        dependency_map: &DependencyMap,
        nm_path: &std::path::Path,
    ) -> Result<(), CommandError> {
        use rayon::prelude::*;

        let cache_root = PathBuf::from(CACHE_DIRECTORY.as_str());

        let mut scope_dirs: HashSet<PathBuf> = HashSet::new();
        for pkg_at_ver in dependency_map.keys() {
            if !crate::util::is_safe_path_component(pkg_at_ver) {
                continue;
            }
            let (pkg_name, _) = Versions::parse_raw_package_details(pkg_at_ver.clone());
            if let Some(slash) = pkg_name.rfind('/') {
                scope_dirs.insert(nm_path.join(&pkg_name[..slash]));
            }
        }
        for scope_dir in &scope_dirs {
            fs_sync::create_dir_all(scope_dir).map_err(CommandError::FailedToCreateFile)?;
        }

        dependency_map.par_iter().try_for_each(|(pkg_at_ver, _)| {
            if !crate::util::is_safe_path_component(pkg_at_ver) {
                return Ok(());
            }
            let (pkg_name, _) = Versions::parse_raw_package_details(pkg_at_ver.clone());
            let src = cache_root.join(pkg_at_ver).join("package");
            let dest = nm_path.join(&pkg_name);
            match crate::util::create_dir_link(&src, &dest) {
                Ok(_) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
                Err(e) => Err(CommandError::FailedToCreateFile(e)),
            }
        })
    }
}
