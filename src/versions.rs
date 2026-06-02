use std::collections::HashMap;

use semver::{Op, VersionReq};

pub use semver::Version;

use crate::{
    errors::{CommandError, ParseError},
    types::VersionData,
};

pub const LATEST: &str = "latest";
pub const EMPTY_VERSION: Version = Version::new(0, 0, 0);

pub struct Versions;

impl Versions {
    pub fn parse_raw_package_details(filename: String) -> (String, String) {
        if filename.starts_with('@') {
            // Scoped package: "@scope/name@version" — find the '@' after the '/'
            if let Some(slash_pos) = filename.find('/') {
                let after_slash = &filename[slash_pos + 1..];
                if let Some(at_pos) = after_slash.find('@') {
                    return (
                        filename[..slash_pos + 1 + at_pos].to_string(),
                        after_slash[at_pos + 1..].to_string(),
                    );
                }
            }
            return (filename, String::new());
        }
        match filename.split_once('@') {
            Some((name, version)) => (name.to_string(), version.to_string()),
            None => (filename, String::new()),
        }
    }

    fn validate_package_name(name: &str) -> Result<(), ParseError> {
        if name.contains("..") || name.contains('\\') || std::path::Path::new(name).is_absolute() {
            return Err(ParseError::InvalidPackageName(name.to_string()));
        }
        Ok(())
    }

    pub fn parse_semantic_package_details(
        package_details: String,
    ) -> Result<(String, Option<VersionReq>), ParseError> {
        if package_details.starts_with('@') {
            // Scoped package: "@scope/name" or "@scope/name@version"
            if let Some(slash_pos) = package_details.find('/') {
                let after_slash = &package_details[slash_pos + 1..];
                if let Some(at_pos) = after_slash.find('@') {
                    let name = package_details[..slash_pos + 1 + at_pos].to_string();
                    Self::validate_package_name(&name)?;
                    let version_str = &after_slash[at_pos + 1..];
                    if version_str == LATEST {
                        return Ok((name, None));
                    }
                    let req = Self::parse_npm_version_req(version_str)?;
                    return Ok((name, Some(req)));
                }
            }
            Self::validate_package_name(&package_details)?;
            return Ok((package_details, None));
        }

        match package_details.split_once('@') {
            None => {
                Self::validate_package_name(&package_details)?;
                Ok((package_details, None))
            }
            Some((name, version_str)) => {
                Self::validate_package_name(name)?;
                if version_str == LATEST {
                    return Ok((name.to_string(), None));
                }
                let req = Self::parse_npm_version_req(version_str)?;
                Ok((name.to_string(), Some(req)))
            }
        }
    }

    pub fn parse_semantic_version(version: &str) -> Result<VersionReq, ParseError> {
        Self::parse_npm_version_req(version).or_else(|_| Ok(VersionReq::STAR))
    }

    fn parse_npm_version_req(version: &str) -> Result<VersionReq, ParseError> {
        let v = version.trim();

        if v.is_empty() || v == "*" || v == "x" || v == "X" || v == "latest" {
            return Ok(VersionReq::STAR);
        }

        // Strip leading `v` or `V` (e.g. "v1.0.0")
        let v = v
            .strip_prefix('v')
            .or_else(|| v.strip_prefix('V'))
            .unwrap_or(v);

        // Drop any trailing junk after a space that looks like a secondary bound
        // e.g. ">=1.0.0 <2.0.0" is valid, but some packages emit ">=1.0.0 <2" etc.
        // semver's VersionReq handles space-separated bounds natively; just normalise
        // npm x-ranges.
        let normalized = v.replace(".x", ".*").replace(".X", ".*");

        VersionReq::parse(&normalized).map_err(ParseError::InvalidVersionNotation)
    }

    /// Resolves a `VersionReq` to a concrete version string when possible.
    /// - `None`  → `Some("latest")` (no constraint means latest)
    /// - Single exact comparator → `Some("major.minor.patch")`
    /// - Any range → `None` (must be resolved against available versions)
    pub fn resolve_full_version(semantic_version: Option<&VersionReq>) -> Option<String> {
        match semantic_version {
            None => Some(LATEST.to_string()),
            Some(req) if req.comparators.len() == 1 && req.comparators[0].op == Op::Exact => {
                let comp = &req.comparators[0];
                let minor = comp.minor.unwrap_or(0);
                let patch = comp.patch.unwrap_or(0);
                Some(format!("{}.{}.{}", comp.major, minor, patch))
            }
            _ => None,
        }
    }

    /// Finds the highest version in `versions` that satisfies `semantic_version`.
    pub fn resolve_partial_version(
        semantic_version: Option<&VersionReq>,
        versions: &HashMap<String, VersionData>,
    ) -> Result<String, CommandError> {
        let mut matching: Vec<Version> = versions
            .keys()
            .filter_map(|v| Version::parse(v).ok())
            .filter(|v| semantic_version.is_none_or(|req| req.matches(v)))
            .collect();

        matching.sort();
        matching
            .into_iter()
            .last()
            .map(|v| v.to_string())
            .ok_or(CommandError::InvalidVersion)
    }

    /// Formats a package identifier as "name@version".
    pub fn stringify(name: &String, version: &String) -> String {
        format!("{}@{}", name, version)
    }

    /// Returns `true` if `version` is the "latest" sentinel value.
    pub fn is_latest(version: Option<&String>) -> bool {
        version.is_some_and(|v| v == LATEST)
    }
}
