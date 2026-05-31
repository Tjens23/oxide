use async_trait::async_trait;

use crate::{
    errors::{CommandError, ParseError},
    workspace,
};

use super::command_handler::CommandHandler;

#[derive(Default)]
pub struct WorkspacesHandler {
    filter: Option<String>,
}

#[async_trait]
impl CommandHandler for WorkspacesHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        let mut peekable = args.peekable();
        while let Some(arg) = peekable.next() {
            if arg == "--filter" || arg == "-F" {
                let pat = peekable
                    .next()
                    .ok_or_else(|| ParseError::MissingArgument("--filter <pattern>".to_string()))?;
                self.filter = Some(pat);
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let root = std::env::current_dir().map_err(CommandError::FailedToWriteFile)?;
        let packages = workspace::discover(&root)?;

        let matched: Vec<_> = match &self.filter {
            Some(f) => workspace::apply_filter(&packages, f),
            None => packages.iter().collect(),
        };

        if matched.is_empty() {
            println!("No workspace packages found.");
            return Ok(());
        }

        println!("{:<35} {:<12} {}", "Name", "Version", "Path");
        println!("{}", "-".repeat(72));

        for pkg in &matched {
            println!(
                "{:<35} {:<12} {}",
                pkg.name,
                pkg.version,
                pkg.path.to_string_lossy()
            );
            if !pkg.scripts.is_empty() {
                let mut sorted = pkg.scripts.clone();
                sorted.sort();
                println!("  scripts: {}", sorted.join(", "));
            }
        }

        println!("\n{} package(s)", matched.len());
        Ok(())
    }
}
