//! A plain log of what the window does, one line per step, in a file the admin can send.
//!
//! The lines name steps and outcomes only. Callers must never pass a password, a token or a
//! session id: say that one was given, not what it is.

use std::io::Write as _;
use std::path::PathBuf;

/// `~/Library/Logs/Keeper Forge/forge.log` on macOS, `%LOCALAPPDATA%\Keeper Forge\forge.log` on
/// Windows, `~/.local/state/keeper-forge/forge.log` elsewhere. The temp folder when none is known.
#[must_use]
pub fn path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let folder = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Logs/Keeper Forge"))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Keeper Forge"))
    } else {
        home.map(|h| h.join(".local/state/keeper-forge"))
    };
    folder.unwrap_or_else(std::env::temp_dir).join("forge.log")
}

/// Appends one timestamped line to the log file and to stderr. A log that cannot be written never
/// stops the work: the line is then only on stderr.
pub fn step(message: &str) {
    let line = format!("{} {message}", stamp());
    // eprintln! panics when stderr is closed, as it is for an app started from the Dock.
    let _ = writeln!(std::io::stderr().lock(), "{line}");
    let file = path();
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    if let Ok(mut open) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        let _ = writeln!(open, "{line}");
    }
}

fn stamp() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!(
        "{} {:02}:{:02}:{:02}",
        now.date(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_step_is_appended_to_the_log_file() {
        super::step("log test line");
        let text = std::fs::read_to_string(super::path()).expect("log file");
        assert!(text.contains("log test line"));
    }
}
