use std::io::Error;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("command '{0}' not found")]
    CommandNotFound(String),
    #[error("missing argument: '{0}'")]
    MissingArgument(String),
    #[error("invalid version notation ({0})")]
    InvalidVersionNotation(semver::Error),
    #[error("invalid package name: '{0}'")]
    InvalidPackageName(String),
}

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("failed to execute http request ({0})")]
    HTTPFailed(reqwest::Error),
    #[error("failed to parse http data to struct via json ({0})")]
    ParsingFailed(serde_json::Error),
    #[error("failed to get http response text ({0})")]
    FailedResponseText(reqwest::Error),
    #[error("failed to get http response bytes ({0})")]
    FailedResponseBytes(reqwest::Error),
    #[error("the package version you provided was invalid or does not exist")]
    InvalidVersion,
    #[error("failed to extract tar file ({0})")]
    ExtractionFailed(Error),
    #[error("could not find cache directory ({0})")]
    NoCacheDirectory(Error),
    #[error("failed to get directory entry ({0})")]
    FailedDirectoryEntry(Error),
    #[error("failed to create file ({0})")]
    FailedToCreateFile(Error),
    #[error("failed to write file ({0})")]
    FailedToWriteFile(Error),
    #[error("failed to serialize package lock ({0})")]
    FailedToSerializePackageLock(serde_json::Error),
    #[error("login failed (HTTP {status}): {body}")]
    LoginFailed { status: u16, body: String },
    #[error("registry requires a one-time password (OTP); re-run with oxide login --otp <code>")]
    OtpRequired,
    #[error("login timed out: browser login was not completed within the allowed time")]
    LoginTimedOut,
    #[error("failed to read file ({0})")]
    FailedToReadFile(Error),
    #[error("{0}")]
    GitFailed(String),
    #[error("failed to validate package integrity")]
    IntegrityCheckFailed,
    #[error("internal error: mutex lock poisoned")]
    MutexPoisoned,
    #[error("unknown config key '{0}'; run 'oxide config list' to see valid keys")]
    UnknownConfigKey(String),
    #[error("failed to write config ({0})")]
    ConfigWriteFailed(Error),
    #[error("failed to spawn process: {0}")]
    ProcessFailed(String),
    #[error("cannot determine OS config directory")]
    ConfigDirUnavailable,
    #[error("unsafe or malformed package identifier: {0}")]
    MalformedPackageId(String),
    #[error("URL must use HTTPS to prevent plaintext transmission: {0}")]
    InsecureUrl(String),
    #[error("response body too large ({0} bytes); refusing to buffer")]
    ResponseTooLarge(u64),
}
