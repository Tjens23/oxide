use crate::errors::CommandError;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bytes::Bytes;
use flate2::bufread::GzDecoder;
use lazy_static::lazy_static;
use sha2::{Digest, Sha256, Sha512};
use std::{
    future::Future,
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Condvar, Mutex,
    },
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

/// Create a file symlink. On Windows, requires Developer Mode or elevated
/// privileges — callers should surface a helpful error when it fails.
pub fn create_file_link(src: &Path, dest: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(src, dest)
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(src, dest)
    }
}

/// Returns `true` if `s` contains no path traversal, root, or drive-prefix components.
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


/// Extracts a `.tar.gz` tarball to `dest`, storing each unique file in the
/// content-addressed `file_store_dir` (sharded by the first 2 hex chars of its
/// SHA-256 digest) and creating a **hardlink** from the store to the
/// destination instead of copying bytes.  Falls back to writing the bytes
/// directly when the hardlink crosses device boundaries (EXDEV / `ErrorKind::
/// CrossesDevices`) or the store write failed for any reason.
///
/// The `package/` prefix is stripped (npm tarball convention) and zip-slip
/// protection is applied identically to [`extract_tarball_strip`].
pub fn extract_tarball_hardlinked(
    bytes: Bytes,
    dest: &Path,
    file_store_dir: &Path,
) -> Result<(), CommandError> {
    let bytes_slice = bytes.as_ref();
    let gz = GzDecoder::new(bytes_slice);
    let mut archive = Archive::new(gz);

    for entry in archive.entries().map_err(CommandError::ExtractionFailed)? {
        let mut entry = entry.map_err(CommandError::ExtractionFailed)?;
        let path = entry
            .path()
            .map_err(CommandError::ExtractionFailed)?
            .into_owned();

        let safe_stripped: std::path::PathBuf = path
            .components()
            .skip(1)
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .collect();
        if safe_stripped.as_os_str().is_empty() {
            continue;
        }

        let out = dest.join(&safe_stripped);

        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out).map_err(CommandError::ExtractionFailed)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(CommandError::ExtractionFailed)?;
            }

            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(CommandError::ExtractionFailed)?;

            let hex: String = Sha256::digest(&content)
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            let store_path = {
                let (prefix, rest) = hex.split_at(2.min(hex.len()));
                file_store_dir.join(prefix).join(rest)
            };

            if !store_path.exists() {
                if let Some(p) = store_path.parent() {
                    let _ = std::fs::create_dir_all(p);
                }
                let _ = std::fs::write(&store_path, &content);
            }

            // Try hardlink; fall back to a plain write on any error
            // (cross-device, store write failure, permissions, …).
            if store_path.exists() {
                match std::fs::hard_link(&store_path, &out) {
                    Ok(_) => {}
                    Err(_) => {
                        std::fs::write(&out, &content)
                            .map_err(CommandError::ExtractionFailed)?;
                    }
                }
            } else {
                std::fs::write(&out, &content).map_err(CommandError::ExtractionFailed)?;
            }
        }
    }

    Ok(())
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

lazy_static! {
    static ref DONE_SIGNAL: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
}

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
        if Self::task_count() == 0 {
            return;
        }
        let (lock, cvar) = &*DONE_SIGNAL;
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        drop(cvar.wait_while(guard, |_| Self::task_count() != 0));
    }

    fn increment_tasks() {
        ACTIVE_TASKS.fetch_add(1, Ordering::SeqCst);
    }

    fn decrement_tasks() {
        let prev = ACTIVE_TASKS.fetch_sub(1, Ordering::SeqCst);
        if prev == 1 {
            let (lock, cvar) = &*DONE_SIGNAL;
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            cvar.notify_all();
        }
    }

    fn task_count() -> usize {
        ACTIVE_TASKS.load(Ordering::SeqCst)
    }
}
