use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{cache::CACHE_DIRECTORY, errors::CommandError};

pub const GITHUB_API_LATEST: &str =
    "https://api.github.com/repos/tjens23/oxide/releases/latest";

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

const UPDATE_CHECK_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Serialize, Deserialize)]
struct UpdateCheckCache {
    last_checked: u64,
    latest_version: String,
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
}

fn cache_path() -> String {
    format!("{}/update-check.json", *CACHE_DIRECTORY)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn write_cache(version: &str) {
    let cache = UpdateCheckCache {
        last_checked: now_secs(),
        latest_version: version.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = std::fs::write(cache_path(), json);
    }
}

fn is_newer(latest: &str) -> Option<String> {
    let current = semver::Version::parse(CURRENT_VERSION).ok()?;
    let latest_v = semver::Version::parse(latest).ok()?;
    if latest_v > current {
        Some(latest.to_string())
    } else {
        None
    }
}

/// Fetches the latest release tag from GitHub, returning the version string
/// without a leading `v`.
pub async fn fetch_latest_version(client: &reqwest::Client) -> Result<String, CommandError> {
    let release: LatestRelease = client
        .get(GITHUB_API_LATEST)
        .header("User-Agent", "oxide-update-check")
        .send()
        .await
        .map_err(CommandError::HTTPFailed)?
        .json()
        .await
        .map_err(CommandError::HTTPFailed)?;

    Ok(release.tag_name.trim_start_matches('v').to_string())
}

/// Returns the latest version string if it is strictly newer than the running
/// binary, using a 24-hour on-disk cache.
///
/// When the cache is missing or stale, a background task is spawned to refresh
/// it; the notification will appear on the next invocation. This keeps the hot
/// path allocation- and latency-free.
pub fn check_for_update_cached() -> Option<String> {
    let path = cache_path();
    let now = now_secs();

    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(cache) = serde_json::from_str::<UpdateCheckCache>(&raw) {
            if now.saturating_sub(cache.last_checked) < UPDATE_CHECK_TTL_SECS {
                return is_newer(&cache.latest_version);
            }
        }
    }

    // Cache is missing or stale — refresh in the background so this call
    // returns immediately and imposes no latency on the current command.
    tokio::spawn(async move {
        let Ok(client) = reqwest::Client::builder()
            .user_agent("oxide-update-check")
            .timeout(Duration::from_secs(5))
            .build()
        else {
            return;
        };
        if let Ok(ver) = fetch_latest_version(&client).await {
            write_cache(&ver);
        }
    });

    None
}

/// Fetches the latest version unconditionally and updates the cache.
/// Used by `oxide -v` / `oxide --version` where the user explicitly wants
/// up-to-date information.
pub async fn fetch_latest_version_fresh() -> Option<String> {
    let client = reqwest::Client::builder()
        .user_agent("oxide-update-check")
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let ver = fetch_latest_version(&client).await.ok()?;
    write_cache(&ver);
    Some(ver)
}
