use std::{
    env::Args,
    io::{self, BufRead, Write},
    path::PathBuf,
};

use async_trait::async_trait;
use serde::Deserialize;
use tokio::time::{sleep, Duration};

use crate::{
    errors::{CommandError, ParseError},
    http::REGISTRY_URL,
};

use super::command_handler::CommandHandler;

const NPM_USER_AGENT: &str = "npm/10.9.2 node/v22.12.0 win32 x64 workspaces/false";

#[derive(Default)]
pub struct LoginHandler {
    otp: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebLoginInit {
    login_url: String,
    done_url: String,
}

#[derive(Deserialize)]
struct DoneResponse {
    token: Option<String>,
}

#[derive(Deserialize)]
struct ClassicLoginResponse {
    token: Option<String>,
    error: Option<String>,
}

fn prompt(label: &str) -> String {
    let mut stdout = io::stdout();
    write!(stdout, "{label}: ").unwrap();
    stdout.flush().unwrap();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).unwrap();
    line.trim().to_string()
}

#[async_trait]
impl CommandHandler for LoginHandler {
    fn parse(&mut self, args: &mut Args) -> Result<(), ParseError> {
        while let Some(arg) = args.next() {
            if arg == "--otp" {
                let otp = args
                    .next()
                    .ok_or_else(|| ParseError::MissingArgument("--otp <code>".to_string()))?;
                self.otp = Some(otp);
            }
        }
        Ok(())
    }

    async fn execute(&self) -> Result<(), CommandError> {
        let client = reqwest::Client::new();

        let token = match web_login(&client).await {
            Ok(t) => t,
            Err(CommandError::LoginFailed { status, .. }) if status == 404 || status == 405 => {
                classic_login(&client, self.otp.as_deref()).await?
            }
            Err(e) => return Err(e),
        };

        save_token(&token)?;
        println!("Logged in — token saved to OS credential store");
        Ok(())
    }
}

async fn web_login(client: &reqwest::Client) -> Result<String, CommandError> {
    let resp = client
        .post(format!("{REGISTRY_URL}/-/v1/login"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("npm-auth-type", "web")
        .header("User-Agent", NPM_USER_AGENT)
        .body("{}")
        .send()
        .await
        .map_err(CommandError::HTTPFailed)?;

    let status = resp.status();
    let body = resp.text().await.map_err(CommandError::FailedResponseText)?;

    if !status.is_success() {
        return Err(CommandError::LoginFailed {
            status: status.as_u16(),
            body,
        });
    }

    let init: WebLoginInit = serde_json::from_str(&body).map_err(CommandError::ParsingFailed)?;

    println!("Login at:");
    println!("{}", init.login_url);
    print!("Press ENTER to open in the browser...");
    io::stdout().flush().unwrap();
    io::stdin().lock().read_line(&mut String::new()).unwrap();
    let _ = open::that(&init.login_url);

    const MAX_POLL_ATTEMPTS: u32 = 60; // 60 × 2 s = 2-minute timeout
    const PROGRESS_INTERVAL: u32 = 15; // print progress every 15 attempts (30 seconds)
    for attempt in 1..=MAX_POLL_ATTEMPTS {
        sleep(Duration::from_secs(2)).await;

        let poll = client
            .get(&init.done_url)
            .header("npm-auth-type", "web")
            .header("User-Agent", NPM_USER_AGENT)
            .send()
            .await
            .map_err(CommandError::HTTPFailed)?;

        if poll.status() == reqwest::StatusCode::OK {
            let poll_body = poll.text().await.map_err(CommandError::FailedResponseText)?;
            let done: DoneResponse =
                serde_json::from_str(&poll_body).map_err(CommandError::ParsingFailed)?;
            if let Some(t) = done.token {
                return Ok(t);
            }
        }

        let remaining = (MAX_POLL_ATTEMPTS - attempt) * 2;
        if attempt % PROGRESS_INTERVAL == 0 && remaining > 0 {
            eprintln!("Still waiting for browser login… ({remaining}s remaining)");
        }
    }

    Err(CommandError::LoginTimedOut)
}

async fn classic_login(client: &reqwest::Client, otp: Option<&str>) -> Result<String, CommandError> {
    let username = prompt("Username");
    let password = rpassword::prompt_password("Password: ")
        .map_err(|e| CommandError::FailedToWriteFile(e.into()))?;
    let email = prompt("Email");

    let body = serde_json::json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": email,
        "type": "user",
    })
    .to_string();

    let url = format!("{REGISTRY_URL}/-/user/org.couchdb.user:{username}");

    let mut req = client
        .put(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("npm-auth-type", "web")
        .header("User-Agent", NPM_USER_AGENT);

    if let Some(code) = otp {
        req = req.header("npm-otp", code);
    }

    let resp = req.body(body).send().await.map_err(CommandError::HTTPFailed)?;

    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED {
        let www_auth = resp
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if www_auth.contains("otp") {
            return Err(CommandError::OtpRequired);
        }
    }

    let resp_body = resp.text().await.map_err(CommandError::FailedResponseText)?;

    if !status.is_success() {
        return Err(CommandError::LoginFailed {
            status: status.as_u16(),
            body: resp_body,
        });
    }

    let parsed: ClassicLoginResponse =
        serde_json::from_str(&resp_body).map_err(CommandError::ParsingFailed)?;

    if let Some(err) = parsed.error {
        return Err(CommandError::LoginFailed { status: status.as_u16(), body: err });
    }

    parsed.token.ok_or_else(|| CommandError::LoginFailed {
        status: status.as_u16(),
        body: "registry did not return a token".into(),
    })
}

const KEYRING_SERVICE: &str = "oxide";
const KEYRING_USER: &str = "npm-token";

fn credentials_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("oxide").join("credentials"))
}

pub fn load_token() -> Option<String> {
    // Try OS keyring first
    if let Some(token) = keyring_core::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()
        .and_then(|e| e.get_password().ok())
    {
        return Some(token);
    }
    // Fall back to credentials file
    credentials_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn save_token(token: &str) -> Result<(), CommandError> {
    // Try OS keyring first
    if keyring_core::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .and_then(|e| e.set_password(token))
        .is_ok()
    {
        return Ok(());
    }
    // Fall back to credentials file
    let path = credentials_file()
        .ok_or_else(|| CommandError::FailedToWriteFile(std::io::Error::other("cannot determine config directory")))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(CommandError::FailedToWriteFile)?;
    }
    std::fs::write(&path, token)
        .map_err(CommandError::FailedToWriteFile)
}