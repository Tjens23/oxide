use std::fs;
use std::process::Command;

use async_trait::async_trait;
use semver::Version;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct VersionHandler {
    bump: String,
}

#[async_trait]
impl CommandHandler for VersionHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        self.bump = args.next().ok_or(ParseError::MissingArgument(
            String::from("version bump (major|minor|patch) or explicit semver"),
        ))?;
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let pkg_raw =
            fs::read_to_string("package.json").map_err(CommandError::FailedToReadFile)?;
        let mut pkg: serde_json::Value =
            serde_json::from_str(&pkg_raw).map_err(CommandError::ParsingFailed)?;

        let current_version_str = pkg["version"]
            .as_str()
            .ok_or(CommandError::InvalidVersion)?;

        let mut version =
            Version::parse(current_version_str).map_err(|_| CommandError::InvalidVersion)?;

        match self.bump.to_lowercase().as_str() {
            "major" => {
                version.major += 1;
                version.minor = 0;
                version.patch = 0;
            }
            "minor" => {
                version.minor += 1;
                version.patch = 0;
            }
            "patch" => {
                version.patch += 1;
            }
            explicit => {
                version =
                    Version::parse(explicit).map_err(|_| CommandError::InvalidVersion)?;
            }
        }

        let new_version = version.to_string();
        let tag = format!("v{}", new_version);

        pkg["version"] = serde_json::Value::String(new_version.clone());
        let updated = serde_json::to_string_pretty(&pkg)
            .map_err(CommandError::FailedToSerializePackageLock)?;
        fs::write("package.json", updated).map_err(CommandError::FailedToWriteFile)?;

        println!("Updated package.json → {}", new_version);

        // Stage package.json
        let status = Command::new("git")
            .args(["add", "package.json"])
            .status()
            .map_err(|e| CommandError::GitFailed(format!("failed to run git add: {e}")))?;
        if !status.success() {
            return Err(CommandError::GitFailed(format!(
                "git add exited with {}",
                status.code().unwrap_or(-1)
            )));
        }

        let status = Command::new("git")
            .args(["commit", "-m", &tag])
            .status()
            .map_err(|e| CommandError::GitFailed(format!("failed to run git commit: {e}")))?;
        if !status.success() {
            return Err(CommandError::GitFailed(format!(
                "git commit exited with {}",
                status.code().unwrap_or(-1)
            )));
        }

        let status = Command::new("git")
            .args(["tag", "-a", &tag, "-m", &tag])
            .status()
            .map_err(|e| CommandError::GitFailed(format!("failed to run git tag: {e}")))?;
        if !status.success() {
            return Err(CommandError::GitFailed(format!(
                "git tag exited with {}",
                status.code().unwrap_or(-1)
            )));
        }

        println!("Created git tag {}", tag);
        Ok(())
    }
}
