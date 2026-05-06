use std::{env::Args, path::PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;
use super::publish::pack;

#[derive(Default)]
pub struct PackHandler {
    out_dir: Option<PathBuf>,
}

#[async_trait]
impl CommandHandler for PackHandler {
    fn parse(&mut self, args: &mut Args) -> Result<(), ParseError> {
        let mut peekable = args.peekable();
        while let Some(arg) = peekable.next() {
            if arg == "--out-dir" {
                let dir = peekable
                    .next()
                    .ok_or_else(|| ParseError::MissingArgument("--out-dir <path>".to_string()))?;
                self.out_dir = Some(PathBuf::from(dir));
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let cwd = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;

        let pkg_raw = std::fs::read_to_string(cwd.join("package.json"))
            .map_err(CommandError::FailedToReadFile)?;
        let pkg_json: Value =
            serde_json::from_str(&pkg_raw).map_err(CommandError::ParsingFailed)?;

        let name = pkg_json
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CommandError::FailedToWriteFile(std::io::Error::other(
                    "missing \"name\" in package.json",
                ))
            })?;

        let version = pkg_json
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CommandError::FailedToWriteFile(std::io::Error::other(
                    "missing \"version\" in package.json",
                ))
            })?;

        println!("Packing {}@{}…", name, version);

        let tarball = pack(&cwd)?;

        let filename = format!(
            "{}-{}.tgz",
            name.replace('/', "-").trim_start_matches('-'),
            version
        );

        let out_dir = self
            .out_dir
            .clone()
            .unwrap_or_else(|| cwd.clone());

        std::fs::create_dir_all(&out_dir).map_err(CommandError::FailedToCreateFile)?;

        let dest = out_dir.join(&filename);
        std::fs::write(&dest, &tarball).map_err(CommandError::FailedToWriteFile)?;

        println!(
            "Created {} ({} bytes)",
            dest.display(),
            tarball.len()
        );

        Ok(())
    }
}
