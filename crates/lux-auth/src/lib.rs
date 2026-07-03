//! Cognito ID-token verification, shared by every Lambda that gates on a
//! user's identity (the sync API and the IoT nudge authorizer).
//!
//! The user pool's public signing keys (JWKS) are fetched once at cold start
//! and used to verify the RS256 signature, issuer, audience, and expiry of
//! every incoming token. The caller's `sub` is the only identity we trust — it
//! keys the per-user DynamoDB partition and the per-user nudge topic, so a
//! forged or another-pool token can never reach someone else's data.
//!
//! Callers on reqwest's `rustls-no-provider` build must install a process
//! crypto provider before [`Verifier::new`] performs the JWKS fetch.

use std::collections::HashMap;

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Verifies Cognito ID tokens against a fixed user pool + app client.
pub struct Verifier {
    keys: HashMap<String, DecodingKey>,
    issuer: String,
    client_id: String,
}

/// The ID-token claims we read. The library validates `iss`/`aud`/`exp` from the
/// raw token; we additionally require `token_use == "id"` and read `sub` (the
/// user id that keys the DynamoDB partition).
#[derive(Debug, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub token_use: String,
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
}

impl Verifier {
    /// Fetch the pool's JWKS and build a verifier. Done once per cold start;
    /// Cognito rotates signing keys rarely, and a recycled Lambda re-fetches.
    pub async fn new(region: &str, pool_id: &str, client_id: &str) -> Result<Self, BoxError> {
        let issuer = format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}");
        let jwks: Jwks = reqwest::get(format!("{issuer}/.well-known/jwks.json"))
            .await?
            .json()
            .await?;
        let keys = jwks
            .keys
            .into_iter()
            .filter_map(|k| {
                DecodingKey::from_rsa_components(&k.n, &k.e)
                    .ok()
                    .map(|dk| (k.kid, dk))
            })
            .collect();
        Ok(Self {
            keys,
            issuer,
            client_id: client_id.to_owned(),
        })
    }

    /// Verify a bearer token and return its claims, or an error string for the log.
    pub fn verify(&self, token: &str) -> Result<Claims, String> {
        let header = decode_header(token).map_err(|e| format!("bad token header: {e}"))?;
        let kid = header.kid.ok_or_else(|| "token missing kid".to_owned())?;
        let key = self
            .keys
            .get(&kid)
            .ok_or_else(|| format!("unknown signing key {kid}"))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.client_id]);

        let data =
            decode::<Claims>(token, key, &validation).map_err(|e| format!("invalid token: {e}"))?;
        if data.claims.token_use != "id" {
            return Err(format!(
                "expected an ID token, got {}",
                data.claims.token_use
            ));
        }
        Ok(data.claims)
    }
}
