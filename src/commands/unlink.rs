use std::path::Path;

use async_trait::async_trait;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;
use super::link::global_node_modules;

fn is_link(path: &Path) -> bool {
    std::fs::read_link(path).is_ok()
}

fn unlink_all() -> Result<(), CommandError> {
    let nm = Path::new("./node_modules");
    if !nm.exists() {
        println!("No node_modules directory found.");
        return Ok(());
    }

    let mut count = 0;
    for entry in std::fs::read_dir(nm).map_err(CommandError::FailedToWriteFile)? {
        let entry = entry.map_err(CommandError::FailedDirectoryEntry)?;
        let path = entry.path();
        if is_link(&path) {
            std::fs::remove_dir_all(&path).map_err(CommandError::FailedToWriteFile)?;
            println!("Unlinked '{}'", entry.file_name().to_string_lossy());
            count += 1;
        }
    }

    if count == 0 {
        println!("No linked packages found in ./node_modules");
    }
    Ok(())
}

fn unlink_package(pkg: &str) -> Result<(), CommandError> {
    let path = Path::new("./node_modules").join(pkg);

    if !path.exists() && std::fs::read_link(&path).is_err() {
        println!("'{}' is not installed in ./node_modules", pkg);
        return Ok(());
    }

    if !is_link(&path) {
        return Err(CommandError::FailedToWriteFile(std::io::Error::other(
            format!("'{}' is not a linked package; use `oxide uninstall` instead", pkg),
        )));
    }

    std::fs::remove_dir_all(&path).map_err(CommandError::FailedToWriteFile)?;
    println!("Unlinked '{}'", pkg);
    Ok(())
}

fn unlink_global(pkg: &str) -> Result<(), CommandError> {
    let global_nm = global_node_modules().ok_or_else(|| {
        CommandError::FailedToWriteFile(std::io::Error::other(
            "cannot determine data directory",
        ))
    })?;

    let path = global_nm.join(pkg);
    if !path.exists() && std::fs::read_link(&path).is_err() {
        println!("'{}' is not linked globally", pkg);
        return Ok(());
    }

    std::fs::remove_dir_all(&path).map_err(CommandError::FailedToWriteFile)?;
    println!("Removed '{}' from global node_modules", pkg);
    Ok(())
}

#[derive(Default)]
pub struct UnlinkHandler {
    package_name: Option<String>,
    global: bool,
}

#[async_trait]
impl CommandHandler for UnlinkHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--global" | "-g" => self.global = true,
                _ => self.package_name = Some(arg),
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        if self.global {
            let pkg = self.package_name.as_deref().ok_or_else(|| {
                ParseError::MissingArgument("--global <package>".to_string())
            });
            return match pkg {
                Ok(p) => unlink_global(p),
                Err(e) => Err(CommandError::FailedToWriteFile(std::io::Error::other(e.to_string()))),
            };
        }

        match &self.package_name {
            None => unlink_all(),
            Some(pkg) => unlink_package(pkg),
        }
    }
}
