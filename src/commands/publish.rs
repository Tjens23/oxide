use std::path::{Path, PathBuf};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use flate2::{write::GzEncoder, Compression};
use serde_json::{json, Value};
use sha1::Digest as _;

use crate::{
    errors::{CommandError, ParseError},
    http::REGISTRY_URL,
};

const NPM_USER_AGENT: &str = "npm/10.9.2 node/v22.12.0 win32 x64 workspaces/false";

use super::command_handler::CommandHandler;
use super::login::load_token;

const ALWAYS_IGNORE: &[&str] = &[
    "node_modules",
    ".git",
    ".hg",
    ".svn",
    ".DS_Store",
    "npm-debug.log",
    ".npmrc",
    ".git",
    "CVS",
];

pub(crate) fn read_ignore_patterns(dir: &Path) -> Vec<String> {
    let npmignore = dir.join(".npmignore");
    let gitignore = dir.join(".gitignore");

    let file = if npmignore.exists() { npmignore } else { gitignore };

    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.trim().to_string())
        .collect()
}

pub(crate) fn is_ignored(rel: &str, user_patterns: &[String]) -> bool {
    let first = rel.split(['/', '\\']).next().unwrap_or("");

    if ALWAYS_IGNORE.iter().any(|p| first == *p) {
        return true;
    }

    // User patterns: simple prefix / exact match.
    for pattern in user_patterns {
        let p = pattern.trim_start_matches('/');
        if rel == p || rel.starts_with(&format!("{}/", p)) || first == p {
            return true;
        }
    }

    false
}

pub(crate) fn collect_files(dir: &Path, user_patterns: &[String]) -> Result<Vec<PathBuf>, CommandError> {
    let mut files = Vec::new();
    collect_recursive(dir, dir, user_patterns, &mut files)?;
    Ok(files)
}

fn collect_recursive(
    root: &Path,
    current: &Path,
    patterns: &[String],
    out: &mut Vec<PathBuf>,
) -> Result<(), CommandError> {
    for entry in std::fs::read_dir(current).map_err(CommandError::FailedToWriteFile)? {
        let entry = entry.map_err(CommandError::FailedDirectoryEntry)?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");

        if is_ignored(&rel, patterns) {
            continue;
        }

        if path.is_dir() {
            collect_recursive(root, &path, patterns, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn pack(dir: &Path) -> Result<Vec<u8>, CommandError> {
    let patterns = read_ignore_patterns(dir);
    let files = collect_files(dir, &patterns)?;

    let buf = Vec::new();
    let gz = GzEncoder::new(buf, Compression::default());
    let mut tar = tar::Builder::new(gz);

    for path in &files {
        let rel = path
            .strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let tar_path = format!("package/{}", rel);

        tar.append_path_with_name(path, &tar_path)
            .map_err(CommandError::ExtractionFailed)?;
    }

    let gz = tar.into_inner().map_err(CommandError::ExtractionFailed)?;
    gz.finish().map_err(CommandError::ExtractionFailed)
}


fn shasum_hex(data: &[u8]) -> String {
    let digest = sha1::Sha1::digest(data);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

fn integrity_sha512(data: &[u8]) -> String {
    let digest = sha2::Sha512::digest(data);
    format!("sha512-{}", BASE64.encode(digest.as_slice()))
}


async fn publish_tarball(
    client: &reqwest::Client,
    token: &str,
    pkg_json: &Value,
    tarball: Vec<u8>,
    tag: &str,
    access: Option<&str>,
    otp: Option<&str>,
    dry_run: bool,
) -> Result<(), CommandError> {
    let name = pkg_json["name"]
        .as_str()
        .ok_or_else(|| CommandError::FailedToWriteFile(std::io::Error::other("missing \"name\" in package.json")))?;
    let version = pkg_json["version"]
        .as_str()
        .ok_or_else(|| CommandError::FailedToWriteFile(std::io::Error::other("missing \"version\" in package.json")))?;

    let filename = format!("{}-{}.tgz", name.replace('/', "-").trim_start_matches('-'), version);
    let shasum = shasum_hex(&tarball);
    let integrity = integrity_sha512(&tarball);
    let name_encoded = name.replace('/', "%2F");
    let tarball_url = format!("{}/{}-/{}/-/{}", REGISTRY_URL, name_encoded, name_encoded, filename);
    let tarball_len = tarball.len();
    let tarball_b64 = BASE64.encode(&tarball);

    let mut version_manifest = pkg_json.clone();
    version_manifest["dist"] = json!({
        "shasum": shasum,
        "integrity": integrity,
        "tarball": tarball_url,
    });

    let mut body = json!({
        "_id": name,
        "name": name,
        "description": pkg_json["description"],
        "dist-tags": { tag: version },
        "versions": { version: version_manifest },
        "_attachments": {
            filename: {
                "content_type": "application/octet-stream",
                "data": tarball_b64,
                "length": tarball_len,
            }
        }
    });

    if let Some(acc) = access {
        body["access"] = json!(acc);
    }

    println!("Publishing {}@{} with tag '{}'…", name, version, tag);

    if dry_run {
        println!("[dry-run] Would PUT {}/{}", REGISTRY_URL, name);
        println!("[dry-run] Tarball size: {} bytes, shasum: {}", tarball_len, shasum);
        return Ok(());
    }

    let mut req = client
        .put(format!("{}/{}", REGISTRY_URL, name_encoded))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", NPM_USER_AGENT)
        .body(body.to_string());

    if let Some(code) = otp {
        req = req.header("npm-otp", code);
    }

    let resp = req.send().await.map_err(CommandError::HTTPFailed)?;
    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED {
        let www_auth = resp
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let is_otp = www_auth.contains("otp") || resp.headers().get("x-npm-otp").is_some();
        let resp_body = resp.text().await.map_err(CommandError::FailedResponseText)?;
        let body_wants_otp = resp_body.contains("one-time pass") || resp_body.contains("otp");
        if is_otp || body_wants_otp {
            return Err(CommandError::LoginFailed {
                status: 401,
                body: "registry requires a one-time password — re-run with: oxide publish --otp <code>".into(),
            });
        }
        return Err(CommandError::LoginFailed { status: status.as_u16(), body: resp_body });
    }

    let resp_body = resp.text().await.map_err(CommandError::FailedResponseText)?;

    if !status.is_success() {
        return Err(CommandError::LoginFailed {
            status: status.as_u16(),
            body: resp_body,
        });
    }

    println!("+ {}@{}", name, version);
    Ok(())
}


#[derive(Default)]
pub struct PublishHandler {
    tag: Option<String>,
    access: Option<String>,
    otp: Option<String>,
    dry_run: bool,
    dir: Option<String>,
}

#[async_trait]
impl CommandHandler for PublishHandler {
    fn parse(&mut self, args: &mut dyn Iterator<Item = String>) -> Result<(), ParseError> {
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--tag" => {
                    self.tag = Some(
                        args.next()
                            .ok_or_else(|| ParseError::MissingArgument("--tag <tag>".to_string()))?,
                    );
                }
                "--access" => {
                    self.access = Some(
                        args.next()
                            .ok_or_else(|| ParseError::MissingArgument("--access <public|restricted>".to_string()))?,
                    );
                }
                "--otp" => {
                    self.otp = Some(
                        args.next()
                            .ok_or_else(|| ParseError::MissingArgument("--otp <code>".to_string()))?,
                    );
                }
                "--dry-run" => self.dry_run = true,
                other if !other.starts_with('-') => self.dir = Some(other.to_string()),
                _ => {}
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let dir = self
            .dir
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().expect("cannot read cwd"));

        let dir = std::fs::canonicalize(&dir).map_err(CommandError::FailedToWriteFile)?;

        let pkg_raw = std::fs::read_to_string(dir.join("package.json"))
            .map_err(CommandError::FailedToWriteFile)?;
        let pkg_json: Value = serde_json::from_str(&pkg_raw).map_err(CommandError::ParsingFailed)?;

        if pkg_json["name"].as_str().is_none() {
            return Err(CommandError::FailedToWriteFile(std::io::Error::other(
                "package.json is missing \"name\"",
            )));
        }
        if pkg_json["version"].as_str().is_none() {
            return Err(CommandError::FailedToWriteFile(std::io::Error::other(
                "package.json is missing \"version\"",
            )));
        }

        let token = if self.dry_run {
            String::new()
        } else {
            load_token().ok_or_else(|| {
                CommandError::FailedToWriteFile(std::io::Error::other(
                    "not logged in — run `oxide login` first",
                ))
            })?
        };

        println!("Packing {}…", dir.display());
        let tarball = pack(&dir)?;

        let tag = self.tag.as_deref().unwrap_or("latest");
        let client = reqwest::Client::new();

        publish_tarball(
            &client,
            &token,
            &pkg_json,
            tarball,
            tag,
            self.access.as_deref(),
            self.otp.as_deref(),
            self.dry_run,
        )
        .await
    }
}
