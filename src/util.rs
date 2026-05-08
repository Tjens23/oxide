use std::{
    future::Future,
    sync::atomic::{self, AtomicUsize},
    thread::{self},
    time::Duration,
};

use atomic::Ordering::SeqCst;
use bytes::Bytes;
use flate2::bufread::GzDecoder;
use tar::Archive;
use tokio::task::JoinHandle;
use crate::errors::CommandError;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha512};

pub fn verify_integrity(bytes: &Bytes, integrity: &str) -> bool {
    if let Some(encoded) = integrity.strip_prefix("sha512-") {
        let digest = Sha512::digest(bytes);
        return BASE64.encode(digest.as_slice()) == encoded;
    }
    false
}

pub fn verify_shasum(bytes: &Bytes, expected_hex: &str) -> bool {
    use sha1::Digest as _;
    let digest = sha1::Sha1::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex == expected_hex
}

pub fn create_dir_link(src: &str, dest: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        junction::create(src, dest)
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(src, dest)
    }
}

pub fn extract_tarball(bytes: Bytes, dest: String) -> Result<(), CommandError> {
    let bytes = &bytes.to_vec()[..];
    let gz = GzDecoder::new(bytes);
    let mut archive = Archive::new(gz);

    archive
        .unpack(&dest)
        .map_err(CommandError::ExtractionFailed)
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
        ACTIVE_TASKS.fetch_add(1, SeqCst);
    }

    fn decrement_tasks() {
        ACTIVE_TASKS.fetch_sub(1, SeqCst);
    }

    fn task_count() -> usize {
        ACTIVE_TASKS.load(SeqCst)
    }
}