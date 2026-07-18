//! The pool's CUSTOM_AUTH triggers, served from the same binary as the routes
//! that drive the flow.
//!
//! Trust lives in Verify: the challenge answer is the Apple identity token,
//! re-verified here from scratch (signature via Apple's JWKS, issuer,
//! audience, expiry) and then bound to the authenticating user through the
//! link store — `answerCorrect` only when the token's Apple `sub` is linked to
//! exactly this Cognito user. That holds even if a client calls Cognito's
//! public CUSTOM_AUTH directly and skips our Function URL: without a valid,
//! linked Apple token there is no way through. (The nonce binding is enforced
//! at the Function URL, where the sheet context exists; here a token is a
//! bearer credential with Apple's own 10-minute expiry.)
//!
//! Cognito requires the whole event echoed back with `response` filled in.

use lambda_runtime::Error;
use serde_json::{json, Value};

use crate::{store, Ctx};

pub async fn handle(ctx: &Ctx, mut event: Value) -> Result<Value, Error> {
    let source = event
        .get("triggerSource")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    match source.as_str() {
        "DefineAuthChallenge_Authentication" => define(&mut event),
        "CreateAuthChallenge_Authentication" => create(&mut event),
        "VerifyAuthChallengeResponse_Authentication" => verify(ctx, &mut event).await,
        other => return Err(format!("unsupported trigger source {other}").into()),
    }
    Ok(event)
}

/// One custom challenge, one shot: issue tokens after a correct answer, fail
/// the auth after a wrong one (the client never retries within a session —
/// each sheet run starts a fresh auth).
fn define(event: &mut Value) {
    let session = event
        .pointer("/request/session")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let succeeded = session.iter().any(|attempt| {
        attempt
            .get("challengeResult")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });

    let response = &mut event["response"];
    if succeeded {
        response["issueTokens"] = json!(true);
        response["failAuthentication"] = json!(false);
    } else if session.is_empty() {
        response["issueTokens"] = json!(false);
        response["failAuthentication"] = json!(false);
        response["challengeName"] = json!("CUSTOM_CHALLENGE");
    } else {
        response["issueTokens"] = json!(false);
        response["failAuthentication"] = json!(true);
    }
}

/// The challenge itself carries nothing secret — Verify does the real work —
/// but Cognito requires the parameter objects to exist.
fn create(event: &mut Value) {
    let response = &mut event["response"];
    response["publicChallengeParameters"] = json!({ "challenge": "apple-identity-token" });
    response["privateChallengeParameters"] = json!({});
    response["challengeMetadata"] = json!("APPLE_IDENTITY_TOKEN");
}

/// A bad token, an unlinked credential, or a link to a different user are all
/// the same outcome: `answerCorrect = false`. Only infrastructure failures
/// (the link store erroring) surface as invoke errors.
async fn verify(ctx: &Ctx, event: &mut Value) {
    let answer = event
        .pointer("/request/challengeAnswer")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let user_sub = event
        .pointer("/request/userAttributes/sub")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let correct = answer_is_correct(ctx, &answer, &user_sub).await;
    event["response"]["answerCorrect"] = json!(correct);
}

async fn answer_is_correct(ctx: &Ctx, answer: &str, user_sub: &str) -> bool {
    if answer.is_empty() || user_sub.is_empty() {
        return false;
    }
    let claims = match ctx.apple.verify_token(answer).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("challenge answer rejected: {e}");
            return false;
        }
    };
    match store::get_link(ctx, claims.sub()).await {
        Ok(Some(link)) => link.sub == user_sub,
        Ok(None) => {
            tracing::warn!("challenge answer for an unlinked apple credential");
            false
        }
        Err(e) => {
            tracing::error!("link lookup failed during verify: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(source: &str, session: Value) -> Value {
        json!({
            "triggerSource": source,
            "request": { "session": session },
            "response": {}
        })
    }

    #[test]
    fn define_starts_with_a_custom_challenge() {
        let mut e = event("DefineAuthChallenge_Authentication", json!([]));
        define(&mut e);
        assert_eq!(e["response"]["challengeName"], "CUSTOM_CHALLENGE");
        assert_eq!(e["response"]["issueTokens"], false);
        assert_eq!(e["response"]["failAuthentication"], false);
    }

    #[test]
    fn define_issues_tokens_after_a_correct_answer() {
        let mut e = event(
            "DefineAuthChallenge_Authentication",
            json!([{ "challengeName": "CUSTOM_CHALLENGE", "challengeResult": true }]),
        );
        define(&mut e);
        assert_eq!(e["response"]["issueTokens"], true);
        assert_eq!(e["response"]["failAuthentication"], false);
    }

    #[test]
    fn define_fails_after_a_wrong_answer() {
        let mut e = event(
            "DefineAuthChallenge_Authentication",
            json!([{ "challengeName": "CUSTOM_CHALLENGE", "challengeResult": false }]),
        );
        define(&mut e);
        assert_eq!(e["response"]["issueTokens"], false);
        assert_eq!(e["response"]["failAuthentication"], true);
    }

    #[test]
    fn create_fills_the_required_parameter_objects() {
        let mut e = event("CreateAuthChallenge_Authentication", json!([]));
        create(&mut e);
        assert!(e["response"]["publicChallengeParameters"].is_object());
        assert!(e["response"]["privateChallengeParameters"].is_object());
    }
}
