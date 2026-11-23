//! `ck-forge registry` and the push to the Hub.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;

use ck_admin_core::{Error, Registry};

pub(crate) fn registry_list(path: &Path) -> Result<String, Error> {
    let registry = Registry::load(path)?;
    if registry.list().is_empty() {
        return Ok(format!("No sticks in {} yet\n", path.display()));
    }
    let mut text = String::new();
    for entry in registry.list() {
        let _ = writeln!(
            text,
            "{}  {}  #{}  {} / {}  {}  {}",
            entry.stick_id,
            entry.created_at,
            entry.machine_number.unwrap_or(1),
            entry.team,
            entry.site,
            entry.contest,
            entry.image
        );
    }
    Ok(text)
}

pub(crate) fn registry_push(hub: &str, user: &str, registry: &Path) -> Result<String, Error> {
    // Without echo, and never from an argument or the environment, so it is not left in a history.
    let password = rpassword::prompt_password(format!("Password for {user} on {hub}: "))
        .map_err(|e| Error::io("reading the password", e))?;
    let pushed = ck_admin_core::push_registry(hub, user, &password, registry)?;
    let text = format!(
        "Hub: {} added, {} already known\n",
        pushed.added, pushed.already_known
    );
    if pushed.conflicts.is_empty() {
        return Ok(text);
    }
    io::stdout()
        .write_all(text.as_bytes())
        .map_err(|e| Error::io("writing the output", e))?;
    Err(Error::Hub(format!(
        "the Hub already has a different secret for: {}; they were left as they were",
        pushed.conflicts.join(", ")
    )))
}
