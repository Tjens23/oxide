use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use semver::VersionReq;
use serde_json::Value;

use crate::{
    cache::{CACHE_DIRECTORY, Cache},
    config::OxideConfig,
    constants::{GLOBAL_BIN_SUBDIR, GLOBAL_MODULES_SUBDIR, NODE_MODULES, OXIDE_LOCK},
    errors::{CommandError, ParseError},
    installer::{InstallContext, Installer, PackageInfo},
    util::TaskAllocator,
    versions::Versions,
    workspace,
};

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct InstallHandler {
    pub package_name: String,
    pub semantic_version: Option<VersionReq>,
    filter: Option<String>,
    global: bool,
    save_dev: bool,
    no_save: bool,
    ignore_scripts: bool,
}

impl InstallHandler {
    pub fn new(package_name: String) -> Self {
        Self {
            package_name,
            ..Default::default()
        }
    }
}

impl InstallHandler {
    fn update_package_json(
        package_name: &str,
        version: &str,
        dev: bool,
    ) -> Result<(), CommandError> {
        let content =
            std::fs::read_to_string("./package.json").unwrap_or_else(|_| "{}".to_string());
        let mut json: Value =
            serde_json::from_str(&content).map_err(CommandError::ParsingFailed)?;

        let deps_key = if dev {
            "devDependencies"
        } else {
            "dependencies"
        };
        let deps = json
            .as_object_mut()
            .ok_or_else(|| {
                CommandError::ParsingFailed(serde_json::from_str::<Value>("null").unwrap_err())
            })?
            .entry(deps_key)
            .or_insert(Value::Object(serde_json::Map::new()));

        if let Some(map) = deps.as_object_mut() {
            map.insert(
                package_name.to_string(),
                Value::String(format!("^{}", version)),
            );
        }

        let output = serde_json::to_string_pretty(&json)
            .map_err(CommandError::FailedToSerializePackageLock)?;
        std::fs::write("./package.json", output).map_err(CommandError::FailedToWriteFile)?;
        Ok(())
    }
}

#[async_trait]
impl CommandHandler for InstallHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        let mut peekable = args.peekable();
        while let Some(arg) = peekable.next() {
            match arg.as_str() {
                "-g" | "--global" => self.global = true,
                "-D" | "--save-dev" => self.save_dev = true,
                "--no-save" => self.no_save = true,
                "--ignore-scripts" => self.ignore_scripts = true,
                "--filter" | "-F" => {
                    let pat = peekable.next().ok_or_else(|| {
                        ParseError::MissingArgument("--filter <pattern>".to_string())
                    })?;
                    self.filter = Some(pat);
                }
                package_details => {
                    let (name, version) =
                        Versions::parse_semantic_package_details(package_details.to_string())?;
                    self.package_name = name;
                    self.semantic_version = version;
                }
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        if self.package_name.is_empty() {
            return self.install_from_package_json().await;
        }

        if self.global {
            return self.execute_global().await;
        }

        if let Some(ref filter) = self.filter {
            let root = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
            let packages = workspace::discover(&root)?;
            let matched = workspace::apply_filter(&packages, filter);

            if matched.is_empty() {
                println!("No workspace packages matched filter '{}'.", filter);
                return Ok(());
            }

            for pkg in &matched {
                println!("\n[{}] installing '{}'..", pkg.name, self.package_name);
                std::env::set_current_dir(&pkg.path).map_err(CommandError::FailedToWriteFile)?;
                Box::pin(self.execute_single()).await?;
            }

            std::env::set_current_dir(&root).map_err(CommandError::FailedToWriteFile)?;
            return Ok(());
        }

        self.execute_single().await
    }
}

impl InstallHandler {
    async fn install_from_package_json(&self) -> Result<(), CommandError> {
        let content =
            std::fs::read_to_string("./package.json").map_err(CommandError::FailedToReadFile)?;
        let json: Value = serde_json::from_str(&content).map_err(CommandError::ParsingFailed)?;

        let deps = json
            .get("dependencies")
            .and_then(|d| d.as_object())
            .cloned()
            .unwrap_or_default();

        if deps.is_empty() {
            println!("No dependencies found in package.json.");
            return Ok(());
        }

        for (name, version_value) in &deps {
            let version_str = version_value.as_str().unwrap_or("");
            let package_details = if version_str.is_empty() {
                name.clone()
            } else {
                format!("{}@{}", name, version_str)
            };

            let (package_name, semantic_version) =
                Versions::parse_semantic_package_details(package_details)
                    .map_err(|e| CommandError::GitFailed(e.to_string()))?;

            let handler = InstallHandler {
                package_name,
                semantic_version,
                no_save: self.no_save,
                ignore_scripts: self.ignore_scripts,
                ..Default::default()
            };

            handler.execute_single().await?;
        }

        Ok(())
    }

    async fn execute_single(&self) -> Result<(), CommandError> {
        println!("Installing '{}'..", self.package_name);

        let started_at = std::time::Instant::now();

        let client = reqwest::Client::new();
        let semantic_version = self.semantic_version.as_ref();
        let full_version = Versions::resolve_full_version(semantic_version);
        let full_version = full_version.as_ref();

        let (is_cached, cached_version) =
            Cache::exists(&self.package_name, full_version, semantic_version).await?;

        Installer::create_modules_dir();

        if is_cached {
            let version = cached_version.expect("Could not resolve version of cached package");
            if !crate::util::is_safe_path_component(&version) {
                return Err(CommandError::GitFailed(format!(
                    "unsafe version string in cache: {}",
                    version
                )));
            }
            let stringified = Versions::stringify(&self.package_name, &version);
            let lockfile_path = PathBuf::from(CACHE_DIRECTORY.as_str())
                .join(&stringified)
                .join("package")
                .join(OXIDE_LOCK);
            let lockfile_complete = std::fs::read_to_string(&lockfile_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<crate::types::PackageLock>(&raw).ok())
                .map(|lf| !lf.dependencies.is_empty())
                .unwrap_or(false);

            if lockfile_complete {
                Cache::load_cached_version(stringified, std::path::Path::new(NODE_MODULES))?;
                if !self.no_save {
                    Self::update_package_json(&self.package_name, &version, self.save_dev)?;
                }
                println!("Done in {:.2}s", started_at.elapsed().as_secs_f64());
                return Ok(());
            }
            // Lockfile missing or empty deps — fall through to fresh install
        }

        let version_data = Installer::get_version_data(
            client.clone(),
            &self.package_name,
            full_version,
            semantic_version,
        )
        .await?;

        let dependency_map_mux = Arc::new(Mutex::new(HashMap::new()));

        let install_context = InstallContext {
            client,
            dependency_map_mux: Arc::clone(&dependency_map_mux),
        };

        let stringified = Versions::stringify(&version_data.name, &version_data.version);
        let resolved_name = version_data.name.clone();
        let resolved_version = version_data.version.clone();

        if !crate::util::is_safe_path_component(&stringified) {
            return Err(CommandError::GitFailed(format!(
                "unsafe package identifier received from registry: {}",
                stringified
            )));
        }

        let package_info = PackageInfo {
            version_data,
            is_latest: Versions::is_latest(full_version),
            stringified: stringified.to_string(),
        };

        Installer::install_package(
            install_context,
            package_info,
            Arc::new(Mutex::new(Vec::new())),
        )?;

        TaskAllocator::block_until_done();

        Installer::setup_cache_packages(Arc::clone(&dependency_map_mux))?;
        Installer::write_project_lockfile(dependency_map_mux)?;
        Cache::load_cached_version(stringified, Path::new(NODE_MODULES))?;

        if !self.no_save {
            Self::update_package_json(&resolved_name, &resolved_version, self.save_dev)?;
        }

        self.note_ignore_scripts();

        println!("Done in {:.2}s", started_at.elapsed().as_secs_f64());
        Ok(())
    }

    fn note_ignore_scripts(&self) {
        let from_config = OxideConfig::load().is_true("ignore-scripts");
        if self.ignore_scripts || from_config {
            println!("note: --ignore-scripts is set; lifecycle scripts will not run.");
        }
    }

    fn global_nm_dir() -> Result<PathBuf, CommandError> {
        let base = dirs::config_dir()
            .ok_or_else(|| CommandError::GitFailed("cannot determine config directory".into()))?
            .join("oxide");
        Ok(base.join(GLOBAL_MODULES_SUBDIR))
    }

    fn global_bin_dir() -> Result<PathBuf, CommandError> {
        let cfg = OxideConfig::load();
        let base = match cfg.get("global-bin-dir") {
            Some(dir) => return Ok(PathBuf::from(dir)),
            None => dirs::config_dir()
                .ok_or_else(|| CommandError::GitFailed("cannot determine config directory".into()))?
                .join("oxide"),
        };
        Ok(base.join(GLOBAL_BIN_SUBDIR))
    }

    async fn execute_global(&self) -> Result<(), CommandError> {
        println!("Installing '{}' globally..", self.package_name);

        let started_at = std::time::Instant::now();
        let client = reqwest::Client::new();
        let semantic_version = self.semantic_version.as_ref();
        let full_version = Versions::resolve_full_version(semantic_version);
        let full_version = full_version.as_ref();

        let (is_cached, cached_version) =
            Cache::exists(&self.package_name, full_version, semantic_version).await?;

        let global_nm = Self::global_nm_dir()?;
        let global_bin = Self::global_bin_dir()?;
        std::fs::create_dir_all(&global_nm).map_err(CommandError::FailedToCreateFile)?;
        std::fs::create_dir_all(&global_bin).map_err(CommandError::FailedToCreateFile)?;

        let (resolved_name, resolved_version) = if is_cached {
            let version = cached_version.expect("Could not resolve version of cached package");
            let stringified = Versions::stringify(&self.package_name, &version);
            Cache::load_cached_version(stringified, &global_nm)?;
            (self.package_name.clone(), version)
        } else {
            let version_data = Installer::get_version_data(
                client.clone(),
                &self.package_name,
                full_version,
                semantic_version,
            )
            .await?;

            let dependency_map_mux = Arc::new(Mutex::new(HashMap::new()));
            let install_context = InstallContext {
                client,
                dependency_map_mux: Arc::clone(&dependency_map_mux),
            };

            let stringified = Versions::stringify(&version_data.name, &version_data.version);
            let resolved_name = version_data.name.clone();
            let resolved_version = version_data.version.clone();

            if !crate::util::is_safe_path_component(&stringified) {
                return Err(CommandError::GitFailed(format!(
                    "unsafe package identifier received from registry: {}",
                    stringified
                )));
            }

            let package_info = PackageInfo {
                version_data,
                is_latest: Versions::is_latest(full_version),
                stringified: stringified.to_string(),
            };

            Installer::install_package(
                install_context,
                package_info,
                Arc::new(Mutex::new(Vec::new())),
            )?;
            TaskAllocator::block_until_done();

            Installer::setup_cache_packages(Arc::clone(&dependency_map_mux))?;
            Installer::write_project_lockfile(dependency_map_mux)?;
            Cache::load_cached_version(stringified, &global_nm)?;
            (resolved_name, resolved_version)
        };

        let short_name = resolved_name.split('/').last().unwrap_or(&resolved_name);
        let package_dir = global_nm.join(short_name);
        Self::link_global_binaries(&package_dir, &global_bin, short_name);

        self.note_ignore_scripts();
        println!(
            "Installed '{}@{}' globally in {:.2}s",
            resolved_name,
            resolved_version,
            started_at.elapsed().as_secs_f64()
        );
        println!("hint: make sure '{}' is in your PATH", global_bin.display());
        Ok(())
    }

    fn link_global_binaries(package_dir: &Path, bin_dir: &Path, short_name: &str) {
        let pkg_json = match std::fs::read_to_string(package_dir.join("package.json")) {
            Ok(raw) => raw,
            Err(_) => return,
        };
        let json: Value = match serde_json::from_str(&pkg_json) {
            Ok(v) => v,
            Err(_) => return,
        };

        match json.get("bin") {
            Some(Value::String(rel)) => {
                let src = package_dir.join(rel.trim_start_matches("./"));
                let dest = bin_dir.join(short_name);
                Self::try_link_binary(&src, &dest);
            }
            Some(Value::Object(map)) => {
                for (bin_name, rel_val) in map {
                    if let Some(rel) = rel_val.as_str() {
                        let src = package_dir.join(rel.trim_start_matches("./"));
                        let dest = bin_dir.join(bin_name);
                        Self::try_link_binary(&src, &dest);
                    }
                }
            }
            _ => {}
        }
    }

    fn try_link_binary(src: &Path, dest: &Path) {
        if dest.exists() {
            let _ = std::fs::remove_file(dest);
        }
        match crate::util::create_file_link(src, dest) {
            Ok(_) => println!("  linked binary '{}'", dest.display()),
            Err(e) => eprintln!(
                "  warning: could not link binary '{}': {} \
                 (on Windows, enable Developer Mode or run as administrator)",
                dest.display(),
                e
            ),
        }
    }
}
