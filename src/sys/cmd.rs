//! Process execution helpers, including privilege escalation through `sudo`.
//!
//! Programs are executed directly (no shell is ever involved), so arguments
//! such as SSIDs and service names are passed verbatim.

use std::io::Write;
use std::process::{Command, Stdio};

pub const NETWORKSETUP: &str = "/usr/sbin/networksetup";
pub const SCUTIL: &str = "/usr/sbin/scutil";
pub const IFCONFIG: &str = "/sbin/ifconfig";
const SUDO: &str = "/usr/bin/sudo";

#[derive(Debug, Clone)]
pub enum CmdError {
    /// Administrator rights are needed and no (valid) cached sudo credential exists.
    NeedPassword,
    /// The supplied sudo password was rejected.
    BadPassword,
    /// The command ran but reported a failure.
    Failed(String),
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CmdError::NeedPassword => write!(f, "administrator password required"),
            CmdError::BadPassword => write!(f, "incorrect administrator password"),
            CmdError::Failed(s) => write!(f, "{s}"),
        }
    }
}

pub type CmdResult = Result<String, CmdError>;

pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Run a program without elevation and return combined stdout+stderr.
pub fn run(prog: &str, args: &[&str]) -> CmdResult {
    let out = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| CmdError::Failed(format!("failed to run {prog}: {e}")))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        return Err(classify_failure(&text));
    }
    check_tool_output(text)
}

/// Run a program with administrator rights.
///
/// * If already root, runs directly.
/// * Otherwise tries `sudo -n` (cached credentials) first, then `sudo -S`
///   feeding `password` on stdin if one was supplied.
pub fn run_privileged(prog: &str, args: &[&str], password: Option<&str>) -> CmdResult {
    if is_root() {
        return run(prog, args);
    }

    // Non-interactive attempt first (cached sudo timestamp).
    let mut sudo_args = vec!["-n", "--", prog];
    sudo_args.extend_from_slice(args);
    let out = Command::new(SUDO)
        .args(&sudo_args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| CmdError::Failed(format!("failed to run sudo: {e}")))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        return check_tool_output(text);
    }
    let needs_pw = text.contains("a password is required") || text.contains("a terminal is required");
    if !needs_pw {
        return Err(classify_failure(&text));
    }

    let Some(pw) = password else {
        return Err(CmdError::NeedPassword);
    };

    let mut sudo_args = vec!["-S", "-p", "", "--", prog];
    sudo_args.extend_from_slice(args);
    let mut child = Command::new(SUDO)
        .args(&sudo_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CmdError::Failed(format!("failed to run sudo: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(pw.as_bytes());
        let _ = stdin.write_all(b"\n");
    }
    let out = child
        .wait_with_output()
        .map_err(|e| CmdError::Failed(format!("sudo failed: {e}")))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if text.contains("incorrect password") || text.contains("Sorry, try again") {
        return Err(CmdError::BadPassword);
    }
    if !out.status.success() {
        return Err(classify_failure(&text));
    }
    check_tool_output(text)
}

fn classify_failure(text: &str) -> CmdError {
    let t = text.trim();
    CmdError::Failed(if t.is_empty() { "command failed".into() } else { t.to_string() })
}

/// `networksetup` frequently exits 0 even on failure; detect its error text.
fn check_tool_output(text: String) -> CmdResult {
    let lower = text.to_lowercase();
    let markers = [
        "** error",
        "error:",
        "is not a recognized network service",
        "could not find network",
        "failed to join",
        "requires admin",
        "must run this tool as root",
        "unable to",
    ];
    if markers.iter().any(|m| lower.contains(m)) {
        Err(CmdError::Failed(text.trim().to_string()))
    } else {
        Ok(text)
    }
}

/// True if a non-privileged failure message indicates admin rights are needed.
pub fn looks_like_permission_error(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("admin") || l.contains("root") || l.contains("permission") || l.contains("not permitted")
}
