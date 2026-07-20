//! Apple-side crypto: identity-token verification against Apple's JWKS, and
//! the signed-JWT client secret that authenticates lux to Apple's token and
//! revocation endpoints.
//!
//! Verification here is the trust boundary for the whole sign-in path — the
//! Function URL routes and the Cognito VerifyAuthChallenge trigger both run
//! through [`AppleAuth::verify_token`], so a token that fails signature,
//! issuer, audience, or expiry never touches a user record.

use std::collections::HashMap;

use jsonwebtoken::{
    decode, decode_header, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

const APPLE_ISSUER: &str = "https://appleid.apple.com";
const APPLE_JWKS_URL: &str = "https://appleid.apple.com/auth/keys";
const APPLE_TOKEN_URL: &str = "https://appleid.apple.com/auth/token";
const APPLE_REVOKE_URL: &str = "https://appleid.apple.com/auth/revoke";

/// The Sign in with Apple key (`lux/siwa-key` in Secrets Manager): the `.p8`
/// private key minted in the developer portal plus its identifiers. Signs the
/// client-secret JWT; never leaves the Lambda.
#[derive(Debug, Deserialize)]
pub struct SiwaKey {
    pub key_id: String,
    pub team_id: String,
    pub private_key: String,
}

/// What a verified identity token asserts. `email` is present only when the
/// token carries one Apple marked verified — the only email account-linking
/// may trust.
#[derive(Debug)]
pub struct AppleIdentity {
    pub sub: String,
    pub email: Option<String>,
}

/// The identity-token claims we read; signature/issuer/audience/expiry are
/// validated from the raw token by the library.
#[derive(Debug, Deserialize)]
struct AppleClaims {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    /// Apple sends `true` or, on some paths, the string `"true"`.
    #[serde(default)]
    email_verified: Option<Value>,
    #[serde(default)]
    nonce: Option<String>,
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

pub struct AppleAuth {
    /// Expected token audience: the app's bundle id (native flows use it as
    /// the Apple `client_id` too).
    audience: String,
    http: reqwest::Client,
    /// Apple's signing keys by `kid`. Fetched lazily, replaced wholesale on a
    /// `kid` miss (Apple rotates keys); one forced refetch, then fail.
    keys: RwLock<HashMap<String, DecodingKey>>,
}

impl AppleAuth {
    pub fn new(audience: String) -> Self {
        Self {
            audience,
            http: reqwest::Client::new(),
            keys: RwLock::new(HashMap::new()),
        }
    }

    /// Full verification for tokens arriving with their sheet context: claims
    /// plus the nonce binding (the token's `nonce` must equal the SHA-256 of
    /// the raw nonce the client minted for this sheet run).
    pub async fn verify_identity(
        &self,
        token: &str,
        raw_nonce: &str,
    ) -> Result<AppleIdentity, String> {
        let view = self.verify_token(token).await?;
        let Some(nonce) = view.0.nonce.as_deref() else {
            return Err("token has no nonce claim".into());
        };
        if nonce != sha256_hex(raw_nonce) {
            return Err("nonce mismatch".into());
        }
        Ok(identity(view))
    }

    /// Signature/issuer/audience/expiry verification alone — the
    /// VerifyAuthChallenge trigger's variant, which has no sheet context (the
    /// nonce binding is enforced where the token enters the system; here the
    /// binding that matters is Apple `sub` ↔ Cognito user, checked by the
    /// caller against the link store).
    pub async fn verify_token(&self, token: &str) -> Result<AppleClaimsView, String> {
        let header = decode_header(token).map_err(|e| format!("bad token header: {e}"))?;
        let kid = header.kid.ok_or_else(|| "token missing kid".to_owned())?;

        if let Some(claims) = self.try_decode(token, &kid).await? {
            return Ok(claims);
        }
        // Unknown kid: Apple rotated keys (or we never fetched). Refresh once.
        self.refresh_keys().await?;
        match self.try_decode(token, &kid).await? {
            Some(claims) => Ok(claims),
            None => Err(format!("unknown apple signing key {kid}")),
        }
    }

    /// Decode against the cached key for `kid`, if present. `Ok(None)` means
    /// "no such key cached"; a present-but-failing key is a hard error.
    async fn try_decode(&self, token: &str, kid: &str) -> Result<Option<AppleClaimsView>, String> {
        let keys = self.keys.read().await;
        let Some(key) = keys.get(kid) else {
            return Ok(None);
        };
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[APPLE_ISSUER]);
        validation.set_audience(&[&self.audience]);
        let data = decode::<AppleClaims>(token, key, &validation)
            .map_err(|e| format!("invalid token: {e}"))?;
        Ok(Some(AppleClaimsView(data.claims)))
    }

    async fn refresh_keys(&self) -> Result<(), String> {
        let jwks: Jwks = self
            .http
            .get(APPLE_JWKS_URL)
            .send()
            .await
            .map_err(|e| format!("apple jwks fetch failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("apple jwks malformed: {e}"))?;
        let fresh: HashMap<String, DecodingKey> = jwks
            .keys
            .into_iter()
            .filter_map(|k| {
                DecodingKey::from_rsa_components(&k.n, &k.e)
                    .ok()
                    .map(|dk| (k.kid, dk))
            })
            .collect();
        if fresh.is_empty() {
            return Err("apple jwks contained no usable keys".into());
        }
        *self.keys.write().await = fresh;
        Ok(())
    }

    /// Exchange the sheet's single-use authorization code for Apple's token
    /// set; returns the refresh token (the revocable credential account
    /// deletion is required to revoke).
    pub async fn exchange_code(&self, key: &SiwaKey, code: &str) -> Result<String, String> {
        #[derive(Deserialize)]
        struct TokenResponse {
            refresh_token: Option<String>,
        }
        let secret = self.client_secret(key)?;
        let resp = self
            .http
            .post(APPLE_TOKEN_URL)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form_body(&[
                ("client_id", self.audience.as_str()),
                ("client_secret", secret.as_str()),
                ("code", code),
                ("grant_type", "authorization_code"),
            ]))
            .send()
            .await
            .map_err(|e| format!("apple token endpoint unreachable: {e}"))?;
        if !resp.status().is_success() {
            // Error bodies carry only an error code (e.g. invalid_grant) —
            // safe to log, and the single useful diagnostic.
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("apple token exchange {status}: {body}"));
        }
        let tokens: TokenResponse = resp
            .json()
            .await
            .map_err(|e| format!("apple token response malformed: {e}"))?;
        tokens
            .refresh_token
            .ok_or_else(|| "apple token response had no refresh_token".into())
    }

    /// Revoke a stored Apple refresh token (account deletion's Apple-side duty).
    pub async fn revoke(&self, key: &SiwaKey, refresh_token: &str) -> Result<(), String> {
        let secret = self.client_secret(key)?;
        let resp = self
            .http
            .post(APPLE_REVOKE_URL)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form_body(&[
                ("client_id", self.audience.as_str()),
                ("client_secret", secret.as_str()),
                ("token", refresh_token),
                ("token_type_hint", "refresh_token"),
            ]))
            .send()
            .await
            .map_err(|e| format!("apple revoke endpoint unreachable: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("apple revoke {status}: {body}"));
        }
        Ok(())
    }

    /// The client secret Apple's `/auth/token` and `/auth/revoke` require: a
    /// short-lived ES256 JWT signed with the portal-minted `.p8` key.
    fn client_secret(&self, key: &SiwaKey) -> Result<String, String> {
        #[derive(Serialize)]
        struct SecretClaims<'a> {
            iss: &'a str,
            iat: u64,
            exp: u64,
            aud: &'a str,
            sub: &'a str,
        }
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(key.key_id.clone());
        let now = now_secs();
        let claims = SecretClaims {
            iss: &key.team_id,
            iat: now,
            exp: now + 300,
            aud: APPLE_ISSUER,
            sub: &self.audience,
        };
        let signer = EncodingKey::from_ec_pem(key.private_key.as_bytes())
            .map_err(|e| format!("siwa private key unusable: {e}"))?;
        encode(&header, &claims, &signer).map_err(|e| format!("client secret signing failed: {e}"))
    }

    #[cfg(test)]
    async fn seed_key(&self, kid: &str, key: DecodingKey) {
        self.keys.write().await.insert(kid.to_owned(), key);
    }
}

/// Verified claims, exposed read-only so callers can't construct one without
/// going through verification.
pub struct AppleClaimsView(AppleClaims);

impl AppleClaimsView {
    pub fn sub(&self) -> &str {
        &self.0.sub
    }
}

fn identity(view: AppleClaimsView) -> AppleIdentity {
    let claims = view.0;
    let verified = match &claims.email_verified {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true",
        _ => false,
    };
    AppleIdentity {
        sub: claims.sub,
        email: claims.email.filter(|_| verified),
    }
}

/// `application/x-www-form-urlencoded` body (this reqwest build compiles
/// without its form support, and the values — JWTs, Apple codes — are simple).
fn form_body(pairs: &[(&str, &str)]) -> String {
    fn enc(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                _ => {
                    use std::fmt::Write;
                    let _ = write!(out, "%{b:02X}");
                }
            }
        }
        out
    }
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn sha256_hex(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    digest.iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KID: &str = "apple-test-key";
    const AUDIENCE: &str = "com.johncarmack.lux";

    // The same throwaway 2048-bit RSA test keypair lux-auth's tests use.
    const PRIVATE_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCuCAG4yH4kvTeh
MkF84jX4Q1RgR28rEbwHG0nbnVCKNtRKsD1DijWjvoXxcLPpLmCBixF6GmyHQ8j1
tSJH5T6HSke/9mgSBnfrkpykIks/LKGJ1/6ox8h9DntfHf6z5uLUvg5geYIyXeg0
6uOxQaaDSQSpOxHzT7VoGLiZCfG/+FwdLQCMOi0bRRxfBo53rBvCJTP8ZgJX3YSK
I8Xj0NLQGZTnb1amI9HIqpxMfWRhPRpAy6kgGEai1WiMdAnDHvDxWNm5vl0rGBzB
eNHhEMEsyIBTsWMs3biNGdt2rb6qDNa3HouPApuuMPLpNm7W1fvxZ7Gghrl2ehK9
SYw1PoLzAgMBAAECggEAJesxsNjif0/JGLLSCQti1gKZllbKNpipHuVHvPW0cEEN
FW78EkTBdjmThq1XTfXgailqd+/c+MYAueSrIP4mlyTMqFtghpjpNSdfQPYF7jBj
zByHbLAHE5R9thZbgkhK4S6+BDBFeYLzjuAlF2CmDtHwlYz81sZl0NYeFp5PkdOH
6nPLbD9CRPfvEB1A/QpHU/DD1/3T8/tDBO392mJKsVxzpG62r4MTr4sP+zyKqnaj
tCO7XxkQUkOwv41JXJKLnUz6SNlSW8vz76qNcRIzccHLWrLtiIW2ZiR6fhF8lFHe
7//HZXANIbdAs+Imao/S7ekWui4wypktYaa/tvnMjQKBgQDd0HFAFPXLlNKI7l4s
sFDVBJ7nqr0G6s1cC+eJbPOVOoy7e+BSCzOp5ivk6YJjJx1yK0tmV5oqxQX17OaO
5aqZOvTLRa5ck5WCl3Q+7mSxs3FpexH6l+SHVtZGLKAoOKwGekF0sJOMgFJO7HXJ
bqLJni1UJTjkbnCMJJ+Q0PFnvwKBgQDI2lB99kOudVBcNCMD5QEcX33bpaqrGFCH
JgDhkpZDVhVMNsONpCqNU7j2nNVEvjgbat/VcdAYGIF4dp90QJHpQxYonPZ/DWps
IeNHuH5syfdYBdcrNP4WgthD0xNbluJPbcji9soofQp5uqMvdMyGNH/Z0KjlnAOJ
ymJoXKVRzQKBgQDE0pX7X93vBKKAgMst6lH/gzchqF5NCgKpf6K3Tecibq68GiKl
im0QgD5IxG8/XlEBoqsoJ+mTs/ojC1BWUjK7/xWCXdVnLkoHdC7hPJY7HFgxWdRN
QYS2FvbRk/2VUxxKLydvzNNQY/klMSsfTz3Bm8rrFJBUGi9iG4k/bjgXbwKBgB+S
oeCLG6yK6Gz2DSMJlpkdMa2bZy6qDc6Q3MaYwmInYAWw/iB/0+iPZp3tnWDG/g7h
R/pHf8yp3YBQNVSS6dzfHNaZhe4G79m7ofyeNdFoFieSE3bJR7/GJbTTs1FMcJrH
yTJUVQb0UPc9rXVCSPw3uHlG4aXmVnAMjleVaK9pAoGBANxk8XLmvRZoVoNKuUGZ
f2GOeMvrbEBLrzaSDN3mGx1dxBisaYl6P6XeKsq8cO+Y5sq+D5gzm26aIxxn/e8e
DZ9X7qMu4asofS1OhFYEzHASk/xQxzHWfQ4GElRg/DxZJv2Md3RmVPKVx8N8+Vw7
p6mW1b1y0a1+HBXK3Q29CgPg
-----END PRIVATE KEY-----
";
    const PUBLIC_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEArggBuMh+JL03oTJBfOI1
+ENUYEdvKxG8BxtJ251QijbUSrA9Q4o1o76F8XCz6S5ggYsRehpsh0PI9bUiR+U+
h0pHv/ZoEgZ365KcpCJLPyyhidf+qMfIfQ57Xx3+s+bi1L4OYHmCMl3oNOrjsUGm
g0kEqTsR80+1aBi4mQnxv/hcHS0AjDotG0UcXwaOd6wbwiUz/GYCV92EiiPF49DS
0BmU529WpiPRyKqcTH1kYT0aQMupIBhGotVojHQJwx7w8VjZub5dKxgcwXjR4RDB
LMiAU7FjLN24jRnbdq2+qgzWtx6LjwKbrjDy6TZu1tX78WexoIa5dnoSvUmMNT6C
8wIDAQAB
-----END PUBLIC KEY-----
";

    // A throwaway PKCS#8 P-256 key, standing in for a portal-minted `.p8`.
    const EC_PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgMgrlg56UNdAIx8Sy
PaNjgYD9H87aiu72kVM4feGqzpehRANCAAQE52G8sprGOcAIUFXU8WtoNLLD3Q80
+yf7BVFFsN+hOx/bqJD2Ums2zRu85qOnODSQ5Mchg4vs1zkk4CHviX8W
-----END PRIVATE KEY-----
";

    #[derive(Serialize)]
    struct TestClaims {
        sub: String,
        iss: String,
        aud: String,
        exp: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        nonce: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        email_verified: Option<Value>,
    }

    impl Default for TestClaims {
        fn default() -> Self {
            Self {
                sub: "001234.abcdef.5678".into(),
                iss: APPLE_ISSUER.into(),
                aud: AUDIENCE.into(),
                exp: 4_102_444_800, // 2100-01-01
                nonce: Some(sha256_hex("raw-nonce")),
                email: Some("user@example.com".into()),
                email_verified: Some(Value::Bool(true)),
            }
        }
    }

    fn sign(claims: &TestClaims) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(KID.to_string());
        encode(
            &header,
            claims,
            &EncodingKey::from_rsa_pem(PRIVATE_PEM).expect("test key parses"),
        )
        .expect("test token signs")
    }

    async fn auth() -> AppleAuth {
        // Prod installs ring in main() before any client exists; tests mirror it.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let auth = AppleAuth::new(AUDIENCE.into());
        auth.seed_key(
            KID,
            DecodingKey::from_rsa_pem(PUBLIC_PEM).expect("test key parses"),
        )
        .await;
        auth
    }

    #[tokio::test]
    async fn verifies_a_valid_token_with_nonce_and_verified_email() {
        let identity = auth()
            .await
            .verify_identity(&sign(&TestClaims::default()), "raw-nonce")
            .await
            .expect("verifies");
        assert_eq!(identity.sub, "001234.abcdef.5678");
        assert_eq!(identity.email.as_deref(), Some("user@example.com"));
    }

    #[tokio::test]
    async fn accepts_apples_stringly_email_verified() {
        let claims = TestClaims {
            email_verified: Some(Value::String("true".into())),
            ..TestClaims::default()
        };
        let identity = auth()
            .await
            .verify_identity(&sign(&claims), "raw-nonce")
            .await
            .expect("verifies");
        assert_eq!(identity.email.as_deref(), Some("user@example.com"));
    }

    #[tokio::test]
    async fn withholds_an_unverified_email() {
        let claims = TestClaims {
            email_verified: Some(Value::Bool(false)),
            ..TestClaims::default()
        };
        let identity = auth()
            .await
            .verify_identity(&sign(&claims), "raw-nonce")
            .await
            .expect("verifies");
        assert!(identity.email.is_none(), "unverified email must not link");
    }

    #[tokio::test]
    async fn rejects_a_nonce_mismatch() {
        let err = auth()
            .await
            .verify_identity(&sign(&TestClaims::default()), "some-other-nonce")
            .await
            .expect_err("must reject");
        assert!(err.contains("nonce"), "got: {err}");
    }

    #[tokio::test]
    async fn rejects_a_missing_nonce() {
        let claims = TestClaims {
            nonce: None,
            ..TestClaims::default()
        };
        assert!(auth()
            .await
            .verify_identity(&sign(&claims), "raw-nonce")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_a_wrong_audience() {
        let claims = TestClaims {
            aud: "com.example.other".into(),
            ..TestClaims::default()
        };
        assert!(auth().await.verify_token(&sign(&claims)).await.is_err());
    }

    #[tokio::test]
    async fn rejects_a_wrong_issuer() {
        let claims = TestClaims {
            iss: "https://evil.example".into(),
            ..TestClaims::default()
        };
        assert!(auth().await.verify_token(&sign(&claims)).await.is_err());
    }

    #[tokio::test]
    async fn rejects_an_expired_token() {
        let claims = TestClaims {
            exp: 946_684_800, // 2000-01-01
            ..TestClaims::default()
        };
        assert!(auth().await.verify_token(&sign(&claims)).await.is_err());
    }

    #[test]
    fn client_secret_is_a_signed_es256_jwt() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let auth = AppleAuth::new(AUDIENCE.into());
        let key = SiwaKey {
            key_id: "ABC123DEFG".into(),
            team_id: "T3UN6N5K6Z".into(),
            private_key: EC_PRIVATE_PEM.into(),
        };
        let secret = auth.client_secret(&key).expect("mints");
        assert_eq!(secret.split('.').count(), 3, "compact JWT");
        let header = decode_header(&secret).expect("header parses");
        assert_eq!(header.alg, Algorithm::ES256);
        assert_eq!(header.kid.as_deref(), Some("ABC123DEFG"));
    }
}
