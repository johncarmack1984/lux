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

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::Serialize;

    const KID: &str = "test-key";
    const ISSUER: &str = "https://cognito-idp.us-west-1.amazonaws.com/us-west-1_test";
    const CLIENT_ID: &str = "test-client";

    // A throwaway 2048-bit RSA keypair, for this test only.
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

    #[derive(Serialize)]
    struct TestClaims {
        sub: String,
        token_use: String,
        iss: String,
        aud: String,
        exp: usize,
    }

    fn verifier() -> Verifier {
        Verifier {
            keys: HashMap::from([(
                KID.to_string(),
                DecodingKey::from_rsa_pem(PUBLIC_PEM).unwrap(),
            )]),
            issuer: ISSUER.to_string(),
            client_id: CLIENT_ID.to_string(),
        }
    }

    fn sign(token_use: &str) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(KID.to_string());
        let claims = TestClaims {
            sub: "user-123".to_string(),
            token_use: token_use.to_string(),
            iss: ISSUER.to_string(),
            aud: CLIENT_ID.to_string(),
            exp: 4_102_444_800, // 2100-01-01
        };
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(PRIVATE_PEM).unwrap(),
        )
        .unwrap()
    }

    // Regression guard: jsonwebtoken 10 ships no crypto backend in its default
    // features, so without `rust_crypto` this verify panics ("Could not
    // determine the process-level CryptoProvider") — which is exactly how the
    // sync API and IoT authorizer 500'd on every authenticated request.
    #[test]
    fn verifies_a_valid_id_token() {
        let claims = verifier()
            .verify(&sign("id"))
            .expect("a well-formed ID token verifies");
        assert_eq!(claims.sub, "user-123");
    }

    #[test]
    fn rejects_a_non_id_token() {
        assert!(verifier().verify(&sign("access")).is_err());
    }
}
