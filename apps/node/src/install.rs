//! `sudo lux-node install` — the binary installs itself as a systemd service.
//!
//! Idempotent: every step checks before it acts, so re-running upgrades the
//! binary in place and fixes whatever is missing. All the sysadmin choreography
//! (service user, dirs, unit, config, login-as-the-service-identity, enable)
//! lives here so the runbook is two commands: download, `sudo ./lux-node
//! install`.

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::Command;

use crate::auth;
use crate::config::{self, StoredSession};
use crate::setups;

const BIN_PATH: &str = "/usr/local/bin/lux-node";
const ETC_DIR: &str = "/etc/lux-node";
const STATE_DIR: &str = "/var/lib/lux-node";
const UNIT_PATH: &str = "/etc/systemd/system/lux-node.service";
const SERVICE_USER: &str = "lux-node";
const UNIT: &str = include_str!("../lux-node.service");

/// Everything the installer needs, so it can run unattended: each field has a
/// flag and (for the two secrets-adjacent ones) an env var, and only when a
/// value is *absent and stdin is a real terminal* does the installer prompt.
/// A pipe or an automation harness that isn't a controlling tty therefore
/// never wedges on a prompt — it gets a clear "pass --x / set LUX_NODE_X"
/// error instead. (rpassword opens /dev/tty directly, so it is only ever
/// called on the interactive path.)
#[derive(Debug, Default)]
pub struct Options {
    pub email: Option<String>,
    pub password: Option<String>,
    pub password_stdin: bool,
    pub setup_id: Option<String>,
    pub setup_name: Option<String>,
    pub universe: Option<u16>,
    pub keep_sleep: bool,
}

impl Options {
    /// Parse `install`'s args (after the subcommand) and the `LUX_NODE_*`
    /// env vars. Flags: `--email`, `--password-stdin`, `--setup-id`,
    /// `--setup <name>`, `--universe`, `--keep-sleep`.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut opts = Options {
            email: std::env::var("LUX_NODE_EMAIL")
                .ok()
                .filter(|s| !s.is_empty()),
            password: std::env::var("LUX_NODE_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty()),
            ..Default::default()
        };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || {
                it.next()
                    .cloned()
                    .ok_or_else(|| format!("{arg} needs a value"))
            };
            match arg.as_str() {
                "--email" => opts.email = Some(value()?),
                "--password-stdin" => opts.password_stdin = true,
                "--setup-id" => opts.setup_id = Some(value()?),
                "--setup" => opts.setup_name = Some(value()?),
                "--universe" => {
                    let v = value()?;
                    opts.universe =
                        Some(v.parse().map_err(|e| format!("bad --universe {v}: {e}"))?);
                }
                "--keep-sleep" => opts.keep_sleep = true,
                other => return Err(format!("unknown install flag {other}")),
            }
        }
        Ok(opts)
    }
}

pub fn install(opts: Options) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("lux-node install targets Linux (systemd)".into());
    }
    if !is_root() {
        return Err("run as root: sudo ./lux-node install".into());
    }

    // 1. The binary itself.
    let me = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    if me != Path::new(BIN_PATH) {
        fs::copy(&me, BIN_PATH).map_err(|e| format!("install {BIN_PATH}: {e}"))?;
        run("chmod", &["755", BIN_PATH])?;
        println!("installed {BIN_PATH}");
    }

    // 2. Service user + dirs.
    if !run_ok("id", &["-u", SERVICE_USER]) {
        run(
            "useradd",
            &[
                "--system",
                "--home",
                STATE_DIR,
                "--shell",
                "/usr/sbin/nologin",
                SERVICE_USER,
            ],
        )?;
        println!("created system user {SERVICE_USER}");
    }
    fs::create_dir_all(ETC_DIR).map_err(|e| format!("mkdir {ETC_DIR}: {e}"))?;
    fs::create_dir_all(STATE_DIR).map_err(|e| format!("mkdir {STATE_DIR}: {e}"))?;
    run("chown", &["-R", SERVICE_USER, STATE_DIR])?;

    // 3. Sign in as the service identity (reuse a stored session when one
    //    exists) — sign-in comes before config so the setup picker below can
    //    ask the sync API instead of making a human type a UUID.
    let env = config::endpoints()?;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    std::env::set_var("XDG_CONFIG_HOME", STATE_DIR);
    let session_file = format!("{STATE_DIR}/lux-node/session.json");
    let id_token = if Path::new(&session_file).exists() {
        let session = config::load_session()?;
        runtime
            .block_on(auth::refresh(&env, &session.refresh_token))?
            .id
    } else {
        let email = value_or_prompt(opts.email.clone(), "lux account email", "--email")?;
        let password = read_password(&opts)?;
        let tokens = runtime.block_on(auth::sign_in(&env, &email, &password))?;
        let refresh = tokens
            .refresh
            .ok_or("Cognito returned no refresh token; cannot run headless")?;
        // Write via the same path the service reads (XDG under the state
        // dir), then hand the file to the service user.
        config::save_session(&StoredSession {
            email: email.clone(),
            refresh_token: refresh,
        })?;
        run("chown", &["-R", SERVICE_USER, STATE_DIR])?;
        println!("signed in as {email}");
        tokens.id
    };

    // 4. Config: pick the setup from the account (name + universe come from
    //    the sync record); a UUID prompt is only the unreachable-API fallback.
    //    Prompt only when no config exists — rerunning never clobbers.
    let config_path = format!("{ETC_DIR}/config.json");
    if !Path::new(&config_path).exists() {
        let setups = runtime.block_on(setups::list(&env, &id_token));
        let (setup_id, universe) = resolve_setup(&opts, setups)?;
        let json = serde_json::json!({ "setupId": setup_id, "universe": universe });
        fs::write(&config_path, format!("{:#}\n", json))
            .map_err(|e| format!("write {config_path}: {e}"))?;
        println!("using setup {setup_id} on universe {universe}");
        println!("wrote {config_path}");
    }

    // 5. Unit file (embedded in the binary, so they can't drift apart).
    fs::write(UNIT_PATH, UNIT).map_err(|e| format!("write {UNIT_PATH}: {e}"))?;

    // 6. Enable + start.
    run("systemctl", &["daemon-reload"])?;
    run("systemctl", &["enable", "--now", "lux-node"])?;

    // 7. An always-on box should stay on (opt out with --keep-sleep).
    if !opts.keep_sleep {
        run(
            "systemctl",
            &[
                "mask",
                "sleep.target",
                "suspend.target",
                "hibernate.target",
                "hybrid-sleep.target",
            ],
        )?;
        println!("sleep/suspend masked (rerun with --keep-sleep to skip this)");
    }

    println!("lux-node is running. Watch it: journalctl -u lux-node -f");
    Ok(())
}

/// Decide which setup this node applies, from flags first, then the fetched
/// list, then (only at a terminal) an interactive pick. `universe` prefers
/// `--universe`, else the matched record's, else 1.
fn resolve_setup(
    opts: &Options,
    setups: Result<Vec<lux_wire::SetupRecord>, String>,
) -> Result<(String, u16), String> {
    // Explicit id wins outright; universe from the flag, or the record if the
    // list came back, or 1.
    if let Some(id) = &opts.setup_id {
        let universe = opts.universe.unwrap_or_else(|| {
            setups
                .as_ref()
                .ok()
                .and_then(|list| list.iter().find(|s| &s.id == id))
                .map(|s| s.universe)
                .unwrap_or(1)
        });
        return Ok((id.clone(), universe));
    }

    let list = match setups {
        Ok(list) if !list.is_empty() => list,
        Ok(_) => {
            return Err(
                "no setups on this account yet — create one in the app, then pass --setup-id"
                    .into(),
            )
        }
        Err(e) => {
            return Err(format!(
                "could not list setups ({e}); pass --setup-id <uuid> [--universe <n>] to proceed"
            ))
        }
    };

    // Resolve `--setup <name>` against the list (exact, case-insensitive).
    if let Some(name) = &opts.setup_name {
        let matches: Vec<&lux_wire::SetupRecord> = list
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case(name))
            .collect();
        return match matches.as_slice() {
            [one] => Ok((one.id.clone(), opts.universe.unwrap_or(one.universe))),
            [] => Err(format!("no setup named {name:?}; {}", available(&list))),
            _ => Err(format!(
                "{} setups named {name:?} — disambiguate with --setup-id",
                matches.len()
            )),
        };
    }

    // Nothing specified: pick interactively, or explain the flag if headless.
    if !std::io::stdin().is_terminal() {
        return Err(format!(
            "no setup chosen and not a terminal; pass --setup-id <uuid> or --setup <name>. {}",
            available(&list)
        ));
    }
    pick_setup(&list, opts.universe)
}

/// A one-line summary of the account's setups for error messages.
fn available(list: &[lux_wire::SetupRecord]) -> String {
    let names: Vec<String> = list
        .iter()
        .map(|s| format!("{} ({})", s.name, &s.id[..8.min(s.id.len())]))
        .collect();
    format!("available: {}", names.join(", "))
}

/// Numbered pick over the account's setups; returns (id, universe). The short
/// id disambiguates same-named setups.
fn pick_setup(
    setups: &[lux_wire::SetupRecord],
    universe_override: Option<u16>,
) -> Result<(String, u16), String> {
    println!("setups on this account:");
    for (i, setup) in setups.iter().enumerate() {
        let short: String = setup.id.chars().take(8).collect();
        println!(
            "  {}. {} — universe {} ({short})",
            i + 1,
            setup.name,
            setup.universe
        );
    }
    let choice = prompt(&format!("apply which setup [1-{}]", setups.len()))?;
    let index: usize = choice
        .parse()
        .ok()
        .filter(|n| (1..=setups.len()).contains(n))
        .ok_or_else(|| format!("pick a number between 1 and {}", setups.len()))?;
    let picked = &setups[index - 1];
    Ok((
        picked.id.clone(),
        universe_override.unwrap_or(picked.universe),
    ))
}

/// A flag/env value if present, else an interactive prompt, else a clear error
/// naming the flag to set — so a non-terminal run fails loud, never hangs.
fn value_or_prompt(value: Option<String>, label: &str, flag: &str) -> Result<String, String> {
    if let Some(v) = value.filter(|v| !v.is_empty()) {
        return Ok(v);
    }
    if std::io::stdin().is_terminal() {
        return prompt(label);
    }
    Err(format!(
        "{label} not provided and not a terminal; pass {flag}"
    ))
}

/// The password from `--password-stdin` / `LUX_NODE_PASSWORD` / an interactive
/// prompt (in that order). rpassword opens /dev/tty, so it is reached only
/// when stdin is a real terminal.
pub fn read_password(opts: &Options) -> Result<String, String> {
    if let Some(pw) = opts.password.clone().filter(|p| !p.is_empty()) {
        return Ok(pw);
    }
    if opts.password_stdin {
        let mut pw = String::new();
        std::io::stdin()
            .read_line(&mut pw)
            .map_err(|e| format!("read password from stdin: {e}"))?;
        return Ok(pw.trim_end_matches(['\r', '\n']).to_owned());
    }
    if std::io::stdin().is_terminal() {
        return rpassword::prompt_password("password: ")
            .map_err(|e| format!("password prompt: {e}"));
    }
    Err("no password: set LUX_NODE_PASSWORD, pass --password-stdin, or run in a terminal".into())
}

fn is_root() -> bool {
    run_out("id", &["-u"]).is_some_and(|out| out.trim() == "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn record(id: &str, name: &str, universe: u16) -> lux_wire::SetupRecord {
        lux_wire::SetupRecord {
            id: id.into(),
            name: name.into(),
            universe,
            fixtures: serde_json::json!([]),
            rev: 0,
            updated_at: 0,
            deleted: false,
        }
    }

    #[test]
    fn parses_flags() {
        let o = Options::parse(&args(&[
            "--email",
            "a@b.com",
            "--setup",
            "Home",
            "--universe",
            "3",
            "--keep-sleep",
            "--password-stdin",
        ]))
        .unwrap();
        assert_eq!(o.email.as_deref(), Some("a@b.com"));
        assert_eq!(o.setup_name.as_deref(), Some("Home"));
        assert_eq!(o.universe, Some(3));
        assert!(o.keep_sleep);
        assert!(o.password_stdin);
        assert!(Options::parse(&args(&["--nope"])).is_err());
        assert!(Options::parse(&args(&["--email"])).is_err()); // missing value
    }

    #[test]
    fn setup_id_flag_wins_without_the_network() {
        let o = Options {
            setup_id: Some("abc".into()),
            universe: Some(7),
            ..Default::default()
        };
        // Even with the list unavailable, an explicit id resolves.
        assert_eq!(
            resolve_setup(&o, Err("offline".into())).unwrap(),
            ("abc".into(), 7)
        );
        // Universe falls back to the matched record when not given.
        let o = Options {
            setup_id: Some("abc".into()),
            ..Default::default()
        };
        let list = Ok(vec![record("abc", "Home", 4)]);
        assert_eq!(resolve_setup(&o, list).unwrap(), ("abc".into(), 4));
    }

    #[test]
    fn setup_name_resolves_and_flags_ambiguity() {
        let list = vec![record("id1", "Home", 1), record("id2", "Home", 1)];
        let one = Options {
            setup_name: Some("home".into()), // case-insensitive
            ..Default::default()
        };
        // Two "Home"s → refuse rather than guess.
        assert!(resolve_setup(&one, Ok(list.clone())).is_err());

        let unique = vec![record("id1", "Church", 2)];
        let o = Options {
            setup_name: Some("Church".into()),
            ..Default::default()
        };
        assert_eq!(resolve_setup(&o, Ok(unique)).unwrap(), ("id1".into(), 2));
    }

    #[test]
    fn no_setup_and_no_terminal_errors_instead_of_prompting() {
        // Nothing specified + a list present: the terminal check gates the
        // picker, so under `cargo test` (no tty) this must be an error, never
        // a hang.
        let o = Options::default();
        let err = resolve_setup(&o, Ok(vec![record("id1", "Home", 1)])).unwrap_err();
        assert!(err.contains("--setup-id") || err.contains("--setup"));
    }
}

fn prompt(label: &str) -> Result<String, String> {
    print!("{label}: ");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim().to_owned())
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd} {} failed ({status})", args.join(" ")))
    }
}

fn run_ok(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_out(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
