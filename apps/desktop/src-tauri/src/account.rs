//! Cognito accounts: sign-up, SRP sign-in, sign-out, and silent refresh, plus
//! secure refresh-token storage in the OS keychain.
//!
//! All environment values (pool, client, region, sync URL) come from
//! [`crate::endpoints`] — the generated-and-embedded production config, with
//! `endpoints.local.json` overrides for dev stacks. Nothing is hardcoded here
//! and nothing reads env. Sign-in stays optional — identity only gates cloud
//! sync (Phase 3) and never sits in the live DMX path, so a network or auth
//! outage can't affect output.
//!
//! Cognito's sign-up / InitiateAuth / RespondToAuthChallenge / refresh calls are
//! *unauthenticated* (keyed by the public app-client id, not AWS SigV4), so the
//! SDK client runs with `.no_credentials()` — end users never hold AWS keys. SRP
//! keeps the password on-device: only a zero-knowledge proof crosses the wire.

use crate::lock::LockPolicy;
use std::sync::{Arc, Mutex, OnceLock};

use aws_cognito_srp::{SrpClient, User};
use aws_config::BehaviorVersion;
use aws_sdk_cognitoidentityprovider::config::Region;
use aws_sdk_cognitoidentityprovider::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_cognitoidentityprovider::types::{AttributeType, AuthFlowType, ChallengeNameType};
use aws_sdk_cognitoidentityprovider::Client;
use aws_smithy_http_client::tls::{self, rustls_provider::CryptoMode};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

use crate::cmd::CmdEvent;

const KEYRING_SERVICE: &str = "com.johncarmack.lux";
const KEYRING_USER: &str = "cognito-session";

/// Cognito configuration, from [`crate::endpoints::effective`]. `None` when any
/// piece is missing — accounts simply stay unconfigured.
#[derive(Debug, Clone)]
struct Config {
    region: String,
    pool_id: String,
    client_id: String,
}

fn load_config() -> Option<Config> {
    let endpoints = crate::endpoints::effective();
    let config = Config {
        region: endpoints.cognito_region.clone(),
        pool_id: endpoints.cognito_user_pool_id.clone(),
        client_id: endpoints.cognito_app_client_id.clone(),
    };
    (!config.region.is_empty() && !config.pool_id.is_empty() && !config.client_id.is_empty())
        .then_some(config)
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
    /// Base URL of the lux-sync-api Function URL (`LUX_SYNC_URL`); `None`
    /// disables cloud sync even when auth is configured.
    sync_url: Option<String>,
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
    pub fn load() -> Self {
        let config = load_config();
        match &config {
            Some(c) => log::info!("accounts enabled (Cognito pool {})", c.pool_id),
            None => log::info!(
                "accounts not configured (endpoints file has no Cognito values); sign-in disabled"
            ),
        }
        let sync_url = Some(crate::endpoints::effective().sync_url.clone())
            .filter(|url| !url.is_empty())
            .map(|url| url.trim_end_matches('/').to_string());
        LuxAccount {
            config,
            sync_url,
            session: Arc::new(Mutex::new(Session::default())),
        }
    }

    /// Base URL of the sync API, if cloud sync is configured.
    pub fn sync_url(&self) -> Option<String> {
        self.sync_url.clone()
    }

    pub fn signed_in(&self) -> bool {
        self.session.lock_or_recover().signed_in()
    }

    /// The signed-in account email, used as the local store's cloud-binding key.
    pub fn email(&self) -> Option<String> {
        self.session.lock_or_recover().email.clone()
    }

    /// The current (possibly soon-to-expire) id token, for a sync request.
    pub fn current_id_token(&self) -> Option<String> {
        self.session.lock_or_recover().id_token.clone()
    }

    /// Exchange the stored refresh token for fresh id/access tokens (called by
    /// the sync layer on a 401). Updates the session and returns the new id token.
    pub async fn refresh_id_token(&self) -> Result<String, String> {
        let cfg = self.config()?;
        let refresh = self
            .session
            .lock_or_recover()
            .refresh_token
            .clone()
            .ok_or("not signed in")?;
        let tokens = do_refresh(cfg, refresh).await?;
        let mut session = self.session.lock_or_recover();
        session.id_token = Some(tokens.id.clone());
        session.access_token = Some(tokens.access);
        Ok(tokens.id)
    }

    pub fn status(&self) -> AuthStatus {
        let session = self.session.lock_or_recover();
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
        self.session.lock_or_recover().clear();
        self.status()
    }

    /// Permanently delete the signed-in Cognito user (self-service `DeleteUser`,
    /// authorized by the access token), then clear the session + keychain. The
    /// caller wipes the account's server-side data *first*, while the tokens
    /// still authenticate; both steps are idempotent, so a failure here leaves
    /// the account intact and the whole flow retryable.
    pub fn delete_account(&self) -> Result<AuthStatus, String> {
        let cfg = self.config()?;
        let access = self
            .session
            .lock_or_recover()
            .access_token
            .clone()
            .filter(|t| !t.is_empty())
            .ok_or("not signed in")?;
        if let Err(first) = block_on(do_delete_user(cfg.clone(), access)) {
            // The access token may simply have expired — refresh once and retry,
            // surfacing the original error if the refresh path can't help.
            let refresh = self
                .session
                .lock_or_recover()
                .refresh_token
                .clone()
                .ok_or_else(|| first.clone())?;
            let tokens = block_on(do_refresh(cfg.clone(), refresh)).map_err(|_| first.clone())?;
            self.apply(tokens, None);
            let access = self
                .session
                .lock_or_recover()
                .access_token
                .clone()
                .filter(|t| !t.is_empty())
                .ok_or(first)?;
            block_on(do_delete_user(cfg, access))?;
        }
        clear_session();
        self.session.lock_or_recover().clear();
        Ok(self.status())
    }

    /// Store freshly-issued tokens. `email` is set on sign-in; left untouched on
    /// a silent refresh (it carries over from the restored session).
    fn apply(&self, tokens: Tokens, email: Option<String>) {
        let mut session = self.session.lock_or_recover();
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
pub fn restore_on_startup(app: &AppHandle) {
    let state = app.state::<LuxAccount>();
    let Ok(cfg) = state.config() else { return };
    let Some(stored) = load_session() else { return };
    let session = state.session.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match do_refresh(cfg, stored.refresh_token).await {
            Ok(tokens) => {
                {
                    let mut s = session.lock_or_recover();
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
                // Pull any setups changed on other devices since last run, and
                // start listening for change nudges going forward.
                crate::cloud::schedule_sync(&app);
                crate::nudge::start(&app);
            }
            Err(e) => log::warn!("could not restore session ({e}); staying signed out"),
        }
    });
}

// --- Cognito calls (unauthenticated; owned args so the future is 'static) -----

/// The Mozilla CA set (webpki) as a single PEM bundle, built once.
///
/// The AWS SDK's default HTTP client trusts the platform *native* root store.
/// iOS apps can't read the system CA store, so rustls parses zero roots and the
/// SDK aborts (`debug_assert` in debug, empty trust store in release). Bundling
/// the webpki roots makes TLS verification identical on every platform.
pub(crate) fn webpki_pem_bundle() -> &'static [u8] {
    static BUNDLE: OnceLock<Vec<u8>> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let mut pem = Vec::new();
        for cert in webpki_root_certs::TLS_SERVER_ROOT_CERTS {
            pem.extend_from_slice(
                ::pem::encode(&::pem::Pem::new("CERTIFICATE", cert.as_ref().to_vec())).as_bytes(),
            );
        }
        pem
    })
}

async fn cognito_client(region: &str) -> Client {
    // Trust the bundled webpki roots rather than the platform native store
    // (which is unreadable on iOS). Crypto stays on ring, matching the app's
    // process-default provider.
    let tls_ctx = tls::TlsContext::builder()
        .with_trust_store(tls::TrustStore::empty().with_pem_certificate(webpki_pem_bundle()))
        .build()
        .expect("build TLS context");
    let http_client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(CryptoMode::Ring))
        .tls_context(tls_ctx)
        .build_https();
    let cfg = aws_config::defaults(BehaviorVersion::latest())
        .http_client(http_client)
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
    let srp = SrpClient::new(
        User::new(&cfg.pool_id, &email, &password),
        &cfg.client_id,
        None,
    );
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
        .challenge_responses(
            "PASSWORD_CLAIM_SECRET_BLOCK",
            proof.password_claim_secret_block,
        )
        .challenge_responses("PASSWORD_CLAIM_SIGNATURE", proof.password_claim_signature)
        .challenge_responses("TIMESTAMP", proof.timestamp)
        .send()
        .await
        .map_err(sdk_err)?;

    tokens_from(resp.authentication_result())
}

async fn do_delete_user(cfg: Config, access_token: String) -> Result<(), String> {
    let client = cognito_client(&cfg.region).await;
    client
        .delete_user()
        .access_token(access_token)
        .send()
        .await
        .map_err(sdk_err)?;
    Ok(())
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

/// Register the credential store that [`keyring`] uses. Call once at startup,
/// before any keychain access (session restore or sign-in).
///
/// `keyring`'s `v1` feature auto-registers a default store for macOS, Windows,
/// and Linux, but its `set_credential_store` (keyring 4.1.2 `src/v1.rs`) has no
/// branch for iOS — so on iOS no default store is ever set and every keychain
/// read/write fails with `NoDefaultStore`, silently losing the signed-in
/// session across launches. iOS only exposes the data-protection Keychain, so
/// register the `protected` store as the default here.
#[cfg(target_os = "ios")]
pub fn init_keychain() {
    match apple_native_keyring_store::protected::Store::new() {
        Ok(store) => {
            keyring_core::set_default_store(store);
            log::info!("registered the iOS data-protection Keychain store");
        }
        Err(e) => log::error!("could not register the iOS Keychain store: {e}"),
    }
}

/// No-op off iOS: `keyring`'s `v1` feature self-registers the platform store
/// (macOS Keychain, Windows Credential Manager, Linux Secret Service).
#[cfg(not(target_os = "ios"))]
pub fn init_keychain() {}

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
/// `crate::cloud` borrows it for its own rare synchronous call (account wipe).
pub(crate) fn block_on<F>(fut: F) -> F::Output
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

    /// Proves the SDK client verifies Cognito's TLS cert using the bundled
    /// webpki roots alone — the platform native store is never consulted (this
    /// is what was broken on iOS, where it's unreadable). A bogus InitiateAuth
    /// returns a *modeled* Cognito `ServiceError`, which can only happen after a
    /// successful TLS handshake; a broken trust store surfaces as a
    /// `DispatchFailure` instead. Ignored by default (needs network).
    #[test]
    #[ignore = "hits live Cognito over TLS; run with --ignored"]
    fn webpki_roots_verify_cognito_tls() {
        let cfg = load_config().expect("endpoints.prod.json must configure Cognito");
        let result = block_on(async move {
            cognito_client(&cfg.region)
                .await
                .initiate_auth()
                .client_id(&cfg.client_id)
                .auth_flow(AuthFlowType::UserSrpAuth)
                .auth_parameters("USERNAME", "nobody@example.invalid")
                .auth_parameters("SRP_A", "00")
                .send()
                .await
        });
        // Reaching Cognito at all proves the handshake verified its cert via the
        // webpki roots (the native store is never consulted). A challenge (Ok) or
        // a modeled `ServiceError` both qualify; only a `DispatchFailure` means
        // the trust store couldn't verify the cert — the iOS native-roots break.
        if let Err(SdkError::DispatchFailure(f)) = &result {
            panic!("TLS via webpki roots failed (trust store broken): {f:?}");
        }
    }

    /// End-to-end SRP sign-in against the live Cognito pool (the embedded
    /// endpoints config; drop an `endpoints.local.json` in `src-tauri/` to aim
    /// elsewhere). Ignored by default (needs network + an existing user); the
    /// test user's credentials are the only inputs and stay on the command line:
    /// ```sh
    /// LUX_TEST_EMAIL=… LUX_TEST_PASSWORD=… \
    /// cargo test srp_sign_in_live -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "hits live Cognito; needs a real user's credentials"]
    fn srp_sign_in_live() {
        let cfg = load_config().expect("endpoints config must configure Cognito");
        let email = std::env::var("LUX_TEST_EMAIL").unwrap();
        let password = std::env::var("LUX_TEST_PASSWORD").unwrap();
        let tokens = block_on(do_sign_in(cfg, email, password)).expect("SRP sign-in failed");
        assert!(!tokens.id.is_empty(), "expected an id token");
        assert!(
            tokens.refresh.is_some(),
            "expected a refresh token on sign-in"
        );
    }
}
