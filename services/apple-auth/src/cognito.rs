//! Cognito admin operations: user lookup/creation and the CUSTOM_AUTH dance
//! that turns a verified Apple identity into ordinary user-pool tokens.
//!
//! Users created here follow the pool's shape exactly (`username_attributes =
//! ["email"]`): the email is the username at creation and `email_verified` is
//! set because Apple already verified it — so a later "add a password" or
//! SRP sign-in path needs nothing special.

use aws_sdk_cognitoidentityprovider::types::{
    AttributeType, AuthFlowType, ChallengeNameType, MessageActionType, UserStatusType, UserType,
};

use crate::Ctx;

#[derive(Debug)]
pub struct User {
    pub username: String,
    pub sub: String,
    /// A self-signup that never entered its confirmation code. Apple verifying
    /// the same email is at least as strong a proof, so the sign-in flow
    /// confirms these instead of stranding them.
    pub unconfirmed: bool,
}

#[derive(Debug)]
pub struct Tokens {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i32,
}

pub async fn find_user_by_email(ctx: &Ctx, email: &str) -> Result<Option<User>, String> {
    find_one(ctx, "email", email).await
}

pub async fn find_user_by_sub(ctx: &Ctx, sub: &str) -> Result<Option<User>, String> {
    find_one(ctx, "sub", sub).await
}

async fn find_one(ctx: &Ctx, attr: &str, value: &str) -> Result<Option<User>, String> {
    // The filter grammar quotes the value; a quote can't appear in a valid
    // email or sub, so refuse rather than build a broken (or clever) filter.
    if value.contains('"') || value.contains('\\') {
        return Err(format!("unfilterable {attr} value"));
    }
    let out = ctx
        .cognito
        .list_users()
        .user_pool_id(&ctx.pool_id)
        .filter(format!("{attr} = \"{value}\""))
        .limit(1)
        .send()
        .await
        .map_err(|e| format!("user lookup failed: {e}"))?;
    Ok(out.users().first().and_then(user_from))
}

/// Create the Cognito user for a first-time Apple sign-in: email as username,
/// pre-verified, no invite mail — there is no password to deliver.
///
/// Admin-created users land in FORCE_CHANGE_PASSWORD, which blocks auth, so a
/// random password is set permanent immediately (generated and discarded —
/// nobody ever knows it) to move the user to CONFIRMED. "Forgot password"
/// still works against the verified email if they ever want a real one.
pub async fn create_user(ctx: &Ctx, email: &str) -> Result<User, String> {
    let attr = |name: &str, value: &str| {
        AttributeType::builder()
            .name(name)
            .value(value)
            .build()
            .map_err(|e| format!("attribute build failed: {e}"))
    };
    let out = ctx
        .cognito
        .admin_create_user()
        .user_pool_id(&ctx.pool_id)
        .username(email)
        .user_attributes(attr("email", email)?)
        .user_attributes(attr("email_verified", "true")?)
        .message_action(MessageActionType::Suppress)
        .send()
        .await
        .map_err(|e| format!("user create failed: {e}"))?;
    let user = out
        .user()
        .and_then(user_from)
        .ok_or_else(|| "user create returned no user".to_owned())?;

    ctx.cognito
        .admin_set_user_password()
        .user_pool_id(&ctx.pool_id)
        .username(&user.username)
        .password(discarded_password()?)
        .permanent(true)
        .send()
        .await
        .map_err(|e| format!("user activation failed: {e}"))?;

    Ok(User {
        unconfirmed: false,
        ..user
    })
}

/// Confirm a stalled self-signup (see [`User::unconfirmed`]).
pub async fn confirm_user(ctx: &Ctx, username: &str) -> Result<(), String> {
    ctx.cognito
        .admin_confirm_sign_up()
        .user_pool_id(&ctx.pool_id)
        .username(username)
        .send()
        .await
        .map_err(|e| format!("user confirm failed: {e}"))?;
    Ok(())
}

/// 32 bytes of OS randomness as hex, plus an uppercase letter to satisfy the
/// pool's password policy. Never stored, never returned to anyone.
fn discarded_password() -> Result<String, String> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("no OS randomness: {e}"))?;
    let hex = bytes.iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    });
    Ok(format!("A{hex}"))
}

/// Run the pool's CUSTOM_AUTH flow for `username`, answering the challenge
/// with the Apple identity token. The VerifyAuthChallenge trigger (this same
/// binary) re-verifies the token and its link to exactly this user, so these
/// admin calls add reach, not trust.
pub async fn custom_auth(ctx: &Ctx, username: &str, identity_token: &str) -> Result<Tokens, String> {
    let start = ctx
        .cognito
        .admin_initiate_auth()
        .user_pool_id(&ctx.pool_id)
        .client_id(&ctx.client_id)
        .auth_flow(AuthFlowType::CustomAuth)
        .auth_parameters("USERNAME", username)
        .send()
        .await
        .map_err(|e| format!("auth initiate failed: {e}"))?;

    if start.challenge_name() != Some(&ChallengeNameType::CustomChallenge) {
        return Err(format!(
            "expected CUSTOM_CHALLENGE, got {:?}",
            start.challenge_name()
        ));
    }
    let session = start.session().ok_or("auth initiate returned no session")?;

    let done = ctx
        .cognito
        .admin_respond_to_auth_challenge()
        .user_pool_id(&ctx.pool_id)
        .client_id(&ctx.client_id)
        .challenge_name(ChallengeNameType::CustomChallenge)
        .session(session)
        .challenge_responses("USERNAME", username)
        .challenge_responses("ANSWER", identity_token)
        .send()
        .await
        .map_err(|e| format!("challenge response failed: {e}"))?;

    let result = done
        .authentication_result()
        .ok_or("challenge did not issue tokens")?;
    let s = |v: Option<&str>, what: &str| -> Result<String, String> {
        v.map(str::to_owned)
            .ok_or_else(|| format!("auth result missing {what}"))
    };
    Ok(Tokens {
        id_token: s(result.id_token(), "id token")?,
        access_token: s(result.access_token(), "access token")?,
        refresh_token: s(result.refresh_token(), "refresh token")?,
        expires_in: result.expires_in(),
    })
}

fn user_from(user: &UserType) -> Option<User> {
    let username = user.username()?.to_owned();
    let sub = user
        .attributes()
        .iter()
        .find(|a| a.name() == "sub")
        .and_then(|a| a.value())
        .map(str::to_owned)?;
    Some(User {
        username,
        sub,
        unconfirmed: user.user_status() == Some(&UserStatusType::Unconfirmed),
    })
}
