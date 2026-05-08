use std::{
    collections::HashMap,
    fs::{self as fs_sync, File},
    io::{Read, Seek, SeekFrom},
    str::FromStr,
};

use lazy_static::lazy_static;
use semver::Version;
use tokio::fs;

use crate::{
    errors::CommandError,
    types::PackageLock,
    versions::{Versions, EMPTY_VERSION, LATEST},
};
use semver::VersionReq;

pub struct CachedVersion {
    pub version: String,
    pub is_latest: bool,
}

pub type CachedVersions = HashMap<String, CachedVersion>;

lazy_static! {
    pub static ref CACHE_DIRECTORY: String = format!(
        "{}/node-cache",
        dirs::cache_dir()
            .expect("Failed to find cache directory")
            .to_str()
            .expect("Failed to convert cache directory to string")
    );
    pub static ref CACHED_VERSIONS: CachedVersions = Cache::get_cached_versions();
}

pub struct Cache;
impl Cache {
    pub fn get_cached_versions() -> CachedVersions {
        fs_sync::create_dir_all(CACHE_DIRECTORY.to_string())
            .expect("Failed to create cache directory");

        let dir_contents =
            fs_sync::read_dir(CACHE_DIRECTORY.to_string()).expect("Failed to read cache directory");

        let mut all_entries: Vec<String> = Vec::new();
        let mut cached_versions = HashMap::new();

        for entry in dir_contents.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with('@') {
                let scope_dir = format!("{}/{}", *CACHE_DIRECTORY, filename);
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
            let lock_path = format!(
                "{}/{}/package/oxide-lock.json",
                *CACHE_DIRECTORY, full_entry
            );

            let mut lock_file = match File::open(&lock_path) {
                Ok(f) => f,
                Err(_) => continue, // Skip entries without a lockfile (e.g. mid-extraction)
            };

            // This is not an ideal method but it beats parsing the JSON of every installed package
            let start_byte = 12;
            let end_byte = 15;

            let bytes_length = end_byte - start_byte + 1;
            let mut buf = vec![0; bytes_length];

            lock_file.seek(SeekFrom::Start(start_byte as u64)).unwrap();
            lock_file.read_exact(&mut buf).unwrap();

            let is_latest_str = String::from_utf8(buf).unwrap();
            let is_latest = is_latest_str == "true";

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
                let scope_dir = format!("{}/{}", *CACHE_DIRECTORY, scope);

                if let Ok(mut scope_entries) = fs::read_dir(&scope_dir).await {
                    while let Some(scope_entry) = scope_entries
                        .next_entry()
                        .await
                        .map_err(CommandError::FailedDirectoryEntry)
                        .unwrap()
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

        let mut cache_entries = fs::read_dir(CACHE_DIRECTORY.to_string())
            .await
            .map_err(CommandError::NoCacheDirectory)?;

        while let Some(cache_entry) = cache_entries
            .next_entry()
            .await
            .map_err(CommandError::FailedDirectoryEntry)
            .unwrap()
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

    pub fn load_cached_version(package: String) {
        let lockfile_path = format!(
            "{}/{}/package/oxide-lock.json",
            *CACHE_DIRECTORY, package
        );

        // If no lockfile exists (e.g. from a previously failed install), just link the package itself.
        let dependencies: Vec<String> = match fs_sync::read_to_string(&lockfile_path) {
            Ok(raw) => {
                let lockfile = serde_json::from_str::<PackageLock>(raw.as_str()).unwrap();
                lockfile.dependencies
            }
            Err(_) => vec![],
        };

        // Create cache-level node_modules so Node.js can resolve deps from
        // the real (cache) path rather than the project's node_modules.
        let cache_nm = format!("{}/{}/node_modules", *CACHE_DIRECTORY, package);
        fs_sync::create_dir_all(&cache_nm).expect("Failed to create cache node_modules");
        for dep in &dependencies {
            let (dep_name, _) = Versions::parse_raw_package_details(dep.clone());
            let dep_src = format!("{}/{}/package", *CACHE_DIRECTORY, dep);
            let dep_dest = format!("{}/{}", cache_nm, dep_name);
            if let Some(parent) = std::path::Path::new(&dep_dest).parent() {
                fs_sync::create_dir_all(parent).expect("Failed to create scope dir");
            }
            match crate::util::create_dir_link(&dep_src, &dep_dest) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("{}", e),
            }
        }

        // Link the package itself and all deps into the project's node_modules (flat hoisting).
        let mut all_links = dependencies.clone();
        all_links.push(package.clone());

        for entry in all_links {
            let (package_name, _) = Versions::parse_raw_package_details(entry.to_string());

            let src = format!("{}/{}/package", *CACHE_DIRECTORY, entry);
            let dest = format!("./node_modules/{}", package_name);

            if let Some(parent) = std::path::Path::new(&dest).parent() {
                fs_sync::create_dir_all(parent).expect("Failed to create scope dir");
            }
            match crate::util::create_dir_link(&src, &dest) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("{}", e),
            }
        }
    }
}