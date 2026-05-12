
use async_trait::async_trait;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;
use super::install::InstallHandler;

#[derive(Default)]
pub struct UpgradeHandler {
    package_name: String,
}

impl UpgradeHandler {
    fn remove_from_node_modules(package_name: &str) -> Result<(), CommandError> {
        let path = format!("./node_modules/{}", package_name);
        if std::path::Path::new(&path).exists() {
            std::fs::remove_dir_all(&path).map_err(CommandError::FailedToWriteFile)?;
        }
        Ok(())
    }
}

#[async_trait]
impl CommandHandler for UpgradeHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        self.package_name = args
            .next()
            .ok_or(ParseError::MissingArgument(String::from("package name")))?;
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        println!("Upgrading '{}'..", self.package_name);
        Self::remove_from_node_modules(&self.package_name)?;
        let install_handler = InstallHandler::new(self.package_name.clone());
        install_handler.execute().await
    }
}
