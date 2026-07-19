//! lux-node — headless lux for an always-on Linux box.
//!
//!   sudo lux-node install      one-command setup: binary, user, unit, config,
//!                              login, enable (idempotent; --keep-sleep to
//!                              skip masking suspend)
//!   lux-node login <email>     sign in once; stores the refresh token (0600)
//!   lux-node pair              claim a headless box from the lux app — prints
//!                              a code, blocks until approved (no password)
//!   lux-node run [--config P]  hold the user channel and drive the rig; an
//!                              unpaired box waits to be claimed instead of dying
//!
//! Config file (default ~/.config/lux-node/config.json):
//!   { "setupId": "<uuid>", "universe": 1, "interface": null, "priority": 90 }

mod auth;
mod config;
mod install;
mod node;
mod pairing;
mod setups;

use std::net::Ipv4Addr;
use std::path::PathBuf;

use lux_engine::sacn::SacnSink;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // rumqttc's rustls has no baked-in crypto provider; install ring as the
    // process default before any TLS happens (same as the Lambda services).
    let _ = rustls::crypto::ring::default_provider().install_default();

    if let Err(e) = dispatch() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn dispatch() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("install") => install::install(install::Options::parse(&args[1..])?),
        Some("login") => {
            // `login [<email>] [--password-stdin]` — email positional or
            // --email / $LUX_NODE_EMAIL, password interactive or non-interactive
            // (same rules as install), so login scripts too.
            let opts = install::Options::parse(&login_args(&args[1..]))?;
            login(opts)
        }
        Some("pair") => pair(),
        Some("run") | None => run(explicit_config(&args[..])?),
        Some(other) => Err(format!(
            "unknown command {other}; usage: lux-node install | lux-node login [<email>] | lux-node pair | lux-node run [--config <path>]"
        )),
    }
}

/// Let `login <email>` keep its positional-email ergonomics while reusing the
/// installer's option parser: a bare first arg becomes `--email <it>`.
fn login_args(args: &[String]) -> Vec<String> {
    match args.first() {
        Some(first) if !first.starts_with("--") => {
            let mut rest = vec!["--email".to_owned(), first.clone()];
            rest.extend_from_slice(&args[1..]);
            rest
        }
        _ => args.to_vec(),
    }
}

/// The `--config <path>` the operator passed, if any. `None` (no flag) is
/// distinct from a flag pointing at a missing file: only an explicit,
/// *existing* file outranks the paired state-dir binding in `run`.
fn explicit_config(args: &[String]) -> Result<Option<PathBuf>, String> {
    match args.iter().position(|a| a == "--config") {
        Some(i) => args
            .get(i + 1)
            .map(|p| Some(PathBuf::from(p)))
            .ok_or("--config needs a path".to_owned()),
        None => Ok(None),
    }
}

fn login(opts: install::Options) -> Result<(), String> {
    let env = config::endpoints()?;
    let email = opts
        .email
        .clone()
        .filter(|e| !e.is_empty())
        .ok_or("usage: lux-node login <email> (or --email / $LUX_NODE_EMAIL)")?;
    let password = install::read_password(&opts)?;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let tokens = runtime.block_on(auth::sign_in(&env, &email, &password))?;
    let refresh = tokens
        .refresh
        .ok_or("Cognito returned no refresh token; cannot run headless")?;
    config::save_session(&config::StoredSession {
        email: email.clone(),
        refresh_token: refresh,
        client_id: None,
    })?;
    println!("signed in as {email}; session stored. Next: lux-node run");
    Ok(())
}

/// `lux-node pair` — the human-with-a-shell path: register, print the code,
/// block until approved, and write the same files the unpaired-boot state
/// machine writes. `run` then finds the session and binding and just works.
fn pair() -> Result<(), String> {
    let env = config::endpoints()?;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    if config::session_exists()? {
        return Err(
            "already paired (session.json exists); delete it to re-pair, or just run lux-node run"
                .into(),
        );
    }
    let granted = runtime.block_on(pairing::pair_wait(&env, pairing::stdout_announce))?;
    config::save_session(&granted.session)?;
    config::save_node_binding(&granted.setup_id, granted.universe)?;
    println!(
        "paired as {}; setup {} on universe {}. Next: lux-node run",
        granted.session.email, granted.setup_id, granted.universe
    );
    Ok(())
}

fn run(explicit_config: Option<PathBuf>) -> Result<(), String> {
    let env = config::endpoints()?;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;

    // Unpaired boot: no session means wait to be claimed from the app rather
    // than die. The grant persists exactly what `login` + `install` would.
    let session = if config::session_exists()? {
        config::load_session()?
    } else {
        log::info!("no stored session; waiting to be paired from the lux app");
        let granted = runtime.block_on(pairing::pair_wait(&env, pairing::journal_announce))?;
        config::save_session(&granted.session)?;
        config::save_node_binding(&granted.setup_id, granted.universe)?;
        log::info!(
            "paired as {}; setup {} on universe {}",
            granted.session.email,
            granted.setup_id,
            granted.universe
        );
        granted.session
    };

    // Setup-binding precedence: an explicit, existing `--config` (today's
    // installs) → the paired state-dir `node.json` → nothing (a session with
    // no binding, e.g. a bare `login` — surfaced as a clear error).
    let node_json = config::node_binding_path()?;
    let explicit_exists = explicit_config.as_ref().is_some_and(|p| p.exists());
    let config_file = match config::binding_choice(explicit_exists, node_json.exists()) {
        config::Binding::Explicit => {
            explicit_config.ok_or("internal: explicit binding chosen without a path")?
        }
        config::Binding::StateDir => node_json,
        config::Binding::None => {
            return Err(format!(
                "no setup binding: pass --config <file> or pair the device \
                 (a paired binding is written to {})",
                node_json.display()
            ))
        }
    };
    let cfg = config::load_node_config(&config_file)?;

    let interface = match &cfg.interface {
        Some(ip) => Some(
            ip.parse::<Ipv4Addr>()
                .map_err(|e| format!("bad interface {ip}: {e}"))?,
        ),
        None => None,
    };
    let name = format!(
        "lux-node ({})",
        gethostname::gethostname().to_string_lossy()
    );
    let sink = SacnSink::new(cfg.universe, interface, cfg.priority, &name)?;
    log::info!(
        "lux-node: setup {} -> sACN universe {} at priority {} ({})",
        cfg.setup_id,
        cfg.universe,
        cfg.priority,
        session.email
    );

    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(node::run(env, cfg, session, sink))
}
