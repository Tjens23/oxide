use std::{
    env,
    env::Args,
    fs::File,
    io::{self, BufRead, Write},
};

use async_trait::async_trait;
use serde::Serialize;

use crate::errors::{CommandError, ParseError};

use super::command_handler::CommandHandler;

#[derive(Serialize)]
struct Scripts {
    test: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageJson {
    name: String,
    version: String,
    description: String,
    main: String,
    scripts: Scripts,
    keywords: Vec<String>,
    author: String,
    license: String,
    package_manager: String,
}

#[derive(Default)]
pub struct InitHandler;

fn prompt(label: &str, default: &str) -> String {
    let mut stdout = io::stdout();
    if default.is_empty() {
        write!(stdout, "{}: ", label).unwrap();
    } else {
        write!(stdout, "{} ({}): ", label, default).unwrap();
    }
    stdout.flush().unwrap();

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).unwrap();
    let trimmed = line.trim().to_string();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed
    }
}

#[async_trait]
impl CommandHandler for InitHandler {
    fn parse(&mut self, _args: &mut Args) -> Result<(), ParseError> {
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let default_name = env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| String::from("my-package"));

        println!("This utility will walk you through creating a package.json file.");
        println!("Press ^C at any time to quit.\n");

        let name = prompt("package name", &default_name);
        let version = prompt("version", "1.0.0");
        let description = prompt("description", "");
        let main = prompt("entry point", "index.js");
        let author = prompt("author", "");
        let license = prompt("license", "ISC");

        let package_json = PackageJson {
            name,
            version,
            description,
            main,
            scripts: Scripts {
                test: String::from("echo \"Error: no test specified\" && exit 1"),
            },
            keywords: Vec::new(),
            author,
            license,
            package_manager: format!("oxide@{}", env!("CARGO_PKG_VERSION")),
        };

        let json = serde_json::to_string_pretty(&package_json)
            .map_err(CommandError::FailedToSerializePackageLock)?;

        let mut file =
            File::create("package.json").map_err(CommandError::FailedToCreateFile)?;
        file.write_all(json.as_bytes())
            .map_err(CommandError::FailedToWriteFile)?;

        println!("\nWrote to package.json");
        Ok(())
    }
}
