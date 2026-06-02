mod cache;
mod commands;
mod config;
mod constants;
mod errors;
mod http;
mod installer;
mod types;
mod util;
mod versions;
mod workspace;

#[cfg(test)]
#[path = "tests/test.rs"]
mod tests;

use std::env;

use colored::Colorize;
use commands::command_handler;

fn main() {
    init_keyring();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{} could not start async runtime: {e}", "Fatal:".red().bold());
            std::process::exit(1);
        }
    };

    runtime.block_on(async {
        let parse_result = command_handler::handle_args(env::args()).await;
        if let Err(err) = parse_result {
            eprintln!("{} {err}", "error:".red().bold());
        }
    });
}

fn init_keyring() {
    #[cfg(windows)]
    match windows_native_keyring_store::Store::new() {
        Ok(store) => keyring_core::set_default_store(store),
        Err(e) => eprintln!("{} could not open Windows Credential Store: {e}", "Warning:".yellow()),
    }
    #[cfg(target_os = "macos")]
    match apple_native_keyring_store::keychain::Store::new() {
        Ok(store) => keyring_core::set_default_store(store),
        Err(e) => eprintln!("{} could not open macOS Keychain: {e}", "Warning:".yellow()),
    }
    #[cfg(target_os = "linux")]
    if let Ok(store) = zbus_secret_service_keyring_store::Store::new() {
        keyring_core::set_default_store(store);
    }
}
