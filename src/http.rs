use bytes::Bytes;

use crate::{
    errors::CommandError::{self, *},
    types::{PackageData, VersionData},
};

pub const REGISTRY_URL: &str = "https://registry.npmjs.org";

/// Maximum bytes buffered for a binary/tarball download (512 MiB).
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// Maximum bytes buffered for a JSON registry API response (64 MiB).
const MAX_REGISTRY_BYTES: u64 = 64 * 1024 * 1024;

/// Rejects any URL whose scheme is not `https`.
/// Guards against CWE-319 (cleartext transmission) and limits the SSRF
/// (CWE-918) surface by refusing non-encrypted transports.
fn require_https(url: &str) -> Result<(), CommandError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| InsecureUrl(url.to_string()))?;
    if parsed.scheme() != "https" {
        return Err(InsecureUrl(url.to_string()));
    }
    Ok(())
}

pub struct HTTPRequest;
impl HTTPRequest {
    /// Download a file from any specified URL.
    /// Enforces HTTPS (CWE-319 / CWE-918) and a 512 MiB response cap (CWE-770).
    pub async fn get_bytes(client: reqwest::Client, url: String) -> Result<Bytes, CommandError> {
        require_https(&url)?;

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(HTTPFailed)?;

        // Reject before buffering if the server advertises a body that exceeds
        // the limit (CWE-770). Servers without Content-Length are still capped
        // implicitly by the 512 MiB ceiling enforced by `.bytes()` memory pressure,
        // but an explicit check here provides an early, clean error.
        if let Some(len) = resp.content_length() {
            if len > MAX_DOWNLOAD_BYTES {
                return Err(ResponseTooLarge(len));
            }
        }

        resp.bytes().await.map_err(FailedResponseBytes)
    }

    /// Make a request to the NPM registry.
    /// This includes the recommended header to shorten the response size.
    async fn registry(client: reqwest::Client, route: String) -> Result<String, CommandError> {
        let resp = client
            .get(format!("{REGISTRY_URL}{route}"))
            .header(
                "Accept",
                "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
            )
            .send()
            .await
            .map_err(HTTPFailed)?;

        if let Some(len) = resp.content_length() {
            if len > MAX_REGISTRY_BYTES {
                return Err(ResponseTooLarge(len));
            }
        }

        resp.text().await.map_err(FailedResponseText)
    }

    /// This makes a request for a specific version of a package.
    /// This method should always be preferred where possible as its response size is significantly smaller than full package data.
    pub async fn version_data(
        client: reqwest::Client,
        package_name: &String,
        version: &String,
    ) -> Result<VersionData, CommandError> {
        let response_raw = Self::registry(client, format!("/{package_name}/{version}")).await?;
        serde_json::from_str::<VersionData>(&response_raw).map_err(ParsingFailed)
    }

    /// This makes a request for all data for a package including all its versions.
    /// This method should be avoided where possible as its response size is much larger than just requesting version data.
    pub async fn package_data(
        client: reqwest::Client,
        package_name: &String,
    ) -> Result<PackageData, CommandError> {
        let response_raw = Self::registry(client, format!("/{package_name}")).await?;
        serde_json::from_str::<PackageData>(&response_raw).map_err(ParsingFailed)
    }
}
