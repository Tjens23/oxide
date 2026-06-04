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

use commands::command_handler;

fn main() {
    // Synchronous pre-flight: if this is a bare `oxide install` and nothing
    // has changed, exit before touching the keyring or the tokio runtime.
    if try_install_noop_fastpath() {
        println!("Already up to date.");
        return;
    }

    init_keyring();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Fatal: could not start async runtime: {e}");
            std::process::exit(1);
        }
    };

    runtime.block_on(async {
        let parse_result = command_handler::handle_args(env::args()).await;
        if let Err(err) = parse_result {
            println!("Failed to parse command: {err}");
        }
    });
}

/// Fast synchronous check: if this is a bare `oxide install` invocation and
/// nothing has changed (package.json fingerprint + oxide.lock mtime both match
/// the stored state AND node_modules exists), returns `true` so main can exit
/// immediately — before starting tokio, the keyring, or any async code.
fn try_install_noop_fastpath() -> bool {
    use std::time::UNIX_EPOCH;

    // Only applies to an unqualified `oxide install` with no extra flags or package names.
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 || args[1] != "install" {
        return false;
    }

    let Ok(pkgjson_meta) = std::fs::metadata("./package.json") else {
        return false;
    };
    let pkgjson_mtime = pkgjson_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let fingerprint = format!("{}-{}", pkgjson_meta.len(), pkgjson_mtime);

    let Ok(state_raw) = std::fs::read_to_string(constants::OXIDE_STATE_FILE) else {
        return false;
    };
    let Ok(state) = serde_json::from_str::<serde_json::Value>(&state_raw) else {
        return false;
    };

    if state
        .get("pkgjson_fingerprint")
        .and_then(|v| v.as_str())
        != Some(fingerprint.as_str())
    {
        return false;
    }

    let Some(stored_lock_mtime) = state.get("lockfile_mtime").and_then(|v| v.as_u64()) else {
        return false;
    };
    let current_lock_mtime = std::fs::metadata(constants::OXIDE_LOCK)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64);
    if current_lock_mtime != Some(stored_lock_mtime) {
        return false;
    }

    std::path::Path::new(constants::NODE_MODULES).exists()
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
