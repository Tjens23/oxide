use std::env::Args;

use async_trait::async_trait;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

const RELEASE_BASE_URL: &str =
    "https://github.com/tjens23/oxide/releases/";

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct SelfUpgradeHandler;

#[async_trait]
impl CommandHandler for SelfUpgradeHandler {
    fn parse(&mut self, _args: &mut Args) -> Result<(), ParseError> {
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        println!("Upgrading oxide (current: v{})..", CURRENT_VERSION);

        let binary_name = if cfg!(target_os = "windows") {
            "oxide-windows.exe"
        } else if cfg!(target_os = "macos") {
            "oxide-macos"
        } else {
            "oxide-linux"
        };

        let url = format!("{}/{}", RELEASE_BASE_URL, binary_name);
        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(CommandError::HTTPFailed)?;
        let bytes = response
            .bytes()
            .await
            .map_err(CommandError::FailedResponseBytes)?;

        let current_exe =
            std::env::current_exe().map_err(CommandError::FailedToWriteFile)?;

        #[cfg(windows)]
        {
            // On Windows, rename the running binary (allowed) then write the new one.
            let old_path = current_exe.with_extension("old");
            std::fs::rename(&current_exe, &old_path)
                .map_err(CommandError::FailedToWriteFile)?;
            if let Err(e) = std::fs::write(&current_exe, &bytes) {
                // Restore the original binary if the write fails.
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

        println!("oxide upgraded successfully.");
        Ok(())
    }
}
