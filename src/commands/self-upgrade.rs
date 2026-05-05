use std::env::Args;

use async_trait::async_trait;
use serde::Deserialize;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

const GITHUB_API_LATEST: &str =
    "https://api.github.com/repos/tjens23/oxide/releases/latest";
const GITHUB_DOWNLOAD_BASE: &str =
    "https://github.com/tjens23/oxide/releases/download";

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
}

#[derive(Default)]
pub struct SelfUpgradeHandler;

#[async_trait]
impl CommandHandler for SelfUpgradeHandler {
    fn parse(&mut self, _args: &mut Args) -> Result<(), ParseError> {
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let client = reqwest::Client::builder()
            .user_agent("oxide-self-upgrade")
            .build()
            .map_err(CommandError::HTTPFailed)?;

        let release: LatestRelease = client
            .get(GITHUB_API_LATEST)
            .send()
            .await
            .map_err(CommandError::HTTPFailed)?
            .json()
            .await
            .map_err(CommandError::HTTPFailed)?;

        let latest_tag = release.tag_name; // e.g. "v0.5.1"
        let latest_version = latest_tag.trim_start_matches('v');

        if latest_version == CURRENT_VERSION {
            println!("oxide is already up to date (v{}).", CURRENT_VERSION);
            return Ok(());
        }

        println!(
            "Upgrading oxide: v{} → {} …",
            CURRENT_VERSION, latest_tag
        );

        let binary_name = if cfg!(target_os = "windows") {
            "oxide-windows.exe"
        } else if cfg!(target_os = "macos") {
            "oxide-macos"
        } else {
            "oxide-linux"
        };

        let url = format!("{}/{}/{}", GITHUB_DOWNLOAD_BASE, latest_tag, binary_name);

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(CommandError::HTTPFailed)?;

        let status = response.status();
        if !status.is_success() {
            return Err(CommandError::LoginFailed {
                status: status.as_u16(),
                body: format!(
                    "could not download binary for {} — release asset may not be ready yet",
                    latest_tag
                ),
            });
        }

        let bytes = response
            .bytes()
            .await
            .map_err(CommandError::FailedResponseBytes)?;

        if bytes.len() < 1024 {
            return Err(CommandError::GitFailed(format!(
                "downloaded binary is suspiciously small ({} bytes) — aborting to avoid corrupting the installation",
                bytes.len()
            )));
        }

        let current_exe =
            std::env::current_exe().map_err(CommandError::FailedToWriteFile)?;

        #[cfg(windows)]
        {
            let old_path = current_exe.with_extension("old");
            std::fs::rename(&current_exe, &old_path)
                .map_err(CommandError::FailedToWriteFile)?;
            if let Err(e) = std::fs::write(&current_exe, &bytes) {
                let _ = std::fs::rename(&old_path, &current_exe);
                return Err(CommandError::FailedToWriteFile(e));
            }
            let _ = std::fs::remove_file(&old_path);
        }

        #[cfg(not(windows))]
        {
            let temp_path = current_exe.with_extension("tmp");
            std::fs::write(&temp_path, &bytes).map_err(CommandError::FailedToWriteFile)?;
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    &temp_path,
                    std::fs::Permissions::from_mode(0o755),
                )
                .map_err(CommandError::FailedToWriteFile)?;
            }
            std::fs::rename(&temp_path, &current_exe)
                .map_err(CommandError::FailedToWriteFile)?;
        }

        println!("oxide upgraded to {} successfully.", latest_tag);
        Ok(())
    }
}
