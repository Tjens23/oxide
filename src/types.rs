use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::util;
#[derive(Debug, Deserialize)]
pub struct VersionData {
    pub name: String,
    pub version: String,
    pub dependencies: Option<HashMap<String, String>>,
    pub dist: Dist,
}

#[derive(Debug, Deserialize)]
pub struct Dist {
    pub tarball: String,
    pub integrity: Option<String>,
    pub shasum: Option<String>,
}

impl Dist {
    pub fn verify(&self, bytes: &bytes::Bytes) -> bool {
        if let Some(ref integrity) = self.integrity {
            return util::verify_integrity(bytes, integrity);
        }
        if self.shasum.is_some() {
          
            eprintln!(
                "Security warning: package provides only a SHA-1 checksum (no sha512 integrity \
                 field). Installation refused to avoid accepting a potentially tampered package."
            );
            return false;
        }
       eprintln!(
            "Security warning: package provides no integrity data. \
             Installation refused."
        );
        false
    }
}

#[derive(Deserialize)]
pub struct PackageData {
    pub versions: HashMap<String, VersionData>,
}

#[derive(Serialize, Deserialize)]
pub struct PackageLock {
    #[serde(rename = "isLatest")]
    pub is_latest: bool,
    pub dependencies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
}

impl PackageLock {
    pub fn new(is_latest: bool) -> Self {
        Self {
            is_latest,
            dependencies: Vec::new(),
            integrity: None,
        }
    }
}

pub type DependencyMap = HashMap<String, PackageLock>;


#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub objects: Vec<SearchObject>,
    pub total: u64,
}

#[derive(Debug, Deserialize)]
pub struct SearchObject {
    pub package: SearchPackage,
}

#[derive(Debug, Deserialize)]
pub struct SearchPackage {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}
