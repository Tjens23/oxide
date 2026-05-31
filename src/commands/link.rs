use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    constants::{NODE_MODULES, PACKAGE_JSON},
    errors::{CommandError, ParseError},
};

use super::command_handler::CommandHandler;

pub fn global_node_modules() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("oxide").join("global").join(NODE_MODULES))
}

fn read_package_name(dir: &Path) -> Result<String, CommandError> {
    let content =
        std::fs::read_to_string(dir.join(PACKAGE_JSON)).map_err(CommandError::FailedToWriteFile)?;
    let json: Value = serde_json::from_str(&content).map_err(CommandError::ParsingFailed)?;
    json.get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            CommandError::FailedToWriteFile(std::io::Error::other(
                "package.json is missing a \"name\" field",
            ))
        })
}

fn create_link(src: &Path, dest: &Path) -> Result<(), CommandError> {
    if dest.exists() || std::fs::read_link(dest).is_ok() {
        std::fs::remove_dir_all(dest).map_err(CommandError::FailedToWriteFile)?;
    }
    crate::util::create_dir_link(src, dest).map_err(CommandError::FailedToWriteFile)
}

fn link_current_to_global() -> Result<(), CommandError> {
    let cwd = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
    let name = read_package_name(&cwd)?;
    if !crate::util::is_safe_path_component(&name) {
        return Err(CommandError::FailedToWriteFile(std::io::Error::other(
            "package name contains unsafe path characters",
        )));
    };

    let global_nm = global_node_modules().ok_or_else(|| {
        CommandError::FailedToWriteFile(std::io::Error::other("cannot determine data directory"))
    })?;
    std::fs::create_dir_all(&global_nm).map_err(CommandError::FailedToWriteFile)?;

    let dest = global_nm.join(&name);
    create_link(&cwd, &dest)?;

    println!("Linked '{}' → global node_modules", name);
    Ok(())
}

fn link_dir_to_local(dir: &Path) -> Result<(), CommandError> {
    let abs = std::fs::canonicalize(dir).map_err(CommandError::FailedToWriteFile)?;
    let name = read_package_name(&abs)?;
    if !crate::util::is_safe_path_component(&name) {
        return Err(CommandError::FailedToWriteFile(std::io::Error::other(
            "package name contains unsafe path characters",
        )));
    }

    std::fs::create_dir_all("./node_modules").map_err(CommandError::FailedToWriteFile)?;
    let dest = Path::new("./node_modules").join(&name);
    create_link(&abs, &dest)?;

    println!(
        "Linked '{}': {} → ./node_modules/{}",
        name,
        abs.display(),
        name
    );
    Ok(())
}

fn link_global_to_local(pkg: &str) -> Result<(), CommandError> {
    let global_nm = global_node_modules().ok_or_else(|| {
        CommandError::FailedToWriteFile(std::io::Error::other("cannot determine data directory"))
    })?;

    let src = global_nm.join(pkg);
    if !src.exists() {
        return Err(CommandError::FailedToWriteFile(std::io::Error::other(
            format!(
                "'{}' is not linked globally; run `oxide link` inside that package first",
                pkg
            ),
        )));
    }

    std::fs::create_dir_all("./node_modules").map_err(CommandError::FailedToWriteFile)?;
    let dest = Path::new("./node_modules").join(pkg);
    create_link(&src, &dest)?;

    println!("Linked global '{}' → ./node_modules/{}", pkg, pkg);
    Ok(())
}

#[derive(Default)]
pub struct LinkHandler {
    target: Option<String>,
}

#[async_trait]
impl CommandHandler for LinkHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        self.target = args.next();
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        match &self.target {
            None => link_current_to_global(),
            Some(t) => {
                let path = Path::new(t);
                if path.is_dir() {
                    link_dir_to_local(path)
                } else {
                    link_global_to_local(t)
                }
            }
        }
    }
}
