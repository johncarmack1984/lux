//! `sudo lux-node install` — the binary installs itself as a system service: a
//! systemd unit on Linux, a launchd daemon on macOS.
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
use std::time::{Duration, Instant};

use crate::auth;
use crate::config::{self, StoredSession};
use crate::pairing;
use crate::setups;

const BIN_PATH: &str = "/usr/local/bin/lux-node";
const ETC_DIR: &str = "/etc/lux-node";
const CONFIG_PATH: &str = "/etc/lux-node/config.json";
const STATE_DIR: &str = "/var/lib/lux-node";
const UNIT_PATH: &str = "/etc/systemd/system/lux-node.service";
const SERVICE_USER: &str = "lux-node";
const UNIT: &str = include_str!("../lux-node.service");

// macOS: a launchd daemon, not an agent — macOS exempts launchd daemons from
// Local Network privacy, and sACN multicast is a local network operation.
const MAC_STATE_DIR: &str = "/Library/Application Support/lux-node";
const MAC_LOG_DIR: &str = "/Library/Logs/lux-node";
const MAC_LOG_FILE: &str = "/Library/Logs/lux-node/lux-node.log";
/// A hidden role account (plus a group of the same name) the daemon runs as.
const MAC_SERVICE_USER: &str = "_luxnode";
/// `sysadminctl` reserves 450–499 for role accounts; Apple's own accounts sit
/// below it and keep growing into the 300s.
const MAC_ROLE_ID_FIRST: u32 = 450;
const MAC_ROLE_ID_LAST: u32 = 499;
const LAUNCHD_LABEL: &str = "com.johncarmack.lux-node";
const PLIST_PATH: &str = "/Library/LaunchDaemons/com.johncarmack.lux-node.plist";
const PLIST: &str = include_str!("../com.johncarmack.lux-node.plist");
/// The line in [`PLIST`] that install replaces with the program arguments.
const PLIST_ARGS_SLOT: &str = "\t\t<!-- program arguments: written by lux-node install -->";
/// How long to wait for a running daemon to leave launchd before starting the
/// new one: past launchd's default 20 s exit timeout, after which it SIGKILLs.
const LAUNCHD_STOP_WAIT: Duration = Duration::from_secs(25);

/// Which service manager the install targets. Dispatched at runtime on
/// `cfg!(target_os)`, so both platforms' paths compile, lint, and test on
/// either one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Linux,
    MacOs,
}

impl Platform {
    fn current() -> Result<Self, String> {
        if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else if cfg!(target_os = "macos") {
            Ok(Self::MacOs)
        } else {
            Err("lux-node install supports Linux (systemd) and macOS (launchd)".into())
        }
    }

    fn state_dir(self) -> &'static str {
        match self {
            Self::Linux => STATE_DIR,
            Self::MacOs => MAC_STATE_DIR,
        }
    }

    /// `chown`'s owner argument for the files the service writes.
    fn owner(self) -> String {
        match self {
            Self::Linux => SERVICE_USER.to_owned(),
            Self::MacOs => format!("{MAC_SERVICE_USER}:{MAC_SERVICE_USER}"),
        }
    }
}

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
    /// Claim the box from the lux app instead of signing in with a password:
    /// the device session it mints lasts 10 years (a password session, 30
    /// days), and Sign in with Apple accounts have no password at all.
    pub pair: bool,
}

impl Options {
    /// Parse `install`'s args (after the subcommand) and the `LUX_NODE_*`
    /// env vars. Flags: `--email`, `--password-stdin`, `--setup-id`,
    /// `--setup <name>`, `--universe`, `--keep-sleep`, `--pair`.
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
                "--pair" => opts.pair = true,
                other => return Err(format!("unknown install flag {other}")),
            }
        }
        Ok(opts)
    }
}

pub fn install(opts: Options) -> Result<(), String> {
    let platform = Platform::current()?;
    if !is_root() {
        return Err("run as root: sudo ./lux-node install".into());
    }

    // 1. The binary itself (a fresh Mac may not have /usr/local/bin yet).
    let me = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let replaced = me != Path::new(BIN_PATH);
    if replaced {
        if let Some(dir) = Path::new(BIN_PATH).parent() {
            fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        }
        replace_binary(&me, Path::new(BIN_PATH))?;
        println!("installed {BIN_PATH}");
    }

    // 2. Service user + dirs.
    let state_dir = platform.state_dir();
    let owner = platform.owner();
    match platform {
        Platform::Linux => ensure_linux_user()?,
        Platform::MacOs => ensure_mac_role_account()?,
    }
    fs::create_dir_all(ETC_DIR).map_err(|e| format!("mkdir {ETC_DIR}: {e}"))?;
    fs::create_dir_all(state_dir).map_err(|e| format!("mkdir {state_dir}: {e}"))?;
    run("chown", &["-R", &owner, state_dir])?;
    if platform == Platform::MacOs {
        // launchd appends the daemon's stdout/stderr here (no journald).
        fs::create_dir_all(MAC_LOG_DIR).map_err(|e| format!("mkdir {MAC_LOG_DIR}: {e}"))?;
        run("chown", &["-R", &owner, MAC_LOG_DIR])?;
    }

    // 3. Sign in as the service identity (reuse a stored session when one
    //    exists, or claim the box from the app with --pair) — sign-in comes
    //    before config so the setup picker below can ask the sync API instead
    //    of making a human type a UUID.
    let env = config::endpoints()?;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    std::env::set_var("XDG_CONFIG_HOME", state_dir);
    let id_token = if config::session_exists()? {
        if opts.pair {
            println!(
                "a session is already stored; --pair skipped (delete {} to pair again)",
                config::session_path()?.display()
            );
        }
        let session = config::load_session()?;
        Some(
            runtime
                .block_on(auth::refresh(
                    &env,
                    session.client_id.as_deref(),
                    &session.refresh_token,
                ))?
                .id,
        )
    } else if opts.pair {
        // The approver picks the setup in the app, so the grant carries the
        // binding: it lands in the state dir's node.json, which `run` falls
        // back to while no /etc config exists.
        let granted = runtime.block_on(pairing::pair_wait(&env, pairing::stdout_announce))?;
        config::save_session(&granted.session)?;
        config::save_node_binding(&granted.setup_id, granted.universe)?;
        run("chown", &["-R", &owner, state_dir])?;
        println!(
            "paired as {}; setup {} on universe {}",
            granted.session.email, granted.setup_id, granted.universe
        );
        None
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
            client_id: None,
        })?;
        run("chown", &["-R", &owner, state_dir])?;
        println!("signed in as {email}");
        Some(tokens.id)
    };

    // 4. Config: pick the setup from the account (name + universe come from
    //    the sync record); a UUID prompt is only the unreachable-API fallback.
    //    Prompt only when no binding exists — neither the /etc config nor a
    //    paired node.json — so rerunning never clobbers or rebinds.
    let paired = config::node_binding_path()?.exists();
    if !Path::new(CONFIG_PATH).exists() && !paired {
        let id_token = id_token.ok_or("no session to list setups with; rerun install")?;
        let setups = runtime.block_on(setups::list(&env, &id_token));
        let (setup_id, universe) = resolve_setup(&opts, setups)?;
        let json = serde_json::json!({ "setupId": setup_id, "universe": universe });
        fs::write(CONFIG_PATH, format!("{:#}\n", json))
            .map_err(|e| format!("write {CONFIG_PATH}: {e}"))?;
        println!("using setup {setup_id} on universe {universe}");
        println!("wrote {CONFIG_PATH}");
    }

    // 5–6. The service definition (embedded in the binary, so they can't
    //      drift apart), then start it — onto the new binary after an upgrade.
    match platform {
        Platform::Linux => start_systemd(replaced)?,
        Platform::MacOs => start_launchd(!opts.keep_sleep, replaced)?,
    }

    // 7. An always-on box should stay on (opt out with --keep-sleep).
    if !opts.keep_sleep {
        match platform {
            Platform::Linux => {
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
            // Already done: the daemon's own caffeinate assertion (see PLIST).
            Platform::MacOs => println!(
                "the Mac stays awake on AC power while lux-node runs (rerun with --keep-sleep to skip this)"
            ),
        }
    }

    match platform {
        Platform::Linux => println!("lux-node is running. Watch it: journalctl -u lux-node -f"),
        Platform::MacOs => println!("lux-node is running. Watch it: tail -f {MAC_LOG_FILE}"),
    }
    Ok(())
}

fn ensure_linux_user() -> Result<(), String> {
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
    Ok(())
}

/// Write the unit and enable + start it. `enable --now` leaves an
/// already-running service alone, so after an upgrade restart it onto the new
/// binary.
fn start_systemd(replaced: bool) -> Result<(), String> {
    fs::write(UNIT_PATH, UNIT).map_err(|e| format!("write {UNIT_PATH}: {e}"))?;
    run("systemctl", &["daemon-reload"])?;
    if replaced {
        run("systemctl", &["enable", "lux-node"])?;
        run("systemctl", &["restart", "lux-node"])?;
    } else {
        run("systemctl", &["enable", "--now", "lux-node"])?;
    }
    Ok(())
}

/// Create the hidden role account (and its same-named group) the daemon runs
/// as, unless the account already exists. `dscl -create` replaces values, so
/// a run that died partway through is finished by the next one.
fn ensure_mac_role_account() -> Result<(), String> {
    if run_ok("id", &["-u", MAC_SERVICE_USER]) {
        return Ok(());
    }
    let users = run_out("dscl", &[".", "-list", "/Users", "UniqueID"])
        .ok_or("dscl could not list the local users")?;
    let groups = run_out("dscl", &[".", "-list", "/Groups", "PrimaryGroupID"])
        .ok_or("dscl could not list the local groups")?;
    let id = free_role_id(&dscl_ids(&users), &dscl_ids(&groups))
        .ok_or_else(|| {
            format!(
                "no free id in {MAC_ROLE_ID_FIRST}–{MAC_ROLE_ID_LAST} for the {MAC_SERVICE_USER} role account"
            )
        })?
        .to_string();

    let group = format!("/Groups/{MAC_SERVICE_USER}");
    run("dscl", &[".", "-create", &group])?;
    for (key, value) in [
        ("PrimaryGroupID", id.as_str()),
        ("RealName", "lux-node"),
        ("Password", "*"),
    ] {
        run("dscl", &[".", "-create", &group, key, value])?;
    }
    let user = format!("/Users/{MAC_SERVICE_USER}");
    run("dscl", &[".", "-create", &user])?;
    for (key, value) in [
        ("UniqueID", id.as_str()),
        ("PrimaryGroupID", id.as_str()),
        ("UserShell", "/usr/bin/false"),
        ("NFSHomeDirectory", "/var/empty"),
        ("RealName", "lux-node"),
        ("Password", "*"),
        ("IsHidden", "1"),
    ] {
        run("dscl", &[".", "-create", &user, key, value])?;
    }
    if !run_ok("id", &["-u", MAC_SERVICE_USER]) {
        return Err(format!(
            "created {MAC_SERVICE_USER} but it does not resolve yet; rerun install"
        ));
    }
    println!("created role account {MAC_SERVICE_USER} (uid {id})");
    Ok(())
}

/// The numeric column of `dscl . -list <dir> <attribute>` output.
fn dscl_ids(listing: &str) -> Vec<u32> {
    listing
        .lines()
        .filter_map(|line| line.split_whitespace().last()?.parse().ok())
        .collect()
}

/// The first role-account id used by neither a user nor a group, so the
/// account and its group share one number.
fn free_role_id(uids: &[u32], gids: &[u32]) -> Option<u32> {
    (MAC_ROLE_ID_FIRST..=MAC_ROLE_ID_LAST).find(|id| !uids.contains(id) && !gids.contains(id))
}

/// The launchd job: [`PLIST`] with its program arguments filled in.
/// `keep_awake` runs the node under /bin/sh, which starts `caffeinate -s -w $$`
/// watching its own pid and then execs the node into that pid — so the
/// keep-awake assertion (on AC power only) lives exactly as long as the node,
/// and launchd's SIGTERM reaches the node itself (which then sends E1.31
/// stream-terminated packets on its way out).
fn launchd_plist(keep_awake: bool) -> String {
    let args: Vec<String> = if keep_awake {
        vec![
            "/bin/sh".into(),
            "-c".into(),
            format!("/usr/bin/caffeinate -s -w $$ & exec {BIN_PATH} run --config {CONFIG_PATH}"),
        ]
    } else {
        vec![
            BIN_PATH.into(),
            "run".into(),
            "--config".into(),
            CONFIG_PATH.into(),
        ]
    };
    let lines: Vec<String> = args
        .iter()
        .map(|arg| format!("\t\t<string>{}</string>", xml_escape(arg)))
        .collect();
    PLIST.replace(PLIST_ARGS_SLOT, &lines.join("\n"))
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Write the daemon's plist and start it. A loaded job is left alone unless
/// the binary or the plist changed (systemd's `enable --now` semantics);
/// otherwise it is booted out first so the bootstrap starts the new binary
/// with the new definition — and since bootstrapping over a job that is still
/// stopping fails, only once it has left launchd.
fn start_launchd(keep_awake: bool, replaced: bool) -> Result<(), String> {
    let plist = launchd_plist(keep_awake);
    let changed = fs::read_to_string(PLIST_PATH).ok().as_deref() != Some(plist.as_str());
    fs::write(PLIST_PATH, &plist).map_err(|e| format!("write {PLIST_PATH}: {e}"))?;
    // launchd ignores a daemon plist that isn't root-owned or that anyone
    // else can write.
    run("chown", &["root:wheel", PLIST_PATH])?;
    run("chmod", &["644", PLIST_PATH])?;

    let target = format!("system/{LAUNCHD_LABEL}");
    let loaded = launchd_loaded(&target);
    if loaded && !replaced && !changed {
        return Ok(());
    }
    if loaded {
        // Its exit status isn't the signal (it can answer "in progress" while
        // the job is still stopping); the job leaving launchd is.
        run_ok("launchctl", &["bootout", &target]);
        let deadline = Instant::now() + LAUNCHD_STOP_WAIT;
        while launchd_loaded(&target) {
            if Instant::now() >= deadline {
                return Err(format!(
                    "{target} did not stop within {} s; rerun install",
                    LAUNCHD_STOP_WAIT.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    // `enable` clears a `launchctl disable` override, which would otherwise
    // refuse the bootstrap.
    run("launchctl", &["enable", &target])?;
    run("launchctl", &["bootstrap", "system", PLIST_PATH])
}

fn launchd_loaded(target: &str) -> bool {
    run_ok("launchctl", &["print", target])
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

/// Copy `src` to a sibling of `dst`, then rename it over `dst`. Writing into
/// `dst` directly fails with ETXTBSY while the service is running it; a rename
/// swaps the directory entry and leaves the running process its old inode.
fn replace_binary(src: &Path, dst: &Path) -> Result<(), String> {
    let name = dst
        .file_name()
        .ok_or_else(|| format!("{} has no file name", dst.display()))?;
    let tmp = dst.with_file_name(format!(".{}.new", name.to_string_lossy()));
    let staged = fs::copy(src, &tmp)
        .map_err(|e| format!("stage {}: {e}", tmp.display()))
        .and_then(|_| run("chmod", &["755", &tmp.to_string_lossy()]))
        .and_then(|()| {
            fs::rename(&tmp, dst).map_err(|e| format!("install {}: {e}", dst.display()))
        });
    if staged.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    staged
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
            scenes: serde_json::json!([]),
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
            "--pair",
        ]))
        .unwrap();
        assert_eq!(o.email.as_deref(), Some("a@b.com"));
        assert_eq!(o.setup_name.as_deref(), Some("Home"));
        assert_eq!(o.universe, Some(3));
        assert!(o.keep_sleep);
        assert!(o.password_stdin);
        assert!(o.pair);
        assert!(!Options::parse(&[]).unwrap().pair);
        assert!(Options::parse(&args(&["--nope"])).is_err());
        assert!(Options::parse(&args(&["--email"])).is_err()); // missing value
    }

    #[test]
    fn launchd_plist_keeps_awake_only_when_asked() {
        for keep_awake in [true, false] {
            let xml = launchd_plist(keep_awake);
            let plist = plist::Value::from_reader_xml(xml.as_bytes()).expect("well-formed plist");
            let job = plist.as_dictionary().expect("a dict");
            let string = |key: &str| job.get(key).and_then(plist::Value::as_string);

            let program: Vec<&str> = job
                .get("ProgramArguments")
                .and_then(plist::Value::as_array)
                .expect("ProgramArguments")
                .iter()
                .map(|arg| arg.as_string().expect("string arg"))
                .collect();
            let expected: Vec<String> = if keep_awake {
                vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    format!(
                        "/usr/bin/caffeinate -s -w $$ & exec {BIN_PATH} run --config {CONFIG_PATH}"
                    ),
                ]
            } else {
                vec![
                    BIN_PATH.into(),
                    "run".into(),
                    "--config".into(),
                    CONFIG_PATH.into(),
                ]
            };
            assert_eq!(program, expected);

            assert_eq!(string("Label"), Some(LAUNCHD_LABEL));
            assert_eq!(string("UserName"), Some(MAC_SERVICE_USER));
            assert_eq!(string("GroupName"), Some(MAC_SERVICE_USER));
            assert_eq!(string("StandardOutPath"), Some(MAC_LOG_FILE));
            assert_eq!(string("StandardErrorPath"), Some(MAC_LOG_FILE));
            assert_eq!(
                job.get("KeepAlive").and_then(plist::Value::as_boolean),
                Some(true)
            );
            assert_eq!(
                job.get("ThrottleInterval")
                    .and_then(plist::Value::as_signed_integer),
                Some(5)
            );
            // The daemon must read its session from the dir install wrote it to.
            let env = job
                .get("EnvironmentVariables")
                .and_then(plist::Value::as_dictionary)
                .expect("EnvironmentVariables");
            assert_eq!(
                env.get("XDG_CONFIG_HOME").and_then(plist::Value::as_string),
                Some(MAC_STATE_DIR)
            );
        }
    }

    #[test]
    fn service_definitions_run_the_installed_paths() {
        assert!(UNIT.contains(&format!("ExecStart={BIN_PATH} run --config {CONFIG_PATH}")));
        assert!(MAC_LOG_FILE.starts_with(MAC_LOG_DIR));
        assert_eq!(Platform::MacOs.owner(), "_luxnode:_luxnode");
        assert_eq!(Platform::Linux.owner(), SERVICE_USER);
    }

    #[test]
    fn dscl_ids_reads_the_number_column() {
        let listing = "_amavisd                 83\nnobody                   -2\n\n_oahd 441\n";
        assert_eq!(dscl_ids(listing), vec![83, 441]);
    }

    #[test]
    fn role_id_is_free_for_both_the_account_and_its_group() {
        assert_eq!(free_role_id(&[], &[]), Some(450));
        // Taken as a uid, then as a gid: skip both.
        assert_eq!(free_role_id(&[450], &[451]), Some(452));
        let all: Vec<u32> = (450..=499).collect();
        assert_eq!(free_role_id(&all, &[]), None);
        assert_eq!(free_role_id(&[], &all), None);
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
    fn replace_binary_swaps_in_place_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("lux-node-install-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("new");
        let dst = dir.join("lux-node");
        fs::write(&src, b"new").unwrap();
        fs::write(&dst, b"old").unwrap();

        replace_binary(&src, &dst).unwrap();

        assert_eq!(fs::read(&dst).unwrap(), b"new");
        assert!(!dir.join(".lux-node.new").exists());
        fs::remove_dir_all(&dir).unwrap();
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
