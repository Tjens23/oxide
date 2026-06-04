use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use console::{Emoji, style};
use indicatif::{HumanDuration, MultiProgress, ProgressBar, ProgressStyle};
use semver::VersionReq;
use serde_json::Value;

use crate::{
    cache::{CACHE_DIRECTORY, Cache},
    config::OxideConfig,
    constants::{GLOBAL_BIN_SUBDIR, GLOBAL_MODULES_SUBDIR, NODE_MODULES, OXIDE_LOCK, OXIDE_STATE_FILE},
    errors::{CommandError, ParseError},
    installer::{InstallContext, InstallProgress, Installer, PackageInfo},
    types::DependencyMap,
    util::TaskAllocator,
    versions::Versions,
    workspace,
};

use super::command_handler::CommandHandler;

static LOOKING_GLASS: Emoji<'_, '_> = Emoji("🔍  ", "");
static TRUCK: Emoji<'_, '_> = Emoji("🚚  ", "");
static CLIP: Emoji<'_, '_> = Emoji("🔗  ", "");
static PAPER: Emoji<'_, '_> = Emoji("📃  ", "");
static SPARKLE: Emoji<'_, '_> = Emoji("✨ ", ":-)");

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
        // Build a cheap mtime+size fingerprint for package.json — no file read needed.
        let pkgjson_meta = std::fs::metadata("./package.json")
            .map_err(CommandError::FailedToReadFile)?;
        let pkgjson_mtime = pkgjson_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let pkgjson_fingerprint = format!("{}-{}", pkgjson_meta.len(), pkgjson_mtime);

        let nm_path = std::path::Path::new(NODE_MODULES);
        // State file lives in the project root so `rm -rf node_modules` doesn't wipe it.
        let state_path = std::path::Path::new(OXIDE_STATE_FILE);

        // Fast-path: fingerprint + lockfile mtime match AND node_modules exists → nothing to do.
        'fast_path: {
            let Ok(state_raw) = std::fs::read_to_string(state_path) else { break 'fast_path };
            let Ok(state) = serde_json::from_str::<serde_json::Value>(&state_raw) else { break 'fast_path };

            let stored_fp = state.get("pkgjson_fingerprint").and_then(|v| v.as_str());
            if stored_fp != Some(pkgjson_fingerprint.as_str()) {
                break 'fast_path;
            }

            let stored_lock_mtime = state.get("lockfile_mtime").and_then(|v| v.as_u64());
            let current_lock_mtime = std::fs::metadata(OXIDE_LOCK)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as u64);

            if stored_lock_mtime.is_some() && stored_lock_mtime == current_lock_mtime {
                if nm_path.exists() {
                    println!("Already up to date.");
                    return Ok(());
                }
                // Lockfile is valid but node_modules was deleted — re-link without re-resolving.
                if let Ok(lock_raw) = std::fs::read_to_string(OXIDE_LOCK) {
                    if let Ok(lockfile) = serde_json::from_str::<DependencyMap>(&lock_raw) {
                        if !lockfile.is_empty() {
                            let started_at = std::time::Instant::now();
                            Installer::create_modules_dir()?;
                            Cache::link_all_to_node_modules(&lockfile, nm_path)?;
                            println!("Done in {:.2}s", started_at.elapsed().as_secs_f64());
                            return Ok(());
                        }
                    }
                }
            }
        }

        let content = std::fs::read_to_string("./package.json")
            .map_err(CommandError::FailedToReadFile)?;
        let json: Value = serde_json::from_str(&content).map_err(CommandError::ParsingFailed)?;

        let dep_obj = json
            .get("dependencies")
            .and_then(|d| d.as_object())
            .cloned()
            .unwrap_or_default();

        if dep_obj.is_empty() {
            println!("No dependencies found in package.json.");
            return Ok(());
        }

        let all_deps: HashMap<String, String> = dep_obj
            .into_iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
            .collect();

        let started_at = std::time::Instant::now();
        let cfg = OxideConfig::load();
        let progress_mode = cfg.get("install-progress").unwrap_or("logging");

        let progress = match progress_mode {
            "bar" => {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::with_template("{spinner:.cyan} {msg} [{pos} packages]")
                        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                );
                pb.set_message("Installing packages...");
                pb.enable_steady_tick(std::time::Duration::from_millis(80));
                InstallProgress::Bar(Arc::new(pb))
            }
            "both" => {
                println!(
                    "{} {}Resolving packages...",
                    style("[1/4]").bold().dim(),
                    LOOKING_GLASS
                );
                InstallProgress::Both(Arc::new(MultiProgress::new()))
            }
            _ => {
                println!("Installing packages from package.json..");
                InstallProgress::Logging
            }
        };

        Installer::create_modules_dir()?;

        let client = reqwest::Client::new();
        let dependency_map_mux = Arc::new(Mutex::new(HashMap::new()));
        let install_context = InstallContext {
            client,
            dependency_map_mux: Arc::clone(&dependency_map_mux),
            progress: progress.clone(),
        };

        if matches!(progress, InstallProgress::Both(_)) {
            println!(
                "{} {}Fetching packages...",
                style("[2/4]").bold().dim(),
                TRUCK
            );
        }

        Installer::install_all(install_context, all_deps).await?;
        TaskAllocator::block_until_done();
        progress.finish();

        if matches!(progress, InstallProgress::Both(_)) {
            println!(
                "{} {}Linking dependencies...",
                style("[3/4]").bold().dim(),
                CLIP
            );
        }

        Installer::setup_cache_packages(Arc::clone(&dependency_map_mux))?;
        Installer::write_project_lockfile(Arc::clone(&dependency_map_mux))?;

        {
            let dep_map = dependency_map_mux
                .lock()
                .map_err(|_| CommandError::MutexPoisoned)?;
            let desired = Cache::desired_node_modules(&dep_map);
            let current = Cache::read_current_node_modules(nm_path);
            Cache::apply_node_modules_diff(&current, &desired, nm_path)?;
            Cache::link_all_to_node_modules(&dep_map, nm_path)?;
        }

        if matches!(progress, InstallProgress::Both(_)) {
            println!(
                "{} {}Building fresh packages...",
                style("[4/4]").bold().dim(),
                PAPER
            );
        }

        // Write the oxide-state sidecar so the next run can take the fast path.
        let lock_mtime_after = std::fs::metadata(OXIDE_LOCK)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64);
        let mut state_val = serde_json::json!({ "pkgjson_fingerprint": pkgjson_fingerprint });
        if let Some(mtime) = lock_mtime_after {
            state_val["lockfile_mtime"] = serde_json::json!(mtime);
        }
        let _ = std::fs::write(
            state_path,
            serde_json::to_string(&state_val).unwrap_or_default(),
        );

        self.note_ignore_scripts();

        if matches!(progress, InstallProgress::Both(_)) {
            println!("{} Done in {}", SPARKLE, HumanDuration(started_at.elapsed()));
        } else {
            println!("Done in {:.2}s", started_at.elapsed().as_secs_f64());
        }

        Ok(())
    }

    async fn execute_single(&self) -> Result<(), CommandError> {
        let cfg = OxideConfig::load();
        let progress_mode = cfg.get("install-progress").unwrap_or("logging");

        let started_at = std::time::Instant::now();

        let progress = match progress_mode {
            "bar" => {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::with_template("{spinner:.cyan} {msg} [{pos} packages]")
                        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                );
                pb.set_message(format!("Installing '{}'..", self.package_name));
                pb.enable_steady_tick(std::time::Duration::from_millis(80));
                InstallProgress::Bar(Arc::new(pb))
            }
            "both" => {
                println!(
                    "{} {}Resolving packages...",
                    style("[1/4]").bold().dim(),
                    LOOKING_GLASS
                );
                InstallProgress::Both(Arc::new(MultiProgress::new()))
            }
            _ => {
                println!("Installing '{}'..", self.package_name);
                InstallProgress::Logging
            }
        };

        let client = reqwest::Client::new();
        let semantic_version = self.semantic_version.as_ref();
        let full_version = Versions::resolve_full_version(semantic_version);
        let full_version = full_version.as_ref();

        let (is_cached, cached_version) =
            Cache::exists(&self.package_name, full_version, semantic_version).await?;

        Installer::create_modules_dir()?;

        if is_cached {
            let version = cached_version.ok_or(CommandError::InvalidVersion)?;
            if !crate::util::is_safe_path_component(&version) {
                return Err(CommandError::MalformedPackageId(version));
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
                progress.finish();
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

        if matches!(progress, InstallProgress::Both(_)) {
            println!(
                "{} {}Fetching packages...",
                style("[2/4]").bold().dim(),
                TRUCK
            );
        }

        let install_context = InstallContext {
            client,
            dependency_map_mux: Arc::clone(&dependency_map_mux),
            progress: progress.clone(),
        };

        let stringified = Versions::stringify(&version_data.name, &version_data.version);
        let resolved_name = version_data.name.clone();
        let resolved_version = version_data.version.clone();

        if !crate::util::is_safe_path_component(&stringified) {
            return Err(CommandError::MalformedPackageId(stringified));
        }

        if matches!(progress, InstallProgress::Both(_)) {
            println!(
                "{} {}Linking dependencies...",
                style("[3/4]").bold().dim(),
                CLIP
            );
            println!(
                "{} {}Building fresh packages...",
                style("[4/4]").bold().dim(),
                PAPER
            );
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
        progress.finish();

        Installer::setup_cache_packages(Arc::clone(&dependency_map_mux))?;
        Installer::write_project_lockfile(dependency_map_mux)?;
        Cache::load_cached_version(stringified, Path::new(NODE_MODULES))?;

        if !self.no_save {
            Self::update_package_json(&resolved_name, &resolved_version, self.save_dev)?;
        }

        self.note_ignore_scripts();

        if matches!(progress, InstallProgress::Both(_)) {
            println!("{} Done in {}", SPARKLE, HumanDuration(started_at.elapsed()));
        } else {
            println!("Done in {:.2}s", started_at.elapsed().as_secs_f64());
        }
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
            .ok_or(CommandError::ConfigDirUnavailable)?
            .join("oxide");
        Ok(base.join(GLOBAL_MODULES_SUBDIR))
    }

    fn global_bin_dir() -> Result<PathBuf, CommandError> {
        let cfg = OxideConfig::load();
        let base = match cfg.get("global-bin-dir") {
            Some(dir) => return Ok(PathBuf::from(dir)),
            None => dirs::config_dir()
                .ok_or(CommandError::ConfigDirUnavailable)?
                .join("oxide"),
        };
        Ok(base.join(GLOBAL_BIN_SUBDIR))
    }

    async fn execute_global(&self) -> Result<(), CommandError> {
        let cfg = OxideConfig::load();
        let progress_mode = cfg.get("install-progress").unwrap_or("logging");

        let started_at = std::time::Instant::now();

        let progress = match progress_mode {
            "bar" => {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::with_template("{spinner:.cyan} {msg} [{pos} packages]")
                        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                );
                pb.set_message(format!("Installing '{}' globally..", self.package_name));
                pb.enable_steady_tick(std::time::Duration::from_millis(80));
                InstallProgress::Bar(Arc::new(pb))
            }
            "both" => {
                println!(
                    "{} {}Resolving packages...",
                    style("[1/4]").bold().dim(),
                    LOOKING_GLASS
                );
                InstallProgress::Both(Arc::new(MultiProgress::new()))
            }
            _ => {
                println!("Installing '{}' globally..", self.package_name);
                InstallProgress::Logging
            }
        };

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
            progress.finish();
            let version = cached_version.ok_or(CommandError::InvalidVersion)?;
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

            if matches!(progress, InstallProgress::Both(_)) {
                println!(
                    "{} {}Fetching packages...",
                    style("[2/4]").bold().dim(),
                    TRUCK
                );
            }

            let install_context = InstallContext {
                client,
                dependency_map_mux: Arc::clone(&dependency_map_mux),
                progress: progress.clone(),
            };

            let stringified = Versions::stringify(&version_data.name, &version_data.version);
            let resolved_name = version_data.name.clone();
            let resolved_version = version_data.version.clone();

            if !crate::util::is_safe_path_component(&stringified) {
                return Err(CommandError::MalformedPackageId(stringified));
            }

            if matches!(progress, InstallProgress::Both(_)) {
                println!(
                    "{} {}Linking dependencies...",
                    style("[3/4]").bold().dim(),
                    CLIP
                );
                println!(
                    "{} {}Building fresh packages...",
                    style("[4/4]").bold().dim(),
                    PAPER
                );
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
            progress.finish();

            Installer::setup_cache_packages(Arc::clone(&dependency_map_mux))?;
            Installer::write_project_lockfile(dependency_map_mux)?;
            Cache::load_cached_version(stringified, &global_nm)?;
            (resolved_name, resolved_version)
        };

        let short_name = resolved_name
            .split('/')
            .next_back()
            .unwrap_or(&resolved_name);
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
