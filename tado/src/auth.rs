//! Signing in to tado° with the OAuth 2 device code flow (the only flow tado° allows since 2025).
//!
//! The user opens a tado° page, confirms a short code, and the plugin receives a refresh token.
//! tado° rotates refresh tokens: every refresh returns a new one and revokes the old one, so
//! refreshes are serialised and the new token is written to disk straight away.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::files;

/// tado°'s public client id for the device code flow, from tado°'s own REST API help page.
const CLIENT_ID: &str = "1bb50063-6b0c-4d11-bd99-387f4a91cc46";

fn login_base() -> String {
	std::env::var("TADO_LOGIN_BASE").unwrap_or_else(|_| "https://login.tado.com".into())
}
fn authorize_url() -> String {
	format!("{}/oauth2/device_authorize", login_base())
}
fn token_url() -> String {
	format!("{}/oauth2/token", login_base())
}

#[derive(Debug, Clone)]
pub enum AuthError {
	/// No account, or tado° rejected the saved token. The user has to sign in again.
	SignedOut,
	/// tado° could not be reached; the saved token is kept.
	Network(String),
}

#[derive(Serialize, Deserialize)]
struct Saved {
	refresh_token: String,
}

struct Session {
	refresh_token: String,
	access: Option<(String, Instant)>,
}

static SESSION: LazyLock<Mutex<Option<Session>>> = LazyLock::new(|| {
	let saved: Option<Saved> = files::load(files::AUTH);
	Mutex::new(saved.map(|s| Session { refresh_token: s.refresh_token, access: None }))
});

pub static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
	reqwest::Client::builder()
		.user_agent(concat!("opendeck-tado/", env!("CARGO_PKG_VERSION")))
		.timeout(Duration::from_secs(20))
		.build()
		.expect("HTTP client")
});

#[derive(Deserialize)]
struct TokenResponse {
	access_token: String,
	refresh_token: String,
	expires_in: u64,
}

#[derive(Deserialize, Default)]
struct OAuthError {
	#[serde(default)]
	error: String,
	#[serde(default)]
	error_description: String,
}

fn store(session: &mut Option<Session>, token: TokenResponse) -> String {
	files::save(files::AUTH, &Saved { refresh_token: token.refresh_token.clone() }, true);
	let expires = Instant::now() + Duration::from_secs(token.expires_in.max(60));
	*session = Some(Session {
		refresh_token: token.refresh_token,
		access: Some((token.access_token.clone(), expires)),
	});
	token.access_token
}

pub async fn signed_in() -> bool {
	SESSION.lock().await.is_some()
}

/// A valid access token, refreshing it first when it is about to expire.
pub async fn access_token() -> Result<String, AuthError> {
	let mut session = SESSION.lock().await;
	let Some(s) = session.as_ref() else { return Err(AuthError::SignedOut) };
	if let Some((token, expires)) = &s.access {
		if Instant::now() + Duration::from_secs(30) < *expires {
			return Ok(token.clone());
		}
	}
	let response = HTTP
		.post(token_url())
		.form(&[("client_id", CLIENT_ID), ("grant_type", "refresh_token"), ("refresh_token", &s.refresh_token)])
		.send()
		.await
		.map_err(|e| AuthError::Network(e.to_string()))?;
	let status = response.status();
	if status.is_success() {
		let token: TokenResponse = response.json().await.map_err(|e| AuthError::Network(e.to_string()))?;
		log::info!("Refreshed the tado° access token");
		return Ok(store(&mut session, token));
	}
	let error: OAuthError = response.json().await.unwrap_or_default();
	if error.error == "invalid_grant" || status == reqwest::StatusCode::UNAUTHORIZED {
		log::warn!("tado° refused the saved sign-in ({status} {}): signing out", error.error);
		*session = None;
		files::remove(files::AUTH);
		return Err(AuthError::SignedOut);
	}
	Err(AuthError::Network(format!("{status} {}", error.error)))
}

/// Forgets the cached access token, so the next call refreshes it (after a 401).
pub async fn invalidate_access() {
	if let Some(s) = SESSION.lock().await.as_mut() {
		s.access = None;
	}
}

pub async fn sign_out() {
	*SESSION.lock().await = None;
	files::remove(files::AUTH);
}

#[derive(Deserialize, Clone, Debug)]
pub struct DeviceCode {
	pub device_code: String,
	pub user_code: String,
	pub verification_uri_complete: String,
	pub expires_in: u64,
	#[serde(default = "default_interval")]
	pub interval: u64,
}

fn default_interval() -> u64 {
	5
}

/// Starts a sign-in: returns the code and the page the user confirms it on.
pub async fn start_sign_in() -> Result<DeviceCode, String> {
	let response = HTTP
		.post(authorize_url())
		.form(&[("client_id", CLIENT_ID), ("scope", "offline_access")])
		.send()
		.await
		.map_err(|e| format!("Could not reach tado°: {e}"))?;
	if !response.status().is_success() {
		return Err(format!("tado° refused the sign-in request ({})", response.status()));
	}
	response.json().await.map_err(|e| format!("Unexpected answer from tado°: {e}"))
}

pub enum Poll {
	Done,
	Pending,
	SlowDown,
	Failed(String),
}

/// Asks once whether the user has confirmed the code yet.
pub async fn poll_sign_in(code: &DeviceCode) -> Poll {
	let response = HTTP
		.post(token_url())
		.form(&[
			("client_id", CLIENT_ID),
			("device_code", code.device_code.as_str()),
			("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
		])
		.send()
		.await;
	let response = match response {
		Ok(r) => r,
		Err(e) => {
			log::warn!("Sign-in poll failed: {e}");
			return Poll::Pending;
		}
	};
	if response.status().is_success() {
		return match response.json::<TokenResponse>().await {
			Ok(token) => {
				store(&mut *SESSION.lock().await, token);
				log::info!("Signed in to tado°");
				Poll::Done
			}
			Err(e) => Poll::Failed(format!("Unexpected answer from tado°: {e}")),
		};
	}
	let error: OAuthError = response.json().await.unwrap_or_default();
	match error.error.as_str() {
		"authorization_pending" => Poll::Pending,
		"slow_down" => Poll::SlowDown,
		"expired_token" => Poll::Failed("The code expired. Start the sign-in again.".into()),
		"access_denied" => Poll::Failed("The sign-in was declined.".into()),
		_ => Poll::Failed(format!("Sign-in failed: {} {}", error.error, error.error_description)),
	}
}
