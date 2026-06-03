use semver::VersionReq;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::util::{self, TaskAllocator};
use crate::{
    cache::{CACHE_DIRECTORY, FILE_STORE_DIR, Cache},
    constants::{NODE_MODULES, OXIDE_LOCK},
    errors::CommandError::{self},
    http::HTTPRequest,
    types::{DependencyMap, PackageLock, VersionData},
    versions::{LATEST, Versions},
};

pub type DependencyMapMutex = Arc<Mutex<DependencyMap>>;

#[derive(Clone)]
pub enum InstallProgress {
    Logging,
    Bar(Arc<ProgressBar>),
    Both(Arc<MultiProgress>),
}

impl InstallProgress {
    pub fn finish(&self) {
        match self {
            Self::Bar(pb) => pb.finish_and_clear(),
            Self::Both(m) => {
                let _ = m.clear();
            }
            Self::Logging => {}
        }
    }
}

pub struct PackageInfo {
    pub version_data: VersionData,
    pub is_latest: bool,
    pub stringified: String,
}

#[derive(Clone)]
pub struct InstallContext {
    pub client: reqwest::Client,
    pub dependency_map_mux: DependencyMapMutex,
    pub progress: InstallProgress,
}

pub struct Installer;
impl Installer {
    pub async fn get_version_data(
        client: reqwest::Client,
        package_name: &String,
        full_version: Option<&String>,
        semantic_version: Option<&VersionReq>,
    ) -> Result<VersionData, CommandError> {
        if let Some(version) = full_version {
            return HTTPRequest::version_data(client.clone(), package_name, version).await;
        }

        let mut package_data = HTTPRequest::package_data(client.clone(), package_name).await?;
        let package_version =
            Versions::resolve_partial_version(semantic_version, &package_data.versions)?;

        package_data
            .versions
            .remove(&package_version)
            .ok_or(CommandError::InvalidVersion)
    }

    fn already_resolved(
        context: &InstallContext,
        package_info: &PackageInfo,
    ) -> Result<bool, CommandError> {
        let mut dependency_map = context
            .dependency_map_mux
            .lock()
            .map_err(|_| CommandError::MutexPoisoned)?;
        let stringified_version = Versions::stringify(
            &package_info.version_data.name,
            &package_info.version_data.version,
        );

        let installed_version = dependency_map.get(&stringified_version);

        Ok(match installed_version {
            Some(_) => true,
            None => {
                dependency_map.insert(
                    stringified_version,
                    PackageLock::new(package_info.is_latest),
                );
                false
            }
        })
    }

    fn append_version(
        parents_mux: Arc<Mutex<Vec<String>>>,
        new_version_name: String,
        dependency_map_mux: DependencyMapMutex,
    ) -> Result<(), CommandError> {
        let mut dependency_map = dependency_map_mux
            .lock()
            .map_err(|_| CommandError::MutexPoisoned)?;
        let parents = parents_mux
            .lock()
            .map_err(|_| CommandError::MutexPoisoned)?;

        for parent_version_name in parents.iter() {
            let parent_version = dependency_map
                .entry(parent_version_name.to_string())
                .or_insert(PackageLock::new(parent_version_name.ends_with(LATEST)));

            parent_version
                .dependencies
                .push(new_version_name.to_string());
        }

        Ok(())
    }

    pub fn install_package(
        context: InstallContext,
        package_info: PackageInfo,
        parents_mux: Arc<Mutex<Vec<String>>>,
    ) -> Result<(), CommandError> {
        if Self::already_resolved(&context, &package_info)? {
            Self::append_version(
                Arc::clone(&parents_mux),
                package_info.stringified.clone(),
                Arc::clone(&context.dependency_map_mux),
            )?;
            return Ok(());
        }

        Self::append_version(
            Arc::clone(&parents_mux),
            package_info.stringified.clone(),
            Arc::clone(&context.dependency_map_mux),
        )?;

        {
            let mut parents = parents_mux
                .lock()
                .map_err(|_| CommandError::MutexPoisoned)?;
            parents.push(package_info.stringified.clone());
        }

        TaskAllocator::add_task(async move {
            let version_data = package_info.version_data;
            let stringified = package_info.stringified;
            let package_destination = PathBuf::from(CACHE_DIRECTORY.as_str()).join(&stringified);

            if let Some(ref integrity) = version_data.dist.integrity {
                if let Ok(mut map) = context.dependency_map_mux.lock() {
                    if let Some(entry) = map.get_mut(&stringified) {
                        entry.integrity = Some(integrity.clone());
                    }
                }
            }

            let already_extracted = package_destination.join("package").exists();

            if !already_extracted {
                let package_bytes = {
                    let maybe_cached = version_data
                        .dist
                        .integrity
                        .as_deref()
                        .and_then(Cache::get_tarball);

                    if let Some(cached) = maybe_cached {
                        cached
                    } else {
                        let bytes = match HTTPRequest::get_bytes(
                            context.client.clone(),
                            version_data.dist.tarball.clone(),
                        )
                        .await
                        {
                            Ok(b) => b,
                            Err(e) => {
                                eprintln!("Failed to download '{}': {}", stringified, e);
                                return;
                            }
                        };
                        if !version_data.dist.verify(&bytes) {
                            eprintln!("'{}': {}", stringified, CommandError::IntegrityCheckFailed);
                            return;
                        }
                        if let Some(ref integrity) = version_data.dist.integrity {
                            let _ = Cache::store_tarball(integrity, &bytes);
                        }
                        bytes
                    }
                };

                let strf = stringified.clone();
                let progress = context.progress.clone();
                let store_dir = PathBuf::from(FILE_STORE_DIR.as_str());
                TaskAllocator::add_blocking(move || {
                    match util::extract_tarball_hardlinked(
                        package_bytes,
                        &package_destination,
                        &store_dir,
                    ) {
                        Ok(_) => match progress {
                            InstallProgress::Logging => println!("Installed '{}'", strf),
                            InstallProgress::Bar(pb) => {
                                pb.inc(1);
                                pb.set_message(format!("Installed '{}'", strf));
                            }
                            InstallProgress::Both(m) => {
                                let pkg_pb = m.add(ProgressBar::new_spinner());
                                pkg_pb.set_style(
                                    ProgressStyle::with_template(
                                        "{prefix:.bold.dim} {spinner} {wide_msg}",
                                    )
                                    .unwrap_or_else(|_| ProgressStyle::default_spinner())
                                    .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
                                );
                                pkg_pb.enable_steady_tick(Duration::from_millis(80));
                                pkg_pb.finish_with_message(format!("Installed '{}'", strf));
                            }
                        },
                        Err(e) => eprintln!("Failed to extract '{}': {e}", strf),
                    }
                });
            }

            let dependencies = version_data.dependencies.unwrap_or_default();
            if let Err(e) = Self::install_dependencies(parents_mux, context, dependencies).await {
                eprintln!("Warning: failed to install dependencies: {e}");
            }
        });

        Ok(())
    }

    async fn install_dependencies(
        parents_mux: Arc<Mutex<Vec<String>>>,
        context: InstallContext,
        dependencies: HashMap<String, String>,
    ) -> Result<(), CommandError> {
        for (name, version) in dependencies {
            let req = match Versions::parse_semantic_version(&version) {
                Ok(req) => req,
                Err(_) => {
                    eprintln!(
                        "Warning: skipping '{}' — unparseable version '{}'",
                        name, version
                    );
                    continue;
                }
            };
            let req = Some(&req);

            let full_version = Versions::resolve_full_version(req);
            let full_version = full_version.as_ref();

            let (is_cached, cached_version) = match Cache::exists(&name, full_version, req).await {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Warning: failed to check cache for '{}': {}", name, e);
                    continue;
                }
            };

            if is_cached {
                Self::install_cached_dep(&parents_mux, &context, cached_version, &name)?;
                continue;
            }

            let version_data = match Self::get_version_data(
                context.client.clone(),
                &name,
                full_version,
                req,
            )
            .await
            {
                Ok(data) => data,
                Err(e) => {
                    eprintln!(
                        "Warning: failed to fetch version data for '{}': {}",
                        name, e
                    );
                    continue;
                }
            };

            let stringified = Versions::stringify(&name, &version_data.version);
            let package_info = PackageInfo {
                version_data,
                is_latest: Versions::is_latest(Some(&stringified)),
                stringified,
            };

            Self::install_package(context.clone(), package_info, Arc::clone(&parents_mux))?;
        }

        Ok(())
    }

    fn install_cached_dep(
        parents_mux: &Arc<Mutex<Vec<String>>>,
        context: &InstallContext,
        cached_version: Option<String>,
        name: &str,
    ) -> Result<(), CommandError> {
        let version = cached_version.ok_or(CommandError::InvalidVersion)?;
        let name_string = name.to_string();
        let stringified = Versions::stringify(&name_string, &version);

        Self::append_version(
            Arc::clone(parents_mux),
            stringified.clone(),
            Arc::clone(&context.dependency_map_mux),
        )?;

        let in_dep_map = context
            .dependency_map_mux
            .lock()
            .map_err(|_| CommandError::MutexPoisoned)?
            .contains_key(stringified.as_str());

        if !in_dep_map
            && let Err(e) = Cache::load_cached_version(
                stringified,
                std::path::Path::new(crate::constants::NODE_MODULES),
            )
        {
            eprintln!("Warning: failed to load cached package: {e}");
        }

        Ok(())
    }

    pub fn create_modules_dir() -> Result<(), CommandError> {
        if !Path::new(NODE_MODULES).exists() {
            fs::create_dir_all(NODE_MODULES).map_err(CommandError::FailedToCreateFile)?;
        }
        Ok(())
    }

    pub fn setup_cache_packages(
        dependency_map_mux: DependencyMapMutex,
    ) -> Result<(), CommandError> {
        let dependency_map = dependency_map_mux
            .lock()
            .map_err(|_| CommandError::MutexPoisoned)?;

        Self::write_package_lockfiles(&dependency_map)?;

        let all_packages = Self::collect_all_packages(&dependency_map);

        drop(dependency_map);

        Self::link_package_deps(all_packages)
    }

    fn write_package_lockfiles(dependency_map: &DependencyMap) -> Result<(), CommandError> {
        for (package_name, package_lock) in dependency_map.iter() {
            let package_dir = PathBuf::from(CACHE_DIRECTORY.as_str())
                .join(package_name)
                .join("package");
            fs::create_dir_all(&package_dir).map_err(CommandError::FailedToCreateFile)?;

            let mut package_lock_file = File::create(package_dir.join(OXIDE_LOCK))
                .map_err(CommandError::FailedToCreateFile)?;

            let package_lock_string = serde_json::to_string(package_lock)
                .map_err(CommandError::FailedToSerializePackageLock)?;

            package_lock_file
                .write_all(package_lock_string.as_bytes())
                .map_err(CommandError::FailedToWriteFile)?;
        }
        Ok(())
    }

    fn collect_all_packages(dependency_map: &DependencyMap) -> Vec<(String, Vec<String>)> {
        let mut all_packages: Vec<(String, Vec<String>)> = dependency_map
            .iter()
            .map(|(k, v)| (k.clone(), v.dependencies.clone()))
            .collect();

        let mut visited: std::collections::HashSet<String> =
            dependency_map.keys().cloned().collect();

        let mut queue: Vec<String> = dependency_map
            .values()
            .flat_map(|v| v.dependencies.iter().cloned())
            .filter(|d| !visited.contains(d.as_str()))
            .collect();

        while let Some(pkg) = queue.pop() {
            if visited.contains(&pkg) {
                continue;
            }
            visited.insert(pkg.clone());

            let lockfile_path = PathBuf::from(CACHE_DIRECTORY.as_str())
                .join(&pkg)
                .join("package")
                .join(OXIDE_LOCK);
            let deps: Vec<String> = match fs::read_to_string(&lockfile_path) {
                Ok(raw) => serde_json::from_str::<crate::types::PackageLock>(&raw)
                    .map(|lf| lf.dependencies)
                    .unwrap_or_default(),
                Err(_) => vec![],
            };

            for dep in &deps {
                if !visited.contains(dep.as_str()) {
                    queue.push(dep.clone());
                }
            }
            all_packages.push((pkg, deps));
        }

        all_packages
    }

    fn link_package_deps(all_packages: Vec<(String, Vec<String>)>) -> Result<(), CommandError> {
        let cache_root = PathBuf::from(CACHE_DIRECTORY.as_str());
        for (package_name, deps) in all_packages {
            let cache_nm = cache_root.join(&package_name).join(NODE_MODULES);
            fs::create_dir_all(&cache_nm).map_err(CommandError::FailedToCreateFile)?;

            for dep in &deps {
                let (dep_name, _) = Versions::parse_raw_package_details(dep.clone());
                let dep_src = cache_root.join(dep).join("package");
                let dep_dest = cache_nm.join(&dep_name);
                if let Some(parent) = dep_dest.parent() {
                    fs::create_dir_all(parent).map_err(CommandError::FailedToCreateFile)?;
                }
                match util::create_dir_link(&dep_src, &dep_dest) {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(CommandError::FailedToCreateFile(e)),
                }
            }
        }
        Ok(())
    }

    /// Returns `true` if every package in `lockfile` has an extracted directory
    /// in the global cache — i.e. no downloads are needed.
    pub fn is_fully_cached(lockfile: &DependencyMap) -> bool {
        let cache_root = PathBuf::from(CACHE_DIRECTORY.as_str());
        lockfile.keys().all(|pkg| {
            crate::util::is_safe_path_component(pkg)
                && cache_root.join(pkg).join("package").exists()
        })
    }

    /// Returns `true` if every package from `lockfile` has a corresponding
    /// entry (symlink or directory) already present in `node_modules`.
    pub fn node_modules_matches_lockfile(lockfile: &DependencyMap) -> bool {
        let nm = Path::new(NODE_MODULES);
        if !nm.exists() {
            return false;
        }
        lockfile.keys().all(|pkg| {
            let (name, _) = Versions::parse_raw_package_details(pkg.clone());
            nm.join(&name).exists()
        })
    }

    /// Installs all entries in `deps` (a `{name: version_range}` map as found
    /// in `package.json`) using a single shared [`InstallContext`], so all
    /// downloads happen in parallel under one `TaskAllocator` budget.
    ///
    /// Callers must invoke [`TaskAllocator::block_until_done`] after this
    /// returns to wait for all spawned tasks to finish.
    pub async fn install_all(
        context: InstallContext,
        deps: HashMap<String, String>,
    ) -> Result<(), CommandError> {
        Self::install_dependencies(Arc::new(Mutex::new(Vec::new())), context, deps).await
    }

    pub fn write_project_lockfile(
        dependency_map_mux: DependencyMapMutex,
    ) -> Result<(), CommandError> {
        let new_entries = dependency_map_mux
            .lock()
            .map_err(|_| CommandError::MutexPoisoned)?;

        let mut merged: DependencyMap = fs::read_to_string(OXIDE_LOCK)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();

        for (k, v) in new_entries.iter() {
            merged.entry(k.clone()).or_insert_with(|| PackageLock {
                is_latest: v.is_latest,
                dependencies: v.dependencies.clone(),
                integrity: v.integrity.clone(),
            });
        }

        let serialized = serde_json::to_string_pretty(&merged)
            .map_err(CommandError::FailedToSerializePackageLock)?;

        fs::write(OXIDE_LOCK, serialized).map_err(CommandError::FailedToWriteFile)?;

        Ok(())
    }
}
