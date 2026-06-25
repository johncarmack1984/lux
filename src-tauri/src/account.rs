//! Cognito accounts: sign-up, SRP sign-in, sign-out, and silent refresh, plus
//! secure refresh-token storage in the OS keychain.
//!
//! Like [`crate::remote`], this is configured from the environment and no-ops
//! cleanly when unset: with `COGNITO_*` absent, accounts are disabled and the
//! app runs exactly as it does today (local-only). Identity gates cloud sync
//! (Phase 3) — it never sits in the live DMX path, so a network or auth outage
//! can't affect output.
//!
//! Cognito's sign-up / InitiateAuth / RespondToAuthChallenge / refresh calls are
//! *unauthenticated* (keyed by the public app-client id, not AWS SigV4), so the
//! SDK client runs with `.no_credentials()` — end users never hold AWS keys. SRP
//! keeps the password on-device: only a zero-knowledge proof crosses the wire.

use std::sync::{Arc, Mutex};

use aws_config::BehaviorVersion;
use aws_sdk_cognitoidentityprovider::config::Region;
use aws_sdk_cognitoidentityprovider::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_cognitoidentityprovider::types::{AttributeType, AuthFlowType, ChallengeNameType};
use aws_sdk_cognitoidentityprovider::Client;
use aws_cognito_srp::{SrpClient, User};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime};

use crate::cmd::CmdEvent;

const KEYRING_SERVICE: &str = "com.johncarmack.lux";
const KEYRING_USER: &str = "cognito-session";

/// Cognito configuration from the environment. Absent → accounts disabled.
#[derive(Debug, Clone)]
struct Config {
    region: String,
    pool_id: String,
    client_id: String,
}

fn load_config() -> Option<Config> {
    let _ = dotenvy::dotenv();
    Some(Config {
        region: std::env::var("COGNITO_REGION").ok()?,
        pool_id: std::env::var("COGNITO_USER_POOL_ID").ok()?,
        client_id: std::env::var("COGNITO_APP_CLIENT_ID").ok()?,
    })
}

/// The signed-in session, held in memory. The refresh token is also persisted to
/// the keychain so sign-in survives a restart; the id/access tokens are short
/// lived and recreated on refresh.
#[derive(Default)]
struct Session {
    email: Option<String>,
    id_token: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
}

impl Session {
    fn signed_in(&self) -> bool {
        self.id_token.is_some()
    }
    fn clear(&mut self) {
        *self = Session::default();
    }
}

/// Tauri-managed account state.
pub struct LuxAccount {
    config: Option<Config>,
    session: Arc<Mutex<Session>>,
}

/// Auth status crossing IPC: whether accounts are configured at all, whether
/// someone is signed in, and their email.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub configured: bool,
    pub signed_in: bool,
    pub email: Option<String>,
}

/// Tokens returned by a Cognito auth flow. `refresh` is `None` on a refresh
/// (Cognito doesn't reissue it) and `Some` on initial sign-in.
struct Tokens {
    id: String,
    access: String,
    refresh: Option<String>,
}

/// What we persist to the keychain — enough to restore a session on next launch.
#[derive(Serialize, Deserialize)]
struct StoredSession {
    refresh_token: String,
    email: String,
}

impl LuxAccount {
    pub fn from_env() -> Self {
        let config = load_config();
        match &config {
            Some(c) => log::info!("accounts enabled (Cognito pool {})", c.pool_id),
            None => log::info!("accounts not configured; sign-in disabled (set COGNITO_* to enable)"),
        }
        LuxAccount {
            config,
            session: Arc::new(Mutex::new(Session::default())),
        }
    }

    pub fn status(&self) -> AuthStatus {
        let session = self.session.lock().unwrap();
        AuthStatus {
            configured: self.config.is_some(),
            signed_in: session.signed_in(),
            email: session.email.clone(),
        }
    }

    fn config(&self) -> Result<Config, String> {
        self.config
            .clone()
            .ok_or_else(|| "accounts are not configured".to_string())
    }

    pub fn sign_up(&self, email: String, password: String) -> Result<(), String> {
        let cfg = self.config()?;
        block_on(do_sign_up(cfg, email, password))
    }

    pub fn confirm_sign_up(&self, email: String, code: String) -> Result<(), String> {
        let cfg = self.config()?;
        block_on(do_confirm_sign_up(cfg, email, code))
    }

    pub fn sign_in(&self, email: String, password: String) -> Result<AuthStatus, String> {
        let cfg = self.config()?;
        let tokens = block_on(do_sign_in(cfg, email.clone(), password))?;
        if let Some(refresh) = &tokens.refresh {
            save_session(&StoredSession {
                refresh_token: refresh.clone(),
                email: email.clone(),
            });
        }
        self.apply(tokens, Some(email));
        Ok(self.status())
    }

    pub fn sign_out(&self) -> AuthStatus {
        clear_session();
        self.session.lock().unwrap().clear();
        self.status()
    }

    /// Store freshly-issued tokens. `email` is set on sign-in; left untouched on
    /// a silent refresh (it carries over from the restored session).
    fn apply(&self, tokens: Tokens, email: Option<String>) {
        let mut session = self.session.lock().unwrap();
        session.id_token = Some(tokens.id);
        session.access_token = Some(tokens.access);
        if let Some(refresh) = tokens.refresh {
            session.refresh_token = Some(refresh);
        }
        if email.is_some() {
            session.email = email;
        }
    }
}

/// On launch, if a refresh token is in the keychain, silently refresh to a
/// signed-in session and tell the UI. Failure (revoked/expired) is logged and
/// the app stays signed-out — never fatal.
pub fn restore_on_startup<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<LuxAccount>();
    let Ok(cfg) = state.config() else { return };
    let Some(stored) = load_session() else { return };
    let session = state.session.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match do_refresh(cfg, stored.refresh_token).await {
            Ok(tokens) => {
                {
                    let mut s = session.lock().unwrap();
                    s.id_token = Some(tokens.id);
                    s.access_token = Some(tokens.access);
                    s.refresh_token = load_session().map(|s| s.refresh_token);
                    s.email = Some(stored.email.clone());
                }
                let status = AuthStatus {
                    configured: true,
                    signed_in: true,
                    email: Some(stored.email),
                };
                let _ = CmdEvent::AuthChanged { status }.emit(&app);
                log::info!("restored signed-in session from keychain");
            }
            Err(e) => log::warn!("could not restore session ({e}); staying signed out"),
        }
    });
}

// --- Cognito calls (unauthenticated; owned args so the future is 'static) -----

async fn cognito_client(region: &str) -> Client {
    let cfg = aws_config::defaults(BehaviorVersion::latest())
        .no_credentials()
        .region(Region::new(region.to_owned()))
        .load()
        .await;
    Client::new(&cfg)
}

async fn do_sign_up(cfg: Config, email: String, password: String) -> Result<(), String> {
    let client = cognito_client(&cfg.region).await;
    let email_attr = AttributeType::builder()
        .name("email")
        .value(&email)
        .build()
        .map_err(|e| e.to_string())?;
    client
        .sign_up()
        .client_id(&cfg.client_id)
        .username(&email)
        .password(&password)
        .user_attributes(email_attr)
        .send()
        .await
        .map_err(sdk_err)?;
    Ok(())
}

async fn do_confirm_sign_up(cfg: Config, email: String, code: String) -> Result<(), String> {
    let client = cognito_client(&cfg.region).await;
    client
        .confirm_sign_up()
        .client_id(&cfg.client_id)
        .username(&email)
        .confirmation_code(&code)
        .send()
        .await
        .map_err(sdk_err)?;
    Ok(())
}

async fn do_sign_in(cfg: Config, email: String, password: String) -> Result<Tokens, String> {
    let client = cognito_client(&cfg.region).await;

    // SRP step 1: send SRP_A, get the PASSWORD_VERIFIER challenge.
    let srp = SrpClient::new(User::new(&cfg.pool_id, &email, &password), &cfg.client_id, None);
    let auth = srp.get_auth_parameters();
    let init = client
        .initiate_auth()
        .auth_flow(AuthFlowType::UserSrpAuth)
        .client_id(&cfg.client_id)
        .auth_parameters("USERNAME", auth.username.clone())
        .auth_parameters("SRP_A", auth.a.clone())
        .send()
        .await
        .map_err(sdk_err)?;

    let params = init
        .challenge_parameters()
        .ok_or("no challenge parameters in InitiateAuth response")?;
    let get = |k: &str| {
        params
            .get(k)
            .cloned()
            .ok_or_else(|| format!("challenge missing {k}"))
    };
    let (salt, secret_block, srp_b, user_id) = (
        get("SALT")?,
        get("SECRET_BLOCK")?,
        get("SRP_B")?,
        get("USER_ID_FOR_SRP")?,
    );

    // SRP step 2: prove knowledge of the password and exchange for tokens.
    let proof = srp
        .verify(&secret_block, &user_id, &salt, &srp_b)
        .map_err(|e| format!("SRP verification failed: {e}"))?;
    let resp = client
        .respond_to_auth_challenge()
        .challenge_name(ChallengeNameType::PasswordVerifier)
        .client_id(&cfg.client_id)
        .challenge_responses("USERNAME", user_id)
        .challenge_responses("PASSWORD_CLAIM_SECRET_BLOCK", proof.password_claim_secret_block)
        .challenge_responses("PASSWORD_CLAIM_SIGNATURE", proof.password_claim_signature)
        .challenge_responses("TIMESTAMP", proof.timestamp)
        .send()
        .await
        .map_err(sdk_err)?;

    tokens_from(resp.authentication_result())
}

async fn do_refresh(cfg: Config, refresh_token: String) -> Result<Tokens, String> {
    let client = cognito_client(&cfg.region).await;
    let out = client
        .initiate_auth()
        .auth_flow(AuthFlowType::RefreshTokenAuth)
        .client_id(&cfg.client_id)
        .auth_parameters("REFRESH_TOKEN", refresh_token)
        .send()
        .await
        .map_err(sdk_err)?;
    tokens_from(out.authentication_result())
}

fn tokens_from(
    result: Option<&aws_sdk_cognitoidentityprovider::types::AuthenticationResultType>,
) -> Result<Tokens, String> {
    let r = result.ok_or("no authentication result")?;
    Ok(Tokens {
        id: r.id_token().ok_or("no id token")?.to_owned(),
        access: r.access_token().unwrap_or_default().to_owned(),
        refresh: r.refresh_token().map(str::to_owned),
    })
}

/// Best-effort message from a Cognito SDK error — the modeled service message
/// (e.g. "Incorrect username or password.") when present, else the raw error.
fn sdk_err<E: ProvideErrorMetadata, R>(e: SdkError<E, R>) -> String {
    if let Some(msg) = e.as_service_error().and_then(|se| se.message()) {
        return msg.to_owned();
    }
    e.to_string()
}

// --- keychain (refresh token at rest) ---------------------------------------

fn keyring_entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
}

fn save_session(s: &StoredSession) {
    let result = (|| {
        let json = serde_json::to_string(s).map_err(|e| e.to_string())?;
        keyring_entry()
            .and_then(|e| e.set_password(&json))
            .map_err(|e| e.to_string())
    })();
    if let Err(e) = result {
        log::warn!("could not save session to keychain: {e}");
    }
}

fn load_session() -> Option<StoredSession> {
    let json = keyring_entry().ok()?.get_password().ok()?;
    serde_json::from_str(&json).ok()
}

fn clear_session() {
    if let Ok(entry) = keyring_entry() {
        let _ = entry.delete_credential();
    }
}

// --- async bridge ------------------------------------------------------------

/// Run an async Cognito call to completion from a synchronous ttipc command.
/// A dedicated thread with its own current-thread runtime keeps this safe to
/// call whether or not we're already inside the Tauri runtime; auth is
/// infrequent (sign-in / sign-up / refresh), so the per-call runtime is fine.
fn block_on<F>(fut: F) -> F::Output
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build auth runtime")
            .block_on(fut)
    })
    .join()
    .expect("auth worker thread panicked")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end SRP sign-in against the live Cognito pool. Ignored by default
    /// (needs network + an existing user). Run with the pool env + a test user:
    /// ```sh
    /// COGNITO_REGION=… COGNITO_USER_POOL_ID=… COGNITO_APP_CLIENT_ID=… \
    /// LUX_TEST_EMAIL=… LUX_TEST_PASSWORD=… \
    /// cargo test srp_sign_in_live -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "hits live Cognito; needs env + a real user"]
    fn srp_sign_in_live() {
        let cfg = Config {
            region: std::env::var("COGNITO_REGION").unwrap(),
            pool_id: std::env::var("COGNITO_USER_POOL_ID").unwrap(),
            client_id: std::env::var("COGNITO_APP_CLIENT_ID").unwrap(),
        };
        let email = std::env::var("LUX_TEST_EMAIL").unwrap();
        let password = std::env::var("LUX_TEST_PASSWORD").unwrap();
        let tokens = block_on(do_sign_in(cfg, email, password)).expect("SRP sign-in failed");
        assert!(!tokens.id.is_empty(), "expected an id token");
        assert!(tokens.refresh.is_some(), "expected a refresh token on sign-in");
    }
}
