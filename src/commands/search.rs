use async_trait::async_trait;
use console::style;

use crate::errors::{CommandError, ParseError};
use crate::http::HTTPRequest;

use super::command_handler::CommandHandler;

const DEFAULT_LIMIT: u8 = 20;

#[derive(Default)]
pub struct SearchHandler {
    query: String,
    limit: u8,
    from: u32,
}

#[async_trait]
impl CommandHandler for SearchHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        self.limit = DEFAULT_LIMIT;
        self.from = 0;

        let mut peekable = args.peekable();
        while let Some(arg) = peekable.next() {
            match arg.as_str() {
                "--limit" | "-n" => {
                    let value = peekable
                        .next()
                        .ok_or_else(|| ParseError::MissingArgument("limit".to_string()))?;
                    self.limit = value
                        .parse::<u8>()
                        .map_err(|_| ParseError::MissingArgument("limit must be 1–250".to_string()))?;
                }
                "--from" => {
                    let value = peekable
                        .next()
                        .ok_or_else(|| ParseError::MissingArgument("from".to_string()))?;
                    self.from = value
                        .parse::<u32>()
                        .map_err(|_| ParseError::MissingArgument("from must be a non-negative integer".to_string()))?;
                }
                query => {
                    self.query = query.to_string();
                }
            }
        }

        if self.query.is_empty() {
            return Err(ParseError::MissingArgument("query".to_string()));
        }

        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let client = reqwest::Client::new();
        let result = HTTPRequest::search(client, &self.query, self.limit, self.from).await?;

        if result.objects.is_empty() {
            println!("{}", style(format!("No packages found matching '{}'.", self.query)).yellow());
            return Ok(());
        }

        for obj in &result.objects {
            let pkg = &obj.package;
            let description = pkg.description.as_deref().unwrap_or("");
            if description.is_empty() {
                println!(
                    "{}  {}{}",
                    style("•").dim(),
                    style(&pkg.name).bold(),
                    style(format!("@{}", pkg.version)).dim()
                );
            } else {
                println!(
                    "{}  {}{} — {}",
                    style("•").dim(),
                    style(&pkg.name).bold(),
                    style(format!("@{}", pkg.version)).dim(),
                    description
                );
            }
        }

        let showing = result.objects.len();
        println!(
            "\n{}",
            style(format!(
                "Showing {}-{} of {} result{}.",
                self.from + 1,
                self.from as usize + showing,
                result.total,
                if result.total == 1 { "" } else { "s" }
            ))
            .dim()
        );

        Ok(())
    }
}
