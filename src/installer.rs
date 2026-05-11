use semver::VersionReq;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::util::{self, TaskAllocator};
use crate::{
    cache::{Cache, CACHE_DIRECTORY},
    errors::CommandError::{self},
    http::HTTPRequest,
    types::{DependencyMap, PackageLock, VersionData},
    versions::{Versions, LATEST},
};

pub type DependencyMapMutex = Arc<Mutex<DependencyMap>>;

pub struct PackageInfo {
    pub version_data: VersionData,
    pub is_latest: bool,
    pub stringified: String,
}

#[derive(Clone)]
pub struct InstallContext {
    pub client: reqwest::Client,
    pub dependency_map_mux: DependencyMapMutex,
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

        Ok(package_data
            .versions
            .remove(&package_version)
            .expect("Failed to find resolved package version in package data"))
    }

    fn already_resolved(context: &InstallContext, package_info: &PackageInfo) -> bool {
        let mut dependency_map = context.dependency_map_mux.lock().unwrap();
        let stringified_version = Versions::stringify(
            &package_info.version_data.name,
            &package_info.version_data.version,
        );

        let installed_version = dependency_map.get(&stringified_version);

        match installed_version {
            Some(_) => true,
            None => {
                dependency_map.insert(
                    stringified_version,
                    PackageLock::new(package_info.is_latest),
                );
                false
            }
        }
    }

    fn append_version(
        parents_mux: Arc<Mutex<Vec<String>>>,
        new_version_name: String,
        dependency_map_mux: DependencyMapMutex,
    ) -> Result<(), CommandError> {
        let mut dependency_map = dependency_map_mux.lock().unwrap();
        let parents = parents_mux.lock().unwrap();

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
        if Self::already_resolved(&context, &package_info) {
            Self::append_version(
                Arc::clone(&parents_mux),
                package_info.stringified.to_string(),
                Arc::clone(&context.dependency_map_mux),
            )
            .unwrap();
            return Ok(());
        }

        Self::append_version(
            Arc::clone(&parents_mux),
            package_info.stringified.to_string(),
            Arc::clone(&context.dependency_map_mux),
        )
        .unwrap();

        {
            let mut parents = parents_mux.lock().unwrap();
            parents.push(package_info.stringified.to_string());
        }

        TaskAllocator::add_task(async move {
            let version_data = package_info.version_data;
            let stringified = package_info.stringified;
            let package_destination = format!("{}/{}", *CACHE_DIRECTORY, stringified);
            let already_extracted =
                std::path::Path::new(&format!("{}/package", package_destination)).exists();

            if !already_extracted {
                let package_bytes =
                    match HTTPRequest::get_bytes(context.client.clone(), version_data.dist.tarball.clone())
                        .await
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            eprintln!("Failed to download '{}': {}", stringified, e);
                            return;
                        }
                    };
                if !version_data.dist.verify(&package_bytes) {
                    eprintln!("'{}': {}", stringified, CommandError::IntegrityCheckFailed);
                    return;
                }
                let dest = package_destination.clone();
                let strf = stringified.clone();
                TaskAllocator::add_blocking(move || {
                    util::extract_tarball(package_bytes, dest).unwrap();
                    println!("Installed '{}'", strf);
                });
            }

            let dependencies = version_data.dependencies.unwrap_or(HashMap::new());
            Self::install_dependencies(parents_mux, context, dependencies).await;
        });

        Ok(())
    }

    async fn install_dependencies(
        parents_mux: Arc<Mutex<Vec<String>>>,
        context: InstallContext,
        dependencies: HashMap<String, String>,
    ) {
        for (name, version) in dependencies {
            let req = match Versions::parse_semantic_version(&version) {
                Ok(req) => req,
                Err(_) => {
                    eprintln!("Warning: skipping '{}' — unparseable version '{}'", name, version);
                    continue;
                }
            };
            let req = Some(&req);

            let full_version = Versions::resolve_full_version(req);
            let full_version = full_version.as_ref();

            let (is_cached, cached_version) = match Cache::exists(&name, full_version, req)
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Warning: failed to check cache for '{}': {}", name, e);
                    continue;
                }
            };

            if is_cached {
                let version = cached_version.expect("Could not resolve version of cached package");
                let stringified = Versions::stringify(&name, &version);

                Self::append_version(
                    Arc::clone(&parents_mux),
                    stringified.clone(),
                    Arc::clone(&context.dependency_map_mux),
                )
                .unwrap();

                let in_dep_map = context
                    .dependency_map_mux
                    .lock()
                    .unwrap()
                    .contains_key(stringified.as_str());

                if !in_dep_map {
                    Cache::load_cached_version(stringified);
                }
                continue;
            }

            let version_data =
                match Self::get_version_data(context.client.clone(), &name, full_version, req)
                    .await
                {
                    Ok(data) => data,
                    Err(e) => {
                        eprintln!("Warning: failed to fetch version data for '{}': {}", name, e);
                        continue;
                    }
                };

            let stringified = Versions::stringify(&name, &version_data.version);

            let package_info = PackageInfo {
                version_data,
                is_latest: Versions::is_latest(Some(&stringified)),
                stringified,
            };

            Self::install_package(context.clone(), package_info, Arc::clone(&parents_mux)).unwrap();
        }
    }

    pub fn create_modules_dir() {
        if Path::new("./node_modules").exists() {
            return;
        }

        fs::create_dir("./node_modules").expect("Failed to create node modules folder");
    }

    pub fn setup_cache_packages(dependency_map_mux: DependencyMapMutex) -> Result<(), CommandError> {
        let dependency_map = dependency_map_mux.lock().unwrap();

        for (package_name, package_lock) in dependency_map.iter() {
            let package_dir = format!("{}/{}/package", *CACHE_DIRECTORY, package_name);
            fs::create_dir_all(&package_dir).map_err(CommandError::FailedToCreateFile)?;

            let mut package_lock_file = File::create(format!("{}/oxide-lock.json", package_dir))
                .map_err(CommandError::FailedToCreateFile)?;

            let package_lock_string = serde_json::to_string(package_lock)
                .map_err(CommandError::FailedToSerializePackageLock)?;

            package_lock_file
                .write_all(package_lock_string.as_bytes())
                .map_err(CommandError::FailedToWriteFile)?;
        }


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

            let lockfile_path =
                format!("{}/{}/package/oxide-lock.json", *CACHE_DIRECTORY, pkg);
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

        for (package_name, deps) in all_packages {
            let cache_nm = format!("{}/{}/node_modules", *CACHE_DIRECTORY, package_name);
            fs::create_dir_all(&cache_nm).map_err(CommandError::FailedToCreateFile)?;

            for dep in &deps {
                let (dep_name, _) = Versions::parse_raw_package_details(dep.clone());
                let dep_src = format!("{}/{}/package", *CACHE_DIRECTORY, dep);
                let dep_dest = format!("{}/{}", cache_nm, dep_name);
                if let Some(parent) = std::path::Path::new(&dep_dest).parent() {
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

    pub fn write_project_lockfile(dependency_map_mux: DependencyMapMutex) -> Result<(), CommandError> {
        let new_entries = dependency_map_mux.lock().unwrap();

        let mut merged: DependencyMap = fs::read_to_string("./oxide-lock.json")
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();

        for (k, v) in new_entries.iter() {
            merged.insert(k.clone(), PackageLock {
                is_latest: v.is_latest,
                dependencies: v.dependencies.clone(),
            });
        }

        let serialized = serde_json::to_string_pretty(&merged)
            .map_err(CommandError::FailedToSerializePackageLock)?;

        fs::write("./oxide-lock.json", serialized).map_err(CommandError::FailedToWriteFile)?;

        Ok(())
    }
}