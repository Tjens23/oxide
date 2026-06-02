use async_trait::async_trait;
use colored::Colorize;
use serde_json::Value;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct UninstallHandler {
    package_name: String,
}

impl UninstallHandler {
    fn remove_from_package_json(package_name: &str) -> Result<(), CommandError> {
        let content =
            std::fs::read_to_string("./package.json").map_err(CommandError::FailedToWriteFile)?;
        let mut json: Value =
            serde_json::from_str(&content).map_err(CommandError::ParsingFailed)?;

        if let Some(deps) = json.get_mut("dependencies").and_then(|d| d.as_object_mut()) {
            deps.remove(package_name);
        }

        let output = serde_json::to_string_pretty(&json)
            .map_err(CommandError::FailedToSerializePackageLock)?;
        std::fs::write("./package.json", output).map_err(CommandError::FailedToWriteFile)?;
        Ok(())
    }

    fn remove_from_node_modules(package_name: &str) -> Result<(), CommandError> {
        let path = format!("./node_modules/{}", package_name);
        if std::path::Path::new(&path).exists() {
            std::fs::remove_dir_all(&path).map_err(CommandError::FailedToWriteFile)?;
        }
        Ok(())
    }
}

#[async_trait]
impl CommandHandler for UninstallHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        self.package_name = args
            .next()
            .ok_or(ParseError::MissingArgument(String::from("package name")))?;
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        println!("{} '{}'..", "Uninstalling".cyan(), self.package_name);
        Self::remove_from_node_modules(&self.package_name)?;
        Self::remove_from_package_json(&self.package_name)?;
        println!("{} '{}'", "Uninstalled".green(), self.package_name);
        Ok(())
    }
}
