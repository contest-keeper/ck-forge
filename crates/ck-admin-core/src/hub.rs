//! Telling the Hub about the sticks that were made.
//!
//! The password is used for one login and then dropped. It is never stored or printed, and neither
//! is the session cookie the Hub answers with.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::error::Error;

/// What the Hub did with the registry.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Pushed {
    pub added: usize,
    pub already_known: usize,
    pub conflicts: Vec<String>,
}

/// Logs in to the Hub as `user` and sends it every stick of the registry file.
///
/// Sending the same registry twice is safe. A stick the Hub already has with a different secret is
/// listed in `conflicts` and left as it was.
///
/// # Errors
/// Fails when the address is not safe for a password, the file cannot be read, the Hub cannot be
/// reached, or it refuses the login or the registry.
pub fn push_registry(
    hub: &str,
    user: &str,
    password: &str,
    registry: &Path,
) -> Result<Pushed, Error> {
    let base = hub.trim_end_matches('/');
    if !safe_for_password(base) {
        return Err(Error::Hub(format!(
            "{base} is not https, so the password would cross the network in the clear; use an https address"
        )));
    }
    let body = std::fs::read_to_string(registry)
        .map_err(|e| Error::io(format!("reading {}", registry.display()), e))?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();

    let login = agent
        .post(format!("{base}/api/v1/login"))
        .send_json(json!({ "username": user, "password": password }))
        .map_err(|e| Error::Hub(format!("cannot reach the Hub at {base}: {e}")))?;
    match login.status().as_u16() {
        200 => {}
        401 => return Err(Error::Hub("the Hub refused that name or password".into())),
        429 => {
            return Err(Error::Hub(
                "too many wrong passwords; wait a few minutes".into(),
            ));
        }
        code => return Err(Error::Hub(format!("the Hub answered {code} to the login"))),
    }
    let cookie = login
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| value.strip_prefix("ck_session="))
        .and_then(|rest| rest.split(';').next())
        .map(|token| format!("ck_session={token}"))
        .ok_or_else(|| Error::Hub("the Hub did not start a session".into()))?;

    let mut answer = agent
        .post(format!("{base}/api/v1/sticks"))
        .header("cookie", &cookie)
        .header("content-type", "application/json")
        .send(body)
        .map_err(|e| Error::Hub(format!("cannot reach the Hub at {base}: {e}")))?;
    let status = answer.status().as_u16();
    let text = answer
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}")))?;
    // 409 still carries the counts: the sticks without a conflict were registered.
    match status {
        200 | 409 => serde_json::from_str(&text)
            .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}"))),
        403 => Err(Error::Hub("this account is not an admin".into())),
        400 => Err(Error::Hub(format!("the Hub refused the registry: {text}"))),
        code => Err(Error::Hub(format!("the Hub answered {code}"))),
    }
}

/// A password may only go over https, or to this PC (a Hub started for a test).
fn safe_for_password(base: &str) -> bool {
    if base.starts_with("https://") {
        return true;
    }
    let Some(rest) = base.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', ':']).next().unwrap_or("");
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

#[cfg(test)]
mod tests {
    use super::safe_for_password;

    #[test]
    fn only_https_or_this_pc_may_get_a_password() {
        assert!(safe_for_password("https://hub.example.org"));
        assert!(safe_for_password("http://127.0.0.1:8088"));
        assert!(safe_for_password("http://localhost:8088"));
        assert!(!safe_for_password("http://hub.example.org"));
        assert!(!safe_for_password("http://127.0.0.1.evil.org"));
        assert!(!safe_for_password("hub.example.org"));
    }
}
