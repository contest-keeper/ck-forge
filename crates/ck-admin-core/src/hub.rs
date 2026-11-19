//! Telling the Hub about the sticks that were made.
//!
//! The password is used for one login and then dropped. It is never stored or printed, and neither
//! is the session cookie the Hub answers with.
//!
//! The window signs in through the browser instead: it opens the Hub's page, listens on a port of
//! this PC, and the Hub sends the browser back to that port with a code that works once. The code
//! is traded for a session token that is only kept in memory and never printed.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant};

use base64::Engine;
use ck_protocol::{Settings, StickId};
use serde::Deserialize;
use serde_json::json;

use crate::error::Error;
use crate::profile::StickKeyWindow;
use crate::wallpaper::{MAX_WALLPAPER_BYTES, Wallpaper};

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
        // A redirect would resend the password body to wherever it points.
        .max_redirects(0)
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

    post_registry(&agent, base, ("cookie", &cookie), body)
}

/// Sends the registry text with one request header that says who is sending it.
fn post_registry(
    agent: &ureq::Agent,
    base: &str,
    auth: (&str, &str),
    body: String,
) -> Result<Pushed, Error> {
    let mut answer = agent
        .post(format!("{base}/api/v1/sticks"))
        .header(auth.0, auth.1)
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
        401 => Err(Error::Hub("the sign-in ended; sign in again".into())),
        403 => Err(Error::Hub("this account is not an admin".into())),
        400 => Err(Error::Hub(format!("the Hub refused the registry: {text}"))),
        code => Err(Error::Hub(format!("the Hub answered {code}"))),
    }
}

/// What the Hub gave this window when the admin signed in. The program keeps it in memory and in
/// the operating system's credential vault, never in a file.
pub struct HubSession {
    token: String,
    pub email: String,
}

impl HubSession {
    /// Rebuilds a session the program saved earlier.
    #[must_use]
    pub fn restore(token: String, email: String) -> Self {
        Self { token, email }
    }

    /// The secret the Hub gave. Only for saving it in the operating system's vault.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

// A derived `Debug` would print the token.
impl std::fmt::Debug for HubSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubSession")
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

/// A random value that ties the page the browser comes back from to the sign-in that was started.
#[must_use]
pub fn new_state() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// An id for a profile that is saved to the Hub for the first time.
#[must_use]
pub fn new_profile_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// The Hub page the browser opens. It asks the admin to sign in, if needed, and to agree.
pub fn sign_in_url(hub: &str, port: u16, state: &str) -> Result<String, Error> {
    let base = checked_base(hub)?;
    Ok(format!("{base}/forge/connect?port={port}&state={state}"))
}

/// An address the code and the session may be sent to: https, or this PC (a Hub started for a test).
pub(crate) fn checked_base(hub: &str) -> Result<&str, Error> {
    let base = hub.trim_end_matches('/');
    if safe_for_password(base) {
        Ok(base)
    } else {
        Err(Error::Hub(format!(
            "{base} is not https, so the sign-in would cross the network in the clear"
        )))
    }
}

/// Waits for the browser to come back to `listener` and returns the code the Hub put in the
/// address. A request without the right `state` is answered and ignored, because anything on this
/// PC (a web page, for one) can send a request to the port.
pub fn wait_for_code(listener: &TcpListener, state: &str, wait: Duration) -> Result<String, Error> {
    let io = |e| Error::io("waiting for the browser", e);
    listener.set_nonblocking(true).map_err(io)?;
    let until = Instant::now() + wait;
    while Instant::now() < until {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(io(e)),
        };
        stream.set_nonblocking(false).map_err(io)?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(io)?;
        let code = read_callback(&mut stream, state);
        let (status, text) = match code {
            Some(_) => (
                "200 OK",
                "Signed in. You can close this tab and go back to Keeper Forge.",
            ),
            None => (
                "400 Bad Request",
                "This is not the sign-in Keeper Forge is waiting for.",
            ),
        };
        let page =
            format!("<!doctype html><meta charset=utf-8><title>Keeper Forge</title><p>{text}</p>");
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{page}",
            page.len()
        );
        if let Some(code) = code {
            return Ok(code);
        }
    }
    Err(Error::Hub(
        "the sign-in in the browser was not finished in time; try again".into(),
    ))
}

/// The code of a `GET /callback?code=..&state=..` request, when the state is the expected one.
fn read_callback(stream: &mut impl Read, state: &str) -> Option<String> {
    let mut head = Vec::new();
    let mut chunk = [0_u8; 1024];
    // Only the first line matters, and a browser sends it in one piece.
    while !head.contains(&b'\n') && head.len() < 8192 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&head);
    let target = text.lines().next()?.strip_prefix("GET ")?;
    let query = target.split(' ').next()?.strip_prefix("/callback?")?;
    let mut code = None;
    let mut got_state = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", value)) => code = Some(value),
            Some(("state", value)) => got_state = Some(value),
            _ => {}
        }
    }
    let code = code.filter(|c| !c.is_empty() && c.chars().all(is_token_char))?;
    (got_state == Some(state)).then(|| code.to_owned())
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

#[derive(Deserialize)]
struct TokenAnswer {
    token: String,
    email: String,
}

/// Trades the code from the browser for a session.
pub fn sign_in_with_code(hub: &str, code: &str) -> Result<HubSession, Error> {
    let base = checked_base(hub)?;
    let mut answer = agent()
        .post(format!("{base}/api/v1/forge/token"))
        .send_json(json!({ "code": code }))
        .map_err(|e| Error::Hub(format!("cannot reach the Hub at {base}: {e}")))?;
    match answer.status().as_u16() {
        200 => {}
        400 => return Err(Error::Hub("the sign-in code ended; sign in again".into())),
        code => {
            return Err(Error::Hub(format!(
                "the Hub answered {code} to the sign-in"
            )));
        }
    }
    let answer: TokenAnswer = answer
        .body_mut()
        .read_json()
        .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}")))?;
    Ok(HubSession {
        token: answer.token,
        email: answer.email,
    })
}

/// Sends every stick of the registry file, as the signed-in admin.
///
/// Sending the same registry twice is safe. A stick the Hub already has with a different secret is
/// listed in `conflicts` and left as it was.
pub fn push_registry_signed_in(
    hub: &str,
    session: &HubSession,
    registry: &Path,
) -> Result<Pushed, Error> {
    let base = checked_base(hub)?;
    let body = std::fs::read_to_string(registry)
        .map_err(|e| Error::io(format!("reading {}", registry.display()), e))?;
    let bearer = format!("Bearer {}", session.token);
    post_registry(&agent(), base, ("authorization", &bearer), body)
}

/// Retires a previous stick key without removing its history from the Hub.
pub fn retire_stick_credential(hub: &str, session: &HubSession, id: &str) -> Result<(), Error> {
    let id = StickId::parse(id.trim()).map_err(|_| Error::Hub("enter a valid stick ID".into()))?;
    let base = checked_base(hub)?;
    let answer = agent()
        .delete(format!("{base}/api/v1/sticks/{id}/credential"))
        .header("authorization", &session.bearer())
        .call()
        .map_err(|e| unreachable_hub(base, &e))?;
    let status = answer.status().as_u16();
    match status {
        204 => Ok(()),
        401 => Err(Error::Hub("sign in again to retire the stick key".into())),
        403 => Err(Error::Hub("this account cannot retire stick keys".into())),
        404 => Err(Error::Hub("this organization has no such stick".into())),
        409 => Err(Error::Hub(
            "retiring this key in Forge requires a sign-in with one organization".into(),
        )),
        _ => Err(Error::Hub(format!(
            "the Hub answered {status} while retiring the stick key"
        ))),
    }
}

#[derive(Debug, Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
}

/// A profile as the Hub keeps it: the settings and the wallpaper picture, if any.
#[derive(Debug)]
pub struct HubProfile {
    pub name: String,
    pub settings: Settings,
    pub stick_key_window: Option<StickKeyWindow>,
    pub wallpaper: Option<Wallpaper>,
}

#[derive(Deserialize)]
struct ProfileAnswer {
    name: String,
    settings: serde_json::Value,
    wallpaper: Option<WallpaperName>,
}

#[derive(Deserialize)]
struct WallpaperName {
    name: String,
}

/// The profiles of the signed-in admin's organization.
pub fn list_profiles(hub: &str, session: &HubSession) -> Result<Vec<ProfileSummary>, Error> {
    let base = checked_base(hub)?;
    let mut answer = agent()
        .get(format!("{base}/api/v1/profiles"))
        .header("authorization", &session.bearer())
        .call()
        .map_err(|e| unreachable_hub(base, &e))?;
    check_profile_status(&mut answer)?;
    answer
        .body_mut()
        .read_json()
        .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}")))
}

/// One profile of the organization, with its wallpaper picture.
pub fn fetch_profile(hub: &str, session: &HubSession, id: &str) -> Result<HubProfile, Error> {
    let base = checked_base(hub)?;
    let bearer = session.bearer();
    let mut answer = agent()
        .get(format!("{base}/api/v1/profiles/{id}"))
        .header("authorization", &bearer)
        .call()
        .map_err(|e| unreachable_hub(base, &e))?;
    check_profile_status(&mut answer)?;
    let profile: ProfileAnswer = answer
        .body_mut()
        .read_json()
        .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}")))?;
    let stick_key_window = serde_json::from_value(
        profile
            .settings
            .get("stick_key_window")
            .cloned()
            .unwrap_or_default(),
    )
    .map_err(|e| Error::Hub(format!("the Hub has an unreadable stick key window: {e}")))?;
    let settings = serde_json::from_value(profile.settings).map_err(|e| {
        Error::Hub(format!(
            "the Hub has settings this version cannot read: {e}"
        ))
    })?;
    let wallpaper = match profile.wallpaper {
        Some(WallpaperName { name }) => {
            let mut picture = agent()
                .get(format!("{base}/api/v1/profiles/{id}/wallpaper"))
                .header("authorization", &bearer)
                .call()
                .map_err(|e| unreachable_hub(base, &e))?;
            check_profile_status(&mut picture)?;
            let bytes = picture
                .body_mut()
                .with_config()
                .limit(MAX_WALLPAPER_BYTES)
                .read_to_vec()
                .map_err(|e| Error::Hub(format!("unreadable wallpaper from the Hub: {e}")))?;
            Some(Wallpaper { name, bytes })
        }
        None => None,
    };
    Ok(HubProfile {
        name: profile.name,
        settings,
        stick_key_window,
        wallpaper,
    })
}

/// Creates the profile `id` on the Hub or replaces it. No wallpaper removes the Hub's picture.
pub fn upload_profile(
    hub: &str,
    session: &HubSession,
    id: &str,
    profile: &HubProfile,
) -> Result<(), Error> {
    let base = checked_base(hub)?;
    let wallpaper = profile.wallpaper.as_ref().map(|w| {
        json!({ "name": w.name, "data_base64": base64::engine::general_purpose::STANDARD.encode(&w.bytes) })
    });
    // The Wi-Fi password never leaves the admin's computer except on a stick.
    let mut shared = profile.settings.clone();
    if let Some(wifi) = &mut shared.wifi {
        wifi.password = None;
    }
    let mut settings = serde_json::to_value(&shared)
        .map_err(|e| Error::Hub(format!("settings cannot be sent: {e}")))?;
    if let Some(window) = &profile.stick_key_window {
        settings["stick_key_window"] = json!(window);
    }
    let mut answer = agent()
        .put(format!("{base}/api/v1/profiles/{id}"))
        .header("authorization", &session.bearer())
        .send_json(json!({ "name": profile.name, "settings": settings, "wallpaper": wallpaper }))
        .map_err(|e| unreachable_hub(base, &e))?;
    check_profile_status(&mut answer)
}

fn unreachable_hub(base: &str, error: &ureq::Error) -> Error {
    Error::Hub(format!("cannot reach the Hub at {base}: {error}"))
}

/// Plain words for what the Hub answers to a profile request; `Ok` for any success.
fn check_profile_status(answer: &mut ureq::http::Response<ureq::Body>) -> Result<(), Error> {
    let status = answer.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    let text = answer.body_mut().read_to_string().unwrap_or_default();
    Err(Error::Hub(match status {
        401 => "the sign-in ended; sign in again".to_owned(),
        403 => "this account is not an admin".to_owned(),
        404 => "the Hub has no such profile".to_owned(),
        409 => "this account is in several organizations; profiles need exactly one".to_owned(),
        400 | 413 => format!("the Hub refused the profile: {text}"),
        code => format!("the Hub answered {code}"),
    }))
}

/// Ends the session at the Hub. A Hub that cannot be reached is not an error: the session ends by
/// itself.
pub fn sign_out(hub: &str, session: &HubSession) {
    let Ok(base) = checked_base(hub) else { return };
    let bearer = format!("Bearer {}", session.token);
    let _ = agent()
        .post(format!("{base}/api/v1/logout"))
        .header("authorization", &bearer)
        .send_empty();
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        // A redirect would resend the code or the token to wherever it points.
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// A password may only go over https, or to this PC (a Hub started for a test).
fn safe_for_password(base: &str) -> bool {
    if base.starts_with("https://") {
        return true;
    }
    let Some(rest) = base.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    // Text before an `@` is a user name, and the real host comes after it.
    if authority.contains('@') {
        return false;
    }
    let host = authority.split(':').next().unwrap_or("");
    matches!(host, "localhost" | "127.0.0.1")
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
        assert!(!safe_for_password("http://localhost:80@evil.org"));
        assert!(!safe_for_password("http://localhost@evil.org/"));
        assert!(!safe_for_password("hub.example.org"));
    }
}
