use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use semver::VersionReq;
use serde_json::Value;

use crate::{
    cache::{Cache, CACHE_DIRECTORY},
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
}

impl InstallHandler {
    pub fn new(package_name: String) -> Self {
        Self {
            package_name,
            semantic_version: None,
            filter: None,
        }
    }
}

impl InstallHandler {
    fn update_package_json(package_name: &str, version: &str) -> Result<(), CommandError> {
        let content = std::fs::read_to_string("./package.json").unwrap_or_else(|_| "{}".to_string());
        let mut json: Value = serde_json::from_str(&content).map_err(CommandError::ParsingFailed)?;

        let deps = json
            .as_object_mut()
            .ok_or_else(|| CommandError::ParsingFailed(serde_json::from_str::<Value>("null").unwrap_err()))?
            .entry("dependencies")
            .or_insert(Value::Object(serde_json::Map::new()));

        if let Some(map) = deps.as_object_mut() {
            map.insert(package_name.to_string(), Value::String(format!("^{}", version)));
        }

        let output = serde_json::to_string_pretty(&json).map_err(CommandError::FailedToSerializePackageLock)?;
        std::fs::write("./package.json", output).map_err(CommandError::FailedToWriteFile)?;
        Ok(())
    }

}

#[async_trait]
impl CommandHandler for InstallHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        let mut rest: Vec<String> = args.collect();

        let mut i = 0;
        while i < rest.len() {
            match rest[i].as_str() {
                "--filter" | "-F" => {
                    let pat = rest
                        .get(i + 1)
                        .cloned()
                        .ok_or_else(|| ParseError::MissingArgument("--filter <pattern>".to_string()))?;
                    self.filter = Some(pat);
                    rest.remove(i);
                    rest.remove(i);
                }
                _ => i += 1,
            }
        }

        let package_details = rest
            .into_iter()
            .next()
            .ok_or(ParseError::MissingArgument(String::from("package name")))?;

        let (package_name, semantic_version) =
            Versions::parse_semantic_package_details(package_details)?;
        self.package_name = package_name;
        self.semantic_version = semantic_version;

        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
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
                std::env::set_current_dir(&pkg.path)
                    .map_err(CommandError::FailedToWriteFile)?;
                Box::pin(self.execute_single()).await?;
            }

            std::env::set_current_dir(&root).map_err(CommandError::FailedToWriteFile)?;
            return Ok(());
        }

        self.execute_single().await
    }
}

impl InstallHandler {
    async fn execute_single(&self) -> Result<(), CommandError> {
        // In future we could automatically find a version that is valid for both limits to save storage, but that's not neccessary right now
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
            let stringified = Versions::stringify(&self.package_name, &version);
            let lockfile_path = format!(
                "{}/{}/package/oxide-lock.json",
                *CACHE_DIRECTORY, stringified
            );
            let lockfile_complete = std::fs::read_to_string(&lockfile_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<crate::types::PackageLock>(&raw).ok())
                .map(|lf| !lf.dependencies.is_empty())
                .unwrap_or(false);

            if lockfile_complete {
                Cache::load_cached_version(stringified);
                Self::update_package_json(&self.package_name, &version)?;
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

        // Blocks the main thread however it's not going to have a huge performance impact on tokio
        TaskAllocator::block_until_done();

        Installer::setup_cache_packages(Arc::clone(&dependency_map_mux))?;
        Installer::write_project_lockfile(dependency_map_mux)?;
        Cache::load_cached_version(stringified);
        Self::update_package_json(&resolved_name, &resolved_version)?;

        println!("Done in {:.2}s", started_at.elapsed().as_secs_f64());
        Ok(())
    }
}