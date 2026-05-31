use crate::errors::CommandError;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bytes::Bytes;
use flate2::bufread::GzDecoder;
use sha2::{Digest, Sha512};
use std::{
    future::Future,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    thread::{self},
    time::Duration,
};
use tar::Archive;
use tokio::task::JoinHandle;

pub fn verify_integrity(bytes: &Bytes, integrity: &str) -> bool {
    if let Some(encoded) = integrity.strip_prefix("sha512-") {
        let digest = Sha512::digest(bytes);
        return BASE64.encode(digest.as_slice()) == encoded;
    }
    false
}

pub fn verify_shasum(bytes: &Bytes, expected_hex: &str) -> bool {
    use sha1::Digest;
    let digest = sha1::Sha1::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex == expected_hex
}

pub fn create_dir_link(src: &Path, dest: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        junction::create(src, dest)
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(src, dest)
    }
}

/// Returns `true` if `s` is safe to use as a component in a cache or
/// `node_modules` path.  Rejects traversal sequences (`..`), absolute
/// paths, Windows drive prefixes, and null bytes.
pub fn is_safe_path_component(s: &str) -> bool {
    if s.is_empty() || s.contains('\0') {
        return false;
    }
    for component in std::path::Path::new(s).components() {
        match component {
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return false,
            _ => {}
        }
    }
    true
}

pub fn extract_tarball(bytes: Bytes, dest: &Path) -> Result<(), CommandError> {
    let bytes = &bytes.to_vec()[..];
    let gz = GzDecoder::new(bytes);
    let mut archive = Archive::new(gz);

    archive.unpack(dest).map_err(CommandError::ExtractionFailed)
}

pub fn extract_tarball_strip(bytes: Bytes, dest: &str) -> Result<(), CommandError> {
    use std::path::PathBuf;

    let bytes_slice = bytes.as_ref();
    let gz = GzDecoder::new(bytes_slice);
    let mut archive = Archive::new(gz);

    for entry in archive.entries().map_err(CommandError::ExtractionFailed)? {
        let mut entry = entry.map_err(CommandError::ExtractionFailed)?;
        let path = entry
            .path()
            .map_err(CommandError::ExtractionFailed)?
            .into_owned();

        // Drop the leading "package" component and reject any non-normal path
        // components to prevent zip-slip attacks (e.g. `../../etc/passwd`).
        let safe_stripped: PathBuf = path
            .components()
            .skip(1)
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .collect();
        if safe_stripped.as_os_str().is_empty() {
            continue;
        }

        let out = std::path::Path::new(dest).join(&safe_stripped);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out).map_err(CommandError::ExtractionFailed)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(CommandError::ExtractionFailed)?;
            }
            entry.unpack(&out).map_err(CommandError::ExtractionFailed)?;
        }
    }

    Ok(())
}

pub static ACTIVE_TASKS: AtomicUsize = AtomicUsize::new(0);

pub struct TaskAllocator;

impl TaskAllocator {
    pub fn add_task<T>(future: T) -> JoinHandle<T::Output>
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        Self::increment_tasks();
        tokio::spawn(async move {
            let future_result = future.await;
            Self::decrement_tasks();

            future_result
        })
    }

    pub fn add_blocking<F, R>(f: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        Self::increment_tasks();
        tokio::task::spawn_blocking(move || {
            let task_result = f();
            Self::decrement_tasks();

            task_result
        })
    }

    pub fn block_until_done() {
        while Self::task_count() != 0 {
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn increment_tasks() {
        ACTIVE_TASKS.fetch_add(1, Ordering::SeqCst);
    }

    fn decrement_tasks() {
        ACTIVE_TASKS.fetch_sub(1, Ordering::SeqCst);
    }

    fn task_count() -> usize {
        ACTIVE_TASKS.load(Ordering::SeqCst)
    }
}
