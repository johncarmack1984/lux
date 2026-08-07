//! What can go wrong, in the shape the caller has to act on.
//!
//! The follow engine treats every failure identically (hold the lights), so
//! these variants exist for the *poller*, which does have to tell them apart:
//! [`Error::Unauthorized`] means refresh the token and retry,
//! [`Error::RateLimited`] means wait exactly as long as the server said, and
//! the rest mean try again on the next tick.
//!
//! Nothing here ever carries a token or a client secret. A message that leaks
//! one ends up in CloudWatch forever.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// A credential the crate needs was not supplied. Carries the name of the
    /// thing to set, never its value.
    NotConfigured(&'static str),
    /// The request never got an answer: DNS, TLS, timeout, reset.
    Transport(String),
    /// 401: the access token is expired or revoked. Refresh, then retry once.
    Unauthorized,
    /// 429: back off for `retry_after_s` seconds — the value the server sent,
    /// not a guess.
    RateLimited { retry_after_s: Option<u64> },
    /// Any other non-2xx. `detail` is Planning Center's own error body,
    /// truncated.
    Status { status: u16, detail: String },
    /// A 2xx whose body wasn't the shape we expect.
    Decode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotConfigured(what) => write!(f, "{what} is not set"),
            Error::Transport(e) => write!(f, "planning center unreachable: {e}"),
            Error::Unauthorized => write!(f, "planning center rejected the token"),
            Error::RateLimited {
                retry_after_s: None,
            } => {
                write!(f, "planning center rate limit reached")
            }
            Error::RateLimited {
                retry_after_s: Some(s),
            } => write!(f, "planning center rate limit reached; retry in {s}s"),
            Error::Status { status, detail } => {
                write!(f, "planning center returned {status}: {detail}")
            }
            Error::Decode(e) => write!(f, "unexpected planning center response: {e}"),
        }
    }
}

impl std::error::Error for Error {}
