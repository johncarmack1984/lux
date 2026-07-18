//! Token glue for the user channel.

use base64::Engine;

/// The `sub` claim from our own ID token's payload. Unverified base64 decode
/// on purpose: this only *addresses* the topics we ask for — the IoT
/// authorizer independently verifies the token and scopes the granted policy
/// to the sub it verified, so a wrong value here can only produce a denied
/// subscribe.
pub fn jwt_sub(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(claims.get("sub")?.as_str()?.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_sub_reads_the_payload_claim() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"sub":"abc-123","token_use":"id"}"#);
        let token = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.sig");
        assert_eq!(jwt_sub(&token).as_deref(), Some("abc-123"));
    }

    #[test]
    fn jwt_sub_rejects_garbage() {
        assert_eq!(jwt_sub("not-a-jwt"), None);
        assert_eq!(jwt_sub("a.b.c"), None);
    }
}
