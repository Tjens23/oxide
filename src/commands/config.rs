use async_trait::async_trait;

use crate::{
    config::{OxideConfig, VALID_KEYS},
    errors::{CommandError, ParseError},
};

use super::command_handler::CommandHandler;

enum ConfigSubcommand {
    Set { key: String, value: String },
    Get { key: String },
    List,
    Delete { key: String },
}

#[derive(Default)]
pub struct ConfigHandler {
    subcommand: Option<ConfigSubcommand>,
}

#[async_trait]
impl CommandHandler for ConfigHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        let sub = args
            .next()
            .ok_or_else(|| ParseError::MissingArgument("set | get | list | delete".to_owned()))?;

        self.subcommand = Some(match sub.as_str() {
            "list" | "ls" => ConfigSubcommand::List,

            "get" => {
                let key = args
                    .next()
                    .ok_or_else(|| ParseError::MissingArgument("config get <key>".to_owned()))?;
                ConfigSubcommand::Get { key }
            }

            "set" => {
                let key = args.next().ok_or_else(|| {
                    ParseError::MissingArgument("config set <key> <value>".to_owned())
                })?;
                let value = args.next().ok_or_else(|| {
                    ParseError::MissingArgument("config set <key> <value>".to_owned())
                })?;
                ConfigSubcommand::Set { key, value }
            }

            "delete" | "unset" | "rm" => {
                let key = args
                    .next()
                    .ok_or_else(|| ParseError::MissingArgument("config delete <key>".to_owned()))?;
                ConfigSubcommand::Delete { key }
            }

            other => {
                return Err(ParseError::MissingArgument(format!(
                    "unknown config subcommand '{}'; expected: set | get | list | delete",
                    other
                )));
            }
        });

        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        match self
            .subcommand
            .as_ref()
            .expect("parse must be called first")
        {
            ConfigSubcommand::List => {
                let cfg = OxideConfig::load();
                println!("{:<20} {:<40} {}", "Key", "Description", "Value");
                println!("{}", "-".repeat(75));
                for (key, desc) in VALID_KEYS {
                    let value = cfg.get(key).unwrap_or("(not set)");
                    println!("{:<20} {:<40} {}", key, desc, value);
                }
            }

            ConfigSubcommand::Get { key } => {
                let cfg = OxideConfig::load();
                match cfg.get(key) {
                    Some(v) => println!("{}", v),
                    None => println!("(not set)"),
                }
            }

            ConfigSubcommand::Set { key, value } => {
                let mut cfg = OxideConfig::load();
                cfg.set(key, value.clone())?;
                cfg.save().map_err(CommandError::ConfigWriteFailed)?;
                println!("Set '{}' = '{}'", key, value);
            }

            ConfigSubcommand::Delete { key } => {
                let mut cfg = OxideConfig::load();
                if cfg.delete(key) {
                    cfg.save().map_err(CommandError::ConfigWriteFailed)?;
                    println!("Deleted '{}'", key);
                } else {
                    println!("'{}' was not set", key);
                }
            }
        }

        Ok(())
    }
}
