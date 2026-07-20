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
use std::sync::{Arc, Mutex};

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

/// How the current session was established. Password sessions can change
/// their password and re-auth by SRP; Apple sessions re-auth by sheet. Stored
/// with the session so a restart keeps the distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Password,
    Apple,
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
    provider: Provider,
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
    /// Base URL of the lux-apple-auth Function URL; `None` keeps Sign in with
    /// Apple dark (endpoints field absent/empty).
    apple_auth_url: Option<String>,
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
    /// How the signed-in session was established; `None` when signed out.
    pub provider: Option<Provider>,
    /// Whether native Sign in with Apple is available in this build (platform
    /// carries the entitlement AND the backend URL is configured).
    pub apple: bool,
}

/// Tokens returned by a Cognito auth flow. `refresh` is `None` on a refresh
/// (Cognito doesn't reissue it) and `Some` on initial sign-in.
struct Tokens {
    id: String,
    access: String,
    refresh: Option<String>,
}

/// What we persist to the keychain — enough to restore a session on next launch.
/// `provider` defaults for items written by pre-Apple builds.
#[derive(Serialize, Deserialize)]
struct StoredSession {
    refresh_token: String,
    email: String,
    #[serde(default)]
    provider: Provider,
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
        let base_url = |url: &str| {
            Some(url.to_string())
                .filter(|url| !url.is_empty())
                .map(|url| url.trim_end_matches('/').to_string())
        };
        let endpoints = crate::endpoints::effective();
        LuxAccount {
            config,
            sync_url: base_url(&endpoints.sync_url),
            apple_auth_url: base_url(&endpoints.apple_auth_url),
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
            provider: session.signed_in().then_some(session.provider),
            apple: self.apple_sign_in_available(),
        }
    }

    /// Native Sign in with Apple, on THIS build: the backend must be
    /// configured, accounts must be configured, and the binary must carry the
    /// applesignin entitlement — iOS and the Mac App Store flavor today. The
    /// Developer ID .dmg stays dark until it embeds a provisioning profile
    /// with the entitlement.
    fn apple_sign_in_available(&self) -> bool {
        let entitled =
            cfg!(target_os = "ios") || (cfg!(target_os = "macos") && cfg!(feature = "mas"));
        entitled && self.config.is_some() && self.apple_auth_url.is_some()
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
                provider: Provider::Password,
            });
        }
        self.apply(tokens, Some(email), Some(Provider::Password));
        Ok(self.status())
    }

    /// Sign in with Apple: run the native sheet, then trade its identity token
    /// for ordinary user-pool tokens at the lux-apple-auth service. Async on
    /// purpose — the sheet is user-paced, and this must never block the main
    /// thread the sheet's callbacks are delivered on.
    pub async fn sign_in_with_apple(&self, app: &AppHandle) -> Result<AuthStatus, String> {
        if !self.apple_sign_in_available() {
            return Err("sign in with apple is not available in this build".into());
        }
        let base = self
            .apple_auth_url
            .clone()
            .ok_or("sign in with apple is not configured")?;

        // The token's nonce claim must be SHA-256(raw); the raw value goes only
        // to our backend, which re-hashes and compares — binding the token to
        // this sheet run.
        let raw_nonce = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let hashed_nonce = sha256_hex(&raw_nonce);

        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            lux_apple_id::authorize(
                &hashed_nonce,
                Box::new(move |result| {
                    let _ = tx.send(result);
                }),
            );
        })
        .map_err(|e| format!("could not present the sign-in sheet: {e}"))?;
        let authorization = rx
            .await
            .map_err(|_| "the sign-in sheet never completed".to_string())??;

        let request = lux_wire::apple::SignInRequest {
            identity_token: authorization.identity_token,
            authorization_code: authorization.authorization_code,
            raw_nonce,
            email: authorization.email,
            full_name: authorization.full_name,
        };
        let response = reqwest::Client::new()
            .post(format!(
                "{base}/{}/{}",
                lux_wire::apple::AUTH_SEGMENT,
                lux_wire::apple::APPLE_SEGMENT
            ))
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("could not reach the sign-in service: {e}"))?;
        if !response.status().is_success() {
            let status = response.status();
            let message = response
                .json::<lux_wire::ErrorResponse>()
                .await
                .map(|body| body.error)
                .unwrap_or_else(|_| format!("sign-in service answered {status}"));
            return Err(message);
        }
        let tokens: lux_wire::apple::SignInResponse = response
            .json()
            .await
            .map_err(|e| format!("malformed sign-in response: {e}"))?;

        // The session's email (the local store's cloud-binding key) comes from
        // our own id token — the sheet only carries one on first authorization.
        let email =
            email_from_id_token(&tokens.id_token).ok_or("sign-in response carried no email")?;
        save_session(&StoredSession {
            refresh_token: tokens.refresh_token.clone(),
            email: email.clone(),
            provider: Provider::Apple,
        });
        self.apply(
            Tokens {
                id: tokens.id_token,
                access: tokens.access_token,
                refresh: Some(tokens.refresh_token),
            },
            Some(email),
            Some(Provider::Apple),
        );
        Ok(self.status())
    }

    /// Best-effort Apple-side revocation ahead of account deletion (App Store
    /// guideline 5.1.1): the backend revokes the stored Apple token and drops
    /// the link. Never linked is a quiet no-op; a failure is logged and
    /// deletion proceeds — the user can still revoke from Settings, and the
    /// sign-in path self-heals a stale link.
    pub fn revoke_apple_link(&self) {
        let Some(base) = self.apple_auth_url.clone() else {
            return;
        };
        let Some(token) = self.current_id_token() else {
            return;
        };
        let result = block_on(async move {
            let response = reqwest::Client::new()
                .post(format!(
                    "{base}/{}/{}/{}",
                    lux_wire::apple::AUTH_SEGMENT,
                    lux_wire::apple::APPLE_SEGMENT,
                    lux_wire::apple::REVOKE_SEGMENT
                ))
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(format!("revoke answered {}", response.status()))
            }
        });
        if let Err(e) = result {
            log::warn!("apple revoke skipped ({e}); continuing with deletion");
        }
    }

    /// The account's paired headless devices (lux-node boxes) from the auth
    /// service's registry — the delete-account confirm's tally. No auth
    /// service configured means no pairing anywhere: an empty list.
    pub fn list_paired_devices(&self) -> Result<Vec<lux_wire::device::DeviceRecord>, String> {
        let Some(base) = self.apple_auth_url.clone() else {
            return Ok(Vec::new());
        };
        let Some(token) = self.current_id_token() else {
            return Err("not signed in".into());
        };
        block_on(async move {
            let response = reqwest::Client::new()
                .get(format!(
                    "{base}/{}/{}/{}",
                    lux_wire::apple::AUTH_SEGMENT,
                    lux_wire::device::DEVICE_SEGMENT,
                    lux_wire::device::LIST_SEGMENT
                ))
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                return Err(format!("device list answered {}", response.status()));
            }
            response
                .json::<lux_wire::device::ListResponse>()
                .await
                .map(|r| r.devices)
                .map_err(|e| e.to_string())
        })
    }

    /// Pending (unclaimed) devices the auth service saw from the *caller's own*
    /// public egress — the Add-device screen's list. The backend filters to
    /// same-NAT, so a phone on cellular gets an empty list (not the venue's
    /// boxes). No auth service configured means nothing to pair: an empty list.
    pub fn list_pending_devices(&self) -> Result<Vec<lux_wire::device::PendingDevice>, String> {
        let Some(base) = self.apple_auth_url.clone() else {
            return Ok(Vec::new());
        };
        let Some(token) = self.current_id_token() else {
            return Err("not signed in".into());
        };
        // Prefer IPv4 so we rendezvous on the same public network as the box (see
        // [`pairing_client`] / lux_engine::net).
        let client = pairing_client(&base);
        block_on(async move {
            let response = client
                .get(format!(
                    "{base}/{}/{}/{}",
                    lux_wire::apple::AUTH_SEGMENT,
                    lux_wire::device::DEVICE_SEGMENT,
                    lux_wire::device::PENDING_SEGMENT
                ))
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                return Err(format!("device pending answered {}", response.status()));
            }
            response
                .json::<lux_wire::device::PendingResponse>()
                .await
                .map(|r| r.devices)
                .map_err(|e| e.to_string())
        })
    }

    /// Approve a pending device: bind it to this account and the chosen setup
    /// (the picker replaces `lux-node install`'s interactive list). The box's
    /// next `/token` poll returns the grant and it comes online.
    pub fn approve_device(
        &self,
        pair_ref: String,
        setup_id: String,
        universe: Option<u16>,
        name: Option<String>,
    ) -> Result<(), String> {
        let Some(base) = self.apple_auth_url.clone() else {
            return Err("device pairing is not available on this build".into());
        };
        let Some(token) = self.current_id_token() else {
            return Err("not signed in".into());
        };
        // Same IPv4 preference as the pending list, so the approve's same-egress
        // check lands on the box's network key.
        let client = pairing_client(&base);
        block_on(async move {
            let response = client
                .post(format!(
                    "{base}/{}/{}/{}",
                    lux_wire::apple::AUTH_SEGMENT,
                    lux_wire::device::DEVICE_SEGMENT,
                    lux_wire::device::APPROVE_SEGMENT
                ))
                .bearer_auth(token)
                .json(&lux_wire::device::ApproveRequest {
                    pair_ref,
                    setup_id,
                    universe,
                    name,
                })
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(format!("device approve answered {}", response.status()))
            }
        })
    }

    /// Remove a paired device from the account registry ("Remove device"). The
    /// box drops out of the device list immediately; cutting its live IoT
    /// access is a later (authorizer-level) phase.
    pub fn revoke_device(&self, device_id: String) -> Result<(), String> {
        let Some(base) = self.apple_auth_url.clone() else {
            return Err("device pairing is not available on this build".into());
        };
        let Some(token) = self.current_id_token() else {
            return Err("not signed in".into());
        };
        block_on(async move {
            let response = reqwest::Client::new()
                .post(format!(
                    "{base}/{}/{}/{}",
                    lux_wire::apple::AUTH_SEGMENT,
                    lux_wire::device::DEVICE_SEGMENT,
                    lux_wire::device::REVOKE_SEGMENT
                ))
                .bearer_auth(token)
                .json(&lux_wire::device::RevokeRequest { device_id })
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(format!("device revoke answered {}", response.status()))
            }
        })
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
            self.apply(tokens, None, None);
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

    /// Store freshly-issued tokens. `email`/`provider` are set on sign-in and
    /// left untouched on a silent refresh (they carry over from the restored
    /// session).
    fn apply(&self, tokens: Tokens, email: Option<String>, provider: Option<Provider>) {
        let mut session = self.session.lock_or_recover();
        session.id_token = Some(tokens.id);
        session.access_token = Some(tokens.access);
        if let Some(refresh) = tokens.refresh {
            session.refresh_token = Some(refresh);
        }
        if email.is_some() {
            session.email = email;
        }
        if let Some(provider) = provider {
            session.provider = provider;
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
        match do_refresh(cfg, stored.refresh_token.clone()).await {
            Ok(tokens) => {
                // Re-save so items written by older builds pick up the current
                // keychain access policy (save recreates the item).
                save_session(&stored);
                {
                    let mut s = session.lock_or_recover();
                    s.id_token = Some(tokens.id);
                    s.access_token = Some(tokens.access);
                    s.refresh_token = load_session().map(|s| s.refresh_token);
                    s.email = Some(stored.email.clone());
                    s.provider = stored.provider;
                }
                let status = app.state::<LuxAccount>().status();
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

// The AWS SDK's default HTTP client trusts the platform *native* root store,
// unreadable on iOS — the bundled webpki roots (lux-engine) make TLS
// verification identical on every platform.
pub(crate) use lux_engine::tls::webpki_pem_bundle;

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

fn sha256_hex(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(raw.as_bytes())
        .iter()
        .fold(String::new(), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// The `email` claim from our OWN Cognito id token, parsed without
/// verification on purpose: the desktop holds tokens, it doesn't verify them
/// (that's the services' job), and this one just arrived over TLS from our
/// backend.
fn email_from_id_token(id_token: &str) -> Option<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .get("email")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
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

/// The stored-session keychain entry. Off iOS this goes through `keyring`'s
/// v1 wrapper on purpose — its first use lazily registers the platform
/// credential store (macOS Keychain, Windows Credential Manager, Linux Secret
/// Service) — and hands back the underlying core entry so every caller works
/// with one type.
#[cfg(not(target_os = "ios"))]
fn keyring_entry() -> keyring_core::Result<keyring_core::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map(|entry| entry.inner)
}

/// iOS: build the entry against the data-protection store registered in
/// [`init_keychain`], relaxing the item to `after-first-unlock`. The store's
/// default policy (`when-unlocked`) makes the refresh token unreadable
/// whenever iOS launches the app while the phone is locked (prewarming), so
/// the startup restore silently finds nothing and that long-lived process
/// greets the user signed out. A launch-time credential wants exactly
/// after-first-unlock accessibility.
#[cfg(target_os = "ios")]
fn keyring_entry() -> keyring_core::Result<keyring_core::Entry> {
    let modifiers = std::collections::HashMap::from([("access-policy", "after-first-unlock")]);
    keyring_core::Entry::new_with_modifiers(KEYRING_SERVICE, KEYRING_USER, &modifiers)
}

fn save_session(s: &StoredSession) {
    let result = (|| {
        let json = serde_json::to_string(s).map_err(|e| e.to_string())?;
        let entry = keyring_entry().map_err(|e| e.to_string())?;
        // Recreate rather than update: updating an existing item keeps its
        // original accessibility, and items written by older builds carry the
        // when-unlocked default (see keyring_entry) — delete + set stamps the
        // current policy on every save.
        let _ = entry.delete_credential();
        entry.set_password(&json).map_err(|e| e.to_string())
    })();
    if let Err(e) = result {
        log::warn!("could not save session to keychain: {e}");
    }
}

fn load_session() -> Option<StoredSession> {
    let entry = match keyring_entry() {
        Ok(entry) => entry,
        Err(e) => {
            log::warn!("keychain unavailable ({e}); cannot restore a session");
            return None;
        }
    };
    match entry.get_password() {
        Ok(json) => serde_json::from_str(&json)
            .inspect_err(|e| log::warn!("stored session unreadable ({e}); ignoring it"))
            .ok(),
        // First run or signed out — the normal quiet path.
        Err(keyring_core::Error::NoEntry) => None,
        Err(e) => {
            log::warn!("could not read the stored session ({e}); starting signed out");
            None
        }
    }
}

fn clear_session() {
    if let Ok(entry) = keyring_entry() {
        let _ = entry.delete_credential();
    }
}

// --- async bridge ------------------------------------------------------------

/// Run an async Cognito call to completion from a synchronous ttipc command.
/// A reqwest client that prefers IPv4 for the pairing rendezvous, matching the
/// node so both ends land on the same public-network key (the shared NAT
/// address; see `lux_engine::net`). Best-effort: if the host can't be
/// pre-resolved we fall back to reqwest's default dual-stack DNS, so a lookup
/// hiccup never blocks pairing.
fn pairing_client(base_url: &str) -> reqwest::Client {
    let mut builder = reqwest::Client::builder();
    if let Ok(url) = reqwest::Url::parse(base_url) {
        if let Some(host) = url.host_str() {
            let port = url.port_or_known_default().unwrap_or(443);
            let addrs = lux_engine::net::ipv4_first_addrs(host, port);
            if !addrs.is_empty() {
                builder = builder.resolve_to_addrs(host, &addrs);
            }
        }
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

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
