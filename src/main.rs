mod cache;
mod commands;
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

use commands::command_handler;

fn main() {
    init_keyring();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build Tokio runtime")
        .block_on(async {
            let parse_result = command_handler::handle_args(env::args()).await;
            if let Err(err) = parse_result {
                println!("Failed to parse command: {err}");
            }
        });
}

fn init_keyring() {
    #[cfg(windows)]
    match windows_native_keyring_store::Store::new() {
        Ok(store) => keyring_core::set_default_store(store),
        Err(e) => eprintln!("Warning: could not open Windows Credential Store: {e}"),
    }
    #[cfg(target_os = "macos")]
    match apple_native_keyring_store::keychain::Store::new() {
        Ok(store) => keyring_core::set_default_store(store),
        Err(e) => eprintln!("Warning: could not open macOS Keychain: {e}"),
    }
    #[cfg(target_os = "linux")]
    if let Ok(store) = zbus_secret_service_keyring_store::Store::new() {
        keyring_core::set_default_store(store);
    }
}