//! Cognito auth for the node: the same embedded-SRP sign-in and silent
//! refresh the apps use (mirrored from the desktop's account layer — the
//! desktop crate links Tauri, so the node can't import it; unify into
//! lux-engine when the desktop's account layer is next touched).

use aws_cognito_srp::{SrpClient, User};
use aws_config::BehaviorVersion;
use aws_sdk_cognitoidentityprovider::config::Region;
use aws_sdk_cognitoidentityprovider::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_cognitoidentityprovider::types::{AuthFlowType, ChallengeNameType};
use aws_sdk_cognitoidentityprovider::Client;
use aws_smithy_http_client::tls::{self, rustls_provider::CryptoMode};
use lux_engine::tls::webpki_pem_bundle;

use crate::config::Endpoints;

pub struct Tokens {
    pub id: String,
    pub refresh: Option<String>,
}

async fn cognito_client(region: &str) -> Client {
    // Trust the bundled webpki roots rather than the platform native store —
    // a headless box may not even have ca-certificates installed. Crypto on
    // ring, matching the process-default provider main() installs.
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

pub async fn sign_in(env: &Endpoints, email: &str, password: &str) -> Result<Tokens, String> {
    let client = cognito_client(&env.cognito_region).await;

    // SRP step 1: send SRP_A, get the PASSWORD_VERIFIER challenge.
    let srp = SrpClient::new(
        User::new(&env.cognito_user_pool_id, email, password),
        &env.cognito_app_client_id,
        None,
    );
    let auth = srp.get_auth_parameters();
    let init = client
        .initiate_auth()
        .auth_flow(AuthFlowType::UserSrpAuth)
        .client_id(&env.cognito_app_client_id)
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
        .client_id(&env.cognito_app_client_id)
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

/// `client_id` must be the client that minted the refresh token; `None` means
/// the interactive client from the endpoints (pre-pairing sessions).
pub async fn refresh(
    env: &Endpoints,
    client_id: Option<&str>,
    refresh_token: &str,
) -> Result<Tokens, String> {
    let client = cognito_client(&env.cognito_region).await;
    let out = client
        .initiate_auth()
        .auth_flow(AuthFlowType::RefreshTokenAuth)
        .client_id(client_id.unwrap_or(&env.cognito_app_client_id))
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
