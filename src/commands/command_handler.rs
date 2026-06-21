use async_trait::async_trait;
use console::style;
use std::env::Args;

use crate::errors::{
    CommandError,
    ParseError::{self, CommandNotFound},
};

use super::config::ConfigHandler;
use super::dlx::DlxHandler;
use super::doctor::DoctorHandler;
use super::exec::ExecHandler;
use super::foreach::ForeachHandler;
use super::init::InitHandler;
use super::install::InstallHandler;
use super::link::LinkHandler;
use super::ls::LsHandler;
use super::outdated::OutdatedHandler;
use super::pack::PackHandler;
use super::publish::PublishHandler;
use super::run::RunHandler;
use super::self_upgrade::SelfUpgradeHandler;
use super::uninstall::UninstallHandler;
use super::unlink::UnlinkHandler;
use super::upgrade::UpgradeHandler;
use super::search::SearchHandler;
use super::version::VersionHandler;
use super::why::WhyHandler;
use super::workspaces::WorkspacesHandler;

#[async_trait]
pub trait CommandHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError>;
    async fn execute(&self) -> Result<(), CommandError>;
}

pub async fn handle_args(mut args: Args) -> Result<(), ParseError> {
    args.next();

    let command = match args.next() {
        Some(c) => c,
        None => {
            print_global_help();
            return Ok(());
        }
    };

    if command == "-v" || command == "--version" {
        print_version().await;
        return Ok(());
    }

    if command.eq_ignore_ascii_case("help") || command == "--help" || command == "-h" {
        match args.next().as_deref() {
            Some(sub) => print_command_help(sub),
            None => print_global_help(),
        }
        return Ok(());
    }

    let remaining: Vec<String> = args.collect();
    if remaining.iter().any(|a| a == "--help" || a == "-h") {
        print_command_help(&command);
        return Ok(());
    }

    let mut command_handler: Box<dyn CommandHandler> = match command.to_lowercase().as_str() {
        "login" => Box::new(super::login::LoginHandler::default()),
        "config" => Box::new(ConfigHandler::default()),
        "uninstall" => Box::new(UninstallHandler::default()),
        "upgrade" => Box::new(UpgradeHandler::default()),
        "self-upgrade" => Box::new(SelfUpgradeHandler),
        "init" => Box::new(InitHandler),
        "install" => Box::new(InstallHandler::default()),
        "link" | "ln" => Box::new(LinkHandler::default()),
        "unlink" => Box::new(UnlinkHandler::default()),
        "publish" => Box::new(PublishHandler::default()),
        "exec" | "x" => Box::new(ExecHandler::default()),
        "run" => Box::new(RunHandler::default()),
        "version" => Box::new(VersionHandler::default()),
        "outdated" => Box::new(OutdatedHandler),
        "why" => Box::new(WhyHandler::default()),
        "ls" | "list" => Box::new(LsHandler::default()),
        "pack" => Box::new(PackHandler::default()),
        "doctor" | "check" => Box::new(DoctorHandler),
        "dlx" | "bunx" => Box::new(DlxHandler::default()),
        "workspaces" | "ws" => Box::new(WorkspacesHandler::default()),
        "foreach" | "each" => Box::new(ForeachHandler::default()),
        "search" => Box::new(SearchHandler::default()),
        _ => return Err(CommandNotFound(command.to_string())),
    };

    command_handler.parse(&mut remaining.into_iter())?;
    if let Err(e) = command_handler.execute().await {
        eprintln!("{} {e}", style("Command error:").red().bold());
    }

    if let Some(newer) = crate::update_check::check_for_update_cached() {
        eprintln!(
            "{} Run 'oxide self-upgrade' to update.",
            style(format!("A new version of oxide is available: v{}.", newer))
                .yellow()
                .bold()
        );
    }

    Ok(())
}

async fn print_version() {
    println!("oxide v{}", crate::update_check::CURRENT_VERSION);
    match crate::update_check::fetch_latest_version_fresh().await {
        Some(latest) => {
            if latest != crate::update_check::CURRENT_VERSION {
                println!(
                    "Latest: v{} — run 'oxide self-upgrade' to update.",
                    latest
                );
            } else {
                println!("Latest: v{} (up to date)", latest);
            }
        }
        None => {
            println!("Could not fetch latest version (offline?).");
        }
    }
}

fn print_global_help() {
    println!("Usage: oxide <command> [flags]\n");
    for (name, description) in [
        ("install", "Install a package"),
        ("uninstall", "Uninstall a package"),
        ("upgrade", "Upgrade a package"),
        ("self-upgrade", "Upgrade the oxide tool itself"),
        ("exec", "Execute a binary from node_modules/.bin"),
        ("run", "Run a defined package script"),
        (
            "version",
            "Bump package.json version, commit, and create a git tag",
        ),
        ("init", "Initialize a new project with a package.json"),
        ("link", "Link a package globally or into node_modules"),
        ("unlink", "Remove a linked package"),
        ("publish", "Publish a package to the npm registry"),
        ("login", "Authenticate with the npm registry"),
        ("config", "Read and write persistent configuration"),
        ("outdated", "List dependencies with available updates"),
        ("why", "Explain why a package is installed"),
        ("ls", "List installed packages"),
        ("pack", "Create a publishable .tgz tarball locally"),
        ("doctor", "Check project health and environment"),
        (
            "dlx",
            "Fetch and run a package binary without installing it",
        ),
        (
            "workspaces",
            "List workspace packages defined in this monorepo",
        ),
        ("foreach", "Run a script across workspace packages"),
        ("search", "Search for packages on the npm registry"),
        ("help", "Show help for oxide or a specific command"),
    ] {
        println!("  {:<14} {}", name, description);
    }
    println!(
        "\nRun 'oxide help <command>' or 'oxide <command> --help' for command-specific flags."
    );
}

fn print_command_help(command: &str) {
    match command.to_lowercase().as_str() {
        "install" => print_help(
            "install [<package>[@<version>]] [flags]",
            "Install a package or all dependencies listed in package.json.",
            &[
                (
                    "-g, --global",
                    "Install the package globally instead of locally",
                ),
                ("-D, --save-dev", "Save the package in devDependencies"),
                ("--no-save", "Do not update package.json after installing"),
                (
                    "--ignore-scripts",
                    "Skip lifecycle scripts (forward-looking; scripts are not run by oxide today)",
                ),
                (
                    "-F, --filter <pat>",
                    "Install across workspace packages matching the pattern",
                ),
                ("--help, -h", "Show this help"),
            ],
        ),
        "uninstall" => print_help(
            "uninstall <package> [flags]",
            "Remove a package from node_modules and package.json.",
            &[("--help, -h", "Show this help")],
        ),
        "upgrade" => print_help(
            "upgrade [<package>[@<version>]] [flags]",
            "Upgrade an installed package to the latest or specified version.",
            &[("--help, -h", "Show this help")],
        ),
        "self-upgrade" => print_help(
            "self-upgrade",
            "Upgrade the oxide binary itself to the latest release.",
            &[("--help, -h", "Show this help")],
        ),
        "exec" | "x" => print_help(
            "exec <binary> [args...]",
            "Execute a binary from the local node_modules/.bin directory.",
            &[("--help, -h", "Show this help")],
        ),
        "run" => print_help(
            "run <script> [flags]",
            "Run a script defined in package.json.",
            &[
                ("-r, --recursive", "Run across all workspace packages"),
                (
                    "-F, --filter <pat>",
                    "Run across workspace packages matching the pattern",
                ),
                ("--help, -h", "Show this help"),
            ],
        ),
        "version" => print_help(
            "version <major|minor|patch|<semver>>",
            "Bump the package.json version, create a git commit, and tag it.",
            &[("--help, -h", "Show this help")],
        ),
        "init" => print_help(
            "init [flags]",
            "Interactively create a new package.json in the current directory.",
            &[("--help, -h", "Show this help")],
        ),
        "link" | "ln" => print_help(
            "link [<package>] [flags]",
            "Link the current package globally, or link a global package into node_modules.",
            &[("--help, -h", "Show this help")],
        ),
        "unlink" => print_help(
            "unlink [<package>]",
            "Remove a linked package from node_modules or the global links directory.",
            &[("--help, -h", "Show this help")],
        ),
        "publish" => print_help(
            "publish [flags]",
            "Pack and publish the current package to the npm registry.",
            &[
                (
                    "--otp <code>",
                    "One-time password for two-factor authentication",
                ),
                ("--help, -h", "Show this help"),
            ],
        ),
        "login" => print_help(
            "login [flags]",
            "Authenticate with the npm registry.",
            &[
                (
                    "--otp <code>",
                    "One-time password for two-factor authentication",
                ),
                ("--help, -h", "Show this help"),
            ],
        ),
        "config" => print_help(
            "config <set|get|list|delete> [<key>] [<value>]",
            "Read and write persistent oxide configuration stored in {config_dir}/oxide/config.json.",
            &[
                ("set <key> <value>", "Set a configuration key"),
                ("get <key>", "Print the value of a key"),
                (
                    "list",
                    "List all keys, their descriptions, and current values",
                ),
                ("delete <key>", "Remove a key from the config"),
                ("--help, -h", "Show this help"),
            ],
        ),
        "outdated" => print_help(
            "outdated",
            "List packages that have newer versions available on the registry.",
            &[("--help, -h", "Show this help")],
        ),
        "why" => print_help(
            "why <package>",
            "Explain why a package is present in the dependency tree.",
            &[("--help, -h", "Show this help")],
        ),
        "ls" | "list" => print_help(
            "ls [flags]",
            "List installed packages.",
            &[
                ("--dev, -D", "Show development dependencies"),
                ("--all, -a", "Show all dependencies including transitive"),
                ("--help, -h", "Show this help"),
            ],
        ),
        "pack" => print_help(
            "pack [flags]",
            "Create a .tgz tarball of the package suitable for publishing.",
            &[
                (
                    "--out-dir <dir>",
                    "Directory to write the tarball into (default: current dir)",
                ),
                ("--help, -h", "Show this help"),
            ],
        ),
        "doctor" | "check" => print_help(
            "doctor",
            "Check the project and environment for common issues.",
            &[("--help, -h", "Show this help")],
        ),
        "dlx" | "bunx" => print_help(
            "dlx <package>[@<version>] [--] [args...]",
            "Fetch and run a package binary in a temporary environment without installing it.",
            &[("--help, -h", "Show this help")],
        ),
        "workspaces" | "ws" => print_help(
            "workspaces [flags]",
            "List workspace packages defined in this monorepo.",
            &[
                ("-F, --filter <pat>", "Filter packages by name or path"),
                ("--help, -h", "Show this help"),
            ],
        ),
        "foreach" | "each" => print_help(
            "foreach <script> [flags]",
            "Run a script across all (or filtered) workspace packages.",
            &[
                (
                    "-F, --filter <pat>",
                    "Only run in packages matching the pattern",
                ),
                ("--bail", "Stop on first failure"),
                ("--help, -h", "Show this help"),
            ],
        ),
        "search" => print_help(
            "search <query> [flags]",
            "Search for packages on the npm registry.",
            &[
                ("-n, --limit <n>", "Number of results to return (default 20, max 250)"),
                ("--from <offset>", "Offset for paginating results (default 0)"),
                ("--help, -h", "Show this help"),
            ],
        ),
        "help" => print_global_help(),
        other => {
            println!(
                "No help entry for '{}'. Run 'oxide help' to see all commands.",
                other
            );
        }
    }
}

fn print_help(usage: &str, description: &str, flags: &[(&str, &str)]) {
    println!("Usage: oxide {}\n", usage);
    println!("{}\n", description);
    if !flags.is_empty() {
        println!("Flags:");
        for (flag, desc) in flags {
            println!("  {:<26} {}", flag, desc);
        }
    }
}
