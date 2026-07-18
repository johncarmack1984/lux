//! lux-node — headless lux for an always-on Linux box.
//!
//!   sudo lux-node install      one-command setup: binary, user, unit, config,
//!                              login, enable (idempotent; --keep-sleep to
//!                              skip masking suspend)
//!   lux-node login <email>     sign in once; stores the refresh token (0600)
//!   lux-node run [--config P]  hold the user channel and drive the rig
//!
//! Config file (default ~/.config/lux-node/config.json):
//!   { "setupId": "<uuid>", "universe": 1, "interface": null, "priority": 90 }

mod auth;
mod config;
mod install;
mod node;
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
        Some("run") | None => run(config_path(&args)?),
        Some(other) => Err(format!(
            "unknown command {other}; usage: lux-node install | lux-node login [<email>] | lux-node run [--config <path>]"
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

fn config_path(args: &[String]) -> Result<PathBuf, String> {
    if let Some(i) = args.iter().position(|a| a == "--config") {
        return args
            .get(i + 1)
            .map(PathBuf::from)
            .ok_or("--config needs a path".to_owned());
    }
    config::default_config_path()
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
    })?;
    println!("signed in as {email}; session stored. Next: lux-node run");
    Ok(())
}

fn run(config_file: PathBuf) -> Result<(), String> {
    let env = config::endpoints()?;
    let cfg = config::load_node_config(&config_file)?;
    let session = config::load_session()?;

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
