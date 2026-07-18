//! `sudo lux-node install` — the binary installs itself as a systemd service.
//!
//! Idempotent: every step checks before it acts, so re-running upgrades the
//! binary in place and fixes whatever is missing. All the sysadmin choreography
//! (service user, dirs, unit, config, login-as-the-service-identity, enable)
//! lives here so the runbook is two commands: download, `sudo ./lux-node
//! install`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::auth;
use crate::config::{self, StoredSession};

const BIN_PATH: &str = "/usr/local/bin/lux-node";
const ETC_DIR: &str = "/etc/lux-node";
const STATE_DIR: &str = "/var/lib/lux-node";
const UNIT_PATH: &str = "/etc/systemd/system/lux-node.service";
const SERVICE_USER: &str = "lux-node";
const UNIT: &str = include_str!("../lux-node.service");

pub fn install(keep_sleep: bool) -> Result<(), String> {
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

    // 3. Config (prompt only when absent — rerunning never clobbers).
    let config_path = format!("{ETC_DIR}/config.json");
    if !Path::new(&config_path).exists() {
        let setup_id = prompt("setup id (from the app's setups)")?;
        let universe = prompt("sACN universe [1]")?;
        let universe: u16 = if universe.is_empty() {
            1
        } else {
            universe
                .parse()
                .map_err(|e| format!("bad universe {universe}: {e}"))?
        };
        let json = serde_json::json!({ "setupId": setup_id, "universe": universe });
        fs::write(&config_path, format!("{:#}\n", json))
            .map_err(|e| format!("write {config_path}: {e}"))?;
        println!("wrote {config_path}");
    }

    // 4. Unit file (embedded in the binary, so they can't drift apart).
    fs::write(UNIT_PATH, UNIT).map_err(|e| format!("write {UNIT_PATH}: {e}"))?;

    // 5. Sign in as the service identity (skip when a session already exists).
    let session_file = format!("{STATE_DIR}/lux-node/session.json");
    if !Path::new(&session_file).exists() {
        let email = prompt("lux account email")?;
        let password = rpassword::prompt_password("password: ")
            .map_err(|e| format!("password prompt: {e}"))?;
        let env = config::endpoints()?;
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let tokens = runtime.block_on(auth::sign_in(&env, &email, &password))?;
        let refresh = tokens
            .refresh
            .ok_or("Cognito returned no refresh token; cannot run headless")?;
        // Write via the same path the service reads (XDG under the state dir),
        // then hand the file to the service user.
        std::env::set_var("XDG_CONFIG_HOME", STATE_DIR);
        config::save_session(&StoredSession {
            email: email.clone(),
            refresh_token: refresh,
        })?;
        run("chown", &["-R", SERVICE_USER, STATE_DIR])?;
        println!("signed in as {email}");
    }

    // 6. Enable + start.
    run("systemctl", &["daemon-reload"])?;
    run("systemctl", &["enable", "--now", "lux-node"])?;

    // 7. An always-on box should stay on (opt out with --keep-sleep).
    if !keep_sleep {
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

fn is_root() -> bool {
    run_out("id", &["-u"]).is_some_and(|out| out.trim() == "0")
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
