use async_trait::async_trait;
use std::env::Args;

use crate::errors::{
    CommandError,
    ParseError::{self, CommandNotFound},
};

use super::init::InitHandler;
use super::install::InstallHandler;
use super::link::LinkHandler;
use super::publish::PublishHandler;
use super::self_upgrade::SelfUpgradeHandler;
use super::uninstall::UninstallHandler;
use super::unlink::UnlinkHandler;
use super::upgrade::UpgradeHandler;
use super::version::VersionHandler;

#[async_trait]
pub trait CommandHandler {
    fn parse(&mut self, args: &mut Args) -> Result<(), ParseError>;
    async fn execute(&self) -> Result<(), CommandError>;
}

pub async fn handle_args(mut args: Args) -> Result<(), ParseError> {
    args.next(); 

    let command = match args.next() {
        Some(command) => command,
        None => {
           for (name, description) in [
                ("install", "Install a package"),
                ("uninstall", "Uninstall a package"),
                ("upgrade", "Upgrade a package"),
                ("self-upgrade", "Upgrade the oxide tool itself"),
                ("version", "Bump package.json version, commit, and create a git tag"),
                ("init", "Initialize a new project with a package.json"),                ("link", "Link a package globally or into node_modules"),
                ("unlink", "Remove a linked package"),                ("publish", "Publish a package to the npm registry"),
                ("login", "Authenticate with the npm registry to allow installing private packages"),
            ] {
                println!("  {:<12} {}", name, description);
            }
            return Ok(());
        }
    };

    let mut command_handler: Box<dyn CommandHandler> = match command.to_lowercase().as_str() {
        "login" => Box::new(super::login::LoginHandler::default()),
        "uninstall" => Box::new(UninstallHandler::default()),
        "upgrade" => Box::new(UpgradeHandler::default()),
        "self-upgrade" => Box::new(SelfUpgradeHandler::default()),
        "init" => Box::new(InitHandler::default()),
        "install" => Box::new(InstallHandler::default()),
        "link" | "ln" => Box::new(LinkHandler::default()),
        "unlink" => Box::new(UnlinkHandler::default()),
        "publish" => Box::new(PublishHandler::default()),
        "version" => Box::new(VersionHandler::default()),
        _ => return Err(CommandNotFound(command.to_string())),
    };

    command_handler.parse(&mut args)?;
    let command_result = command_handler.execute().await;

    if let Err(e) = command_result {
        println!("Command error: {e}");
    }

    Ok(())
}