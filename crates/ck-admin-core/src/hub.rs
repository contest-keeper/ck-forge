//! Telling the Hub about the sticks that were made.
//!
//! The password is used for one login and then dropped. It is never stored or printed, and neither
//! is the session cookie the Hub answers with.
//!
//! The window signs in through the browser instead: it asks the Hub for a pairing code, shows it and
//! opens the Hub's page, which shows the same code. When the admin agrees there, the Hub hands this
//! Forge a session token once. The token is only kept in memory and never printed.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use ck_protocol::{EnrolmentCode, Settings, StickId};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use ureq::http::header::{AUTHORIZATION, COOKIE};
use ureq::http::{HeaderName, HeaderValue};

use crate::error::Error;
use crate::hardware::HardwareId;
use crate::profile::StickKeyWindow;
use crate::registry::CredentialScope;
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
    let body = registry_payload(registry)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        // A redirect would resend the password body to wherever it points.
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();

    let login = agent
        .post(format!("{base}/api/v1/login"))
        .send_json(json!({ "email": user, "password": password }))
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

    let cookie = sensitive_header(&cookie)?;
    let url = format!("{}/sticks", org_url(&agent, base, &COOKIE, &cookie)?);
    post_registry(&agent, base, &url, COOKIE, cookie, body)
}

fn registry_payload(registry: &Path) -> Result<String, Error> {
    let body = std::fs::read_to_string(registry)
        .map_err(|error| Error::io(format!("reading {}", registry.display()), error))?;
    if let Ok(serde_json::Value::Object(mut fields)) = serde_json::from_str(&body)
        && fields.remove("enrolments").is_some()
    {
        return Ok(serde_json::Value::Object(fields).to_string());
    }
    Ok(body)
}

/// Sends the registry text with one request header that says who is sending it.
fn post_registry(
    agent: &ureq::Agent,
    base: &str,
    url: &str,
    name: HeaderName,
    value: HeaderValue,
    body: String,
) -> Result<Pushed, Error> {
    let mut answer = agent
        .post(url)
        .header(name, value)
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
        401 => Err(Error::SignedOut("the sign-in ended; sign in again".into())),
        403 => Err(Error::Hub("this account is not an admin".into())),
        400 => Err(Error::Hub(format!("the Hub refused the registry: {text}"))),
        code => Err(Error::Hub(format!("the Hub answered {code}"))),
    }
}

#[derive(Deserialize)]
struct MeAnswer {
    organizations: Vec<MeOrganization>,
}

#[derive(Deserialize)]
struct MeOrganization {
    slug: String,
}

/// `{base}/api/v1/orgs/{slug}` for the one organization the signed-in account belongs to. Forge
/// works with one organization at a time, so an account with none or several is refused.
fn org_url(
    agent: &ureq::Agent,
    base: &str,
    name: &HeaderName,
    value: &HeaderValue,
) -> Result<String, Error> {
    let mut answer = agent
        .get(format!("{base}/api/v1/me"))
        .header(name, value.clone())
        .call()
        .map_err(|e| unreachable_hub(base, &e))?;
    match answer.status().as_u16() {
        200 => {}
        401 => return Err(Error::SignedOut("the sign-in ended; sign in again".into())),
        code => return Err(Error::Hub(format!("the Hub answered {code}"))),
    }
    let me: MeAnswer = answer
        .body_mut()
        .read_json()
        .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}")))?;
    let [only] = me.organizations.as_slice() else {
        return Err(Error::Hub(if me.organizations.is_empty() {
            "this account belongs to no organization".into()
        } else {
            "this account belongs to several organizations; Forge works with one".into()
        }));
    };
    if only.slug.is_empty()
        || !only
            .slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::Hub(
            "the Hub sent an organization name Forge cannot use".into(),
        ));
    }
    Ok(format!("{base}/api/v1/orgs/{}", only.slug))
}

/// Builds a header value that carries a secret and marks it sensitive, so the HTTP stack leaves it
/// out of `Debug` output. Every Hub header with a token or cookie is made here.
pub(crate) fn sensitive_header(value: &str) -> Result<HeaderValue, Error> {
    let mut header =
        HeaderValue::from_str(value).map_err(|_| Error::Hub("sign in to the Hub again".into()))?;
    header.set_sensitive(true);
    Ok(header)
}

/// What the Hub gave this window when the admin signed in. The program keeps it in memory and in
/// the operating system's credential vault, never in a file.
pub struct HubSession {
    token: String,
    pub email: String,
    /// The slug of the one organization the account belongs to, asked of the Hub when first needed.
    org: std::sync::OnceLock<String>,
}

impl HubSession {
    /// Rebuilds a session the program saved earlier.
    #[must_use]
    pub fn restore(token: String, email: String) -> Self {
        Self {
            token,
            email,
            org: std::sync::OnceLock::new(),
        }
    }

    /// The secret the Hub gave. Only for saving it in the operating system's vault.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The address of `tail` inside this account's organization, like `{base}/api/v1/orgs/acm/sticks`.
    pub(crate) fn org_url(&self, base: &str, tail: &str) -> Result<String, Error> {
        if let Some(slug) = self.org.get() {
            return Ok(format!("{base}/api/v1/orgs/{slug}{tail}"));
        }
        let url = org_url(&agent(), base, &AUTHORIZATION, &self.authorization()?)?;
        let _ = self
            .org
            .set(url.rsplit('/').next().unwrap_or_default().to_owned());
        Ok(format!("{url}{tail}"))
    }

    /// The `Authorization` header for a Hub request, marked sensitive so no log prints it.
    pub(crate) fn authorization(&self) -> Result<HeaderValue, Error> {
        sensitive_header(&format!("Bearer {}", self.token))
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

/// An id for a profile that is saved to the Hub for the first time.
#[must_use]
pub fn new_profile_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
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

/// A sign-in waiting for the admin to agree in the browser.
pub struct Pairing {
    /// The code to show the admin. The Hub page shows the same one, so the admin can see that the
    /// page answers this Forge and not someone else's.
    pub code: String,
    /// The Hub page to open in the browser.
    pub url: String,
    // What this Forge asks about the answer with. Only the Hub and this Forge know it.
    poll_key: String,
}

impl std::fmt::Debug for Pairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pairing")
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Started {
    code: String,
    poll_key: String,
}

#[derive(Deserialize)]
struct Polled {
    status: String,
    token: Option<String>,
    email: Option<String>,
}

/// Asks the Hub for a pairing code. Nothing is typed in Forge: the admin agrees in the browser.
pub fn start_pairing(hub: &str) -> Result<Pairing, Error> {
    let base = checked_base(hub)?;
    let mut answer = agent()
        .post(format!("{base}/api/v1/forge/pairings"))
        .send_json(json!({}))
        .map_err(|e| unreachable_hub(base, &e))?;
    match answer.status().as_u16() {
        200 => {}
        404 | 405 => {
            return Err(Error::Hub(
                "this Hub is too old for this Forge; ask for it to be updated".into(),
            ));
        }
        429 => {
            return Err(Error::Hub(
                "too many sign-ins were started; wait a minute and try again".into(),
            ));
        }
        code => {
            return Err(Error::Hub(format!(
                "the Hub answered {code} to the sign-in"
            )));
        }
    }
    let started: Started = answer
        .body_mut()
        .read_json()
        .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}")))?;
    // The code goes into an address; a Hub that sends anything else is not one to follow.
    if started.code.is_empty()
        || started.code.len() > 16
        || !started
            .code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(Error::Hub(
            "the Hub sent a sign-in code that cannot be used".into(),
        ));
    }
    Ok(Pairing {
        url: format!("{base}/forge/connect?code={}", started.code),
        code: started.code,
        poll_key: started.poll_key,
    })
}

/// How a pairing ended.
pub enum PairingEnd {
    Approved(HubSession),
    /// The Hub said no more. It answers the same for a refusal and for an end, so the caller tells
    /// them apart by its own clock.
    Gone,
    /// `wait` ran out first.
    TimedOut,
    Cancelled,
}

/// Waits, asking the Hub every `every`, until the admin agrees in the browser. The session is
/// handed out once, kept in memory and never printed.
pub fn wait_for_pairing(
    hub: &str,
    pairing: &Pairing,
    wait: Duration,
    every: Duration,
) -> Result<HubSession, Error> {
    match watch_pairing(hub, pairing, wait, every, &AtomicBool::new(false))? {
        PairingEnd::Approved(session) => Ok(session),
        PairingEnd::Gone | PairingEnd::Cancelled => Err(Error::Hub(
            "the sign-in was refused or ran out of time; try again".into(),
        )),
        PairingEnd::TimedOut => Err(Error::Hub(
            "the sign-in in the browser was not finished in time; try again".into(),
        )),
    }
}

/// Like `wait_for_pairing`, and stops early when `cancel` is set.
pub fn watch_pairing(
    hub: &str,
    pairing: &Pairing,
    wait: Duration,
    every: Duration,
    cancel: &AtomicBool,
) -> Result<PairingEnd, Error> {
    let base = checked_base(hub)?;
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if cancel.load(Ordering::Relaxed) {
            return Ok(PairingEnd::Cancelled);
        }
        let mut answer = agent()
            .post(format!("{base}/api/v1/forge/pairings/poll"))
            .send_json(json!({ "poll_key": pairing.poll_key }))
            .map_err(|e| unreachable_hub(base, &e))?;
        match answer.status().as_u16() {
            200 => {}
            410 => return Ok(PairingEnd::Gone),
            code => {
                return Err(Error::Hub(format!(
                    "the Hub answered {code} to the sign-in"
                )));
            }
        }
        let polled: Polled = answer
            .body_mut()
            .read_json()
            .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}")))?;
        if let (true, Some(token), Some(email)) =
            (polled.status == "approved", polled.token, polled.email)
        {
            return Ok(PairingEnd::Approved(HubSession::restore(token, email)));
        }
        // Short sleeps, so a cancel is felt at once.
        let next = Instant::now() + every;
        while Instant::now() < next && !cancel.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(PairingEnd::TimedOut)
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
    let body = registry_payload(registry)?;
    let url = session.org_url(base, "/sticks")?;
    post_registry(
        &agent(),
        base,
        &url,
        AUTHORIZATION,
        session.authorization()?,
        body,
    )
}

#[derive(Debug, Deserialize)]
pub struct MintedEnrolmentCode {
    pub code: EnrolmentCode,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

/// How long an enrolment code works. Sticks may be prepared days ahead of a contest, so 30 days
/// matches the longest lifetime the Hub allows.
const ENROLMENT_CODE_MINUTES: i64 = 30 * 24 * 60;

#[derive(Serialize)]
struct EnrolmentRequest<'a> {
    stick_id: &'a StickId,
    team: &'a str,
    site: &'a str,
    contest: &'a str,
    machine_number: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential_scope: Option<&'a CredentialScope>,
    /// The disk the stick is written to. The Hub counts a second write of it as a rewrite.
    #[serde(skip_serializing_if = "Option::is_none")]
    hardware: Option<&'a HardwareId>,
    lifetime_minutes: i64,
}

pub fn mint_enrolment_code(
    hub: &str,
    session: &HubSession,
    stick_id: &StickId,
    settings: &Settings,
    machine_number: u32,
    credential_scope: Option<&CredentialScope>,
    hardware: Option<&HardwareId>,
) -> Result<MintedEnrolmentCode, Error> {
    let hub = checked_base(hub)?;
    let bearer = session.authorization()?;
    let request = EnrolmentRequest {
        stick_id,
        team: settings.team.trim(),
        site: settings.site.trim(),
        contest: &settings.contest.name,
        machine_number,
        credential_scope,
        hardware,
        lifetime_minutes: ENROLMENT_CODE_MINUTES,
    };
    let mut answer = agent()
        .post(session.org_url(hub, "/enrolment-codes")?)
        .header(AUTHORIZATION, bearer)
        .send_json(&request)
        .map_err(|_| Error::Hub("cannot reach the Hub to make an enrolment code".into()))?;
    let message = match answer.status().as_u16() {
        201 => {
            return answer
                .body_mut()
                .with_config()
                .limit(4096)
                .read_json()
                .map_err(|_| Error::Hub("the Hub returned an invalid enrolment code".into()));
        }
        401 => return Err(Error::SignedOut("sign in to the Hub again".into())),
        403 => "this account cannot make enrolment codes".into(),
        409 => "making enrolment codes in Forge requires a sign-in with one organization".into(),
        429 => "too many enrolment codes requested. Try again in a minute".into(),
        status => format!("the Hub returned HTTP {status} while making an enrolment code"),
    };
    Err(Error::Hub(message))
}

#[derive(Serialize)]
struct HardwareWriteRequest<'a> {
    stick_id: &'a StickId,
    #[serde(flatten)]
    hardware: &'a HardwareId,
}

/// Tells the Hub which disk a stick whose id is already known was written to, so it can show the
/// stick as rewritten when the disk is written again. Only the count is kept; a failure here
/// never changes what was written.
pub fn record_stick_write(
    hub: &str,
    session: &HubSession,
    stick_id: &StickId,
    hardware: &HardwareId,
) -> Result<(), Error> {
    let base = checked_base(hub)?;
    let bearer = session.authorization()?;
    let answer = agent()
        .post(session.org_url(base, "/hardware-writes")?)
        .header(AUTHORIZATION, bearer)
        .send_json(&HardwareWriteRequest { stick_id, hardware })
        .map_err(|e| unreachable_hub(base, &e))?;
    match answer.status().as_u16() {
        200 => Ok(()),
        401 => Err(Error::SignedOut("sign in again to record the disk".into())),
        403 => Err(Error::Hub("this account cannot record disks".into())),
        status => Err(Error::Hub(format!(
            "the Hub answered {status} while recording the disk"
        ))),
    }
}

/// A stick as the Hub lists it, with only what Forge needs to recognise a disk it wrote before.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HubStick {
    pub stick_id: String,
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub site: String,
    #[serde(default)]
    pub contest: String,
    #[serde(default)]
    pub machine_number: Option<u32>,
    /// Set when the key is already retired.
    #[serde(default)]
    pub retired_at: Option<String>,
    /// Set while the stick is archived.
    #[serde(default)]
    pub archived_at: Option<String>,
    /// The disk this stick was written to, when Forge reported it.
    #[serde(default)]
    pub hardware: Option<HubStickDisk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HubStickDisk {
    /// The first 8 hex digits of the disk's fingerprint.
    pub id: String,
    pub write_count: u32,
}

/// Every stick of the organization the admin is signed in to.
pub fn list_sticks(hub: &str, session: &HubSession) -> Result<Vec<HubStick>, Error> {
    let base = checked_base(hub)?;
    let mut answer = agent()
        .get(session.org_url(base, "/sticks")?)
        .header(AUTHORIZATION, session.authorization()?)
        .call()
        .map_err(|e| unreachable_hub(base, &e))?;
    match answer.status().as_u16() {
        200 => answer
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_json()
            .map_err(|e| Error::Hub(format!("unreadable answer from the Hub: {e}"))),
        401 => Err(Error::SignedOut("sign in again to see the sticks".into())),
        403 => Err(Error::Hub("this account cannot see the sticks".into())),
        409 => Err(Error::Hub(
            "listing sticks in Forge requires a sign-in with one organization".into(),
        )),
        status => Err(Error::Hub(format!(
            "the Hub answered {status} while listing the sticks"
        ))),
    }
}

#[derive(Serialize)]
struct SignRequest {
    config: String,
}

#[derive(Deserialize)]
struct SignAnswer {
    signature: String,
}

/// Asks the Hub to sign the exact bytes of a stick's settings file and returns the text of the
/// signature file that goes next to it. The Hub signs only for an admin, and only settings of a
/// stick that is not another organization's.
pub fn sign_settings(hub: &str, session: &HubSession, settings: &[u8]) -> Result<String, Error> {
    let base = checked_base(hub)?;
    let bearer = session.authorization()?;
    let request = SignRequest {
        config: base64::engine::general_purpose::STANDARD.encode(settings),
    };
    let mut answer = agent()
        .post(session.org_url(base, "/config-signatures")?)
        .header(AUTHORIZATION, bearer)
        .send_json(&request)
        .map_err(|e| unreachable_hub(base, &e))?;
    match answer.status().as_u16() {
        200 => answer
            .body_mut()
            .with_config()
            .limit(4096)
            .read_json::<SignAnswer>()
            .map(|answer| answer.signature)
            .map_err(|_| Error::Hub("the Hub returned an unreadable signature".into())),
        401 => Err(Error::SignedOut(
            "sign in again to get the settings signed".into(),
        )),
        403 => Err(Error::Hub("this account cannot get settings signed".into())),
        409 => Err(Error::Hub(
            "the Hub will not sign settings for a stick id that belongs to another organization"
                .into(),
        )),
        429 => Err(Error::Hub(
            "too many settings signed in a minute. Try again shortly".into(),
        )),
        503 => Err(Error::Hub(
            "this Hub has no settings signing key yet, so it cannot sign settings".into(),
        )),
        status => Err(Error::Hub(format!(
            "the Hub answered {status} while signing the settings"
        ))),
    }
}

#[derive(Serialize)]
struct ConfigHashRequest<'a> {
    stick_id: &'a StickId,
    sha256: &'a str,
}

/// Tells the Hub the SHA-256 of the settings file written for a stick, so it can warn when the stick
/// later reports another one. Not telling it never changes what was written: that stick is then
/// only not checked.
pub fn record_config_hash(
    hub: &str,
    session: &HubSession,
    stick_id: &StickId,
    sha256: &str,
) -> Result<(), Error> {
    let base = checked_base(hub)?;
    let bearer = session.authorization()?;
    let answer = agent()
        .post(session.org_url(base, "/config-hashes")?)
        .header(AUTHORIZATION, bearer)
        .send_json(&ConfigHashRequest { stick_id, sha256 })
        .map_err(|e| unreachable_hub(base, &e))?;
    match answer.status().as_u16() {
        204 => Ok(()),
        401 => Err(Error::SignedOut(
            "sign in again to record the settings".into(),
        )),
        403 => Err(Error::Hub("this account cannot record settings".into())),
        status => Err(Error::Hub(format!(
            "the Hub answered {status} while recording the settings"
        ))),
    }
}

/// Retires a previous stick key without removing its history from the Hub.
pub fn retire_stick_credential(hub: &str, session: &HubSession, id: &str) -> Result<(), Error> {
    let id = StickId::parse(id.trim()).map_err(|_| Error::Hub("enter a valid stick ID".into()))?;
    let base = checked_base(hub)?;
    let answer = agent()
        .delete(session.org_url(base, &format!("/sticks/{id}/credential"))?)
        .header(AUTHORIZATION, session.authorization()?)
        .call()
        .map_err(|e| unreachable_hub(base, &e))?;
    let status = answer.status().as_u16();
    match status {
        204 => Ok(()),
        401 => Err(Error::SignedOut(
            "sign in again to retire the stick key".into(),
        )),
        403 => Err(Error::Hub("this account cannot retire stick keys".into())),
        404 => Err(Error::NoSuchStick),
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
        .header(AUTHORIZATION, session.authorization()?)
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
    check_profile_id(id)?;
    let base = checked_base(hub)?;
    let bearer = session.authorization()?;
    let mut answer = agent()
        .get(format!("{base}/api/v1/profiles/{id}"))
        .header(AUTHORIZATION, bearer.clone())
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
                .header(AUTHORIZATION, bearer.clone())
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
    check_profile_id(id)?;
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
        .header(AUTHORIZATION, session.authorization()?)
        .send_json(json!({ "name": profile.name, "settings": settings, "wallpaper": wallpaper }))
        .map_err(|e| unreachable_hub(base, &e))?;
    check_profile_status(&mut answer)
}

/// Removes the profile `id` from the Hub.
pub fn delete_profile(hub: &str, session: &HubSession, id: &str) -> Result<(), Error> {
    check_profile_id(id)?;
    let base = checked_base(hub)?;
    let mut answer = agent()
        .delete(format!("{base}/api/v1/profiles/{id}"))
        .header(AUTHORIZATION, session.authorization()?)
        .call()
        .map_err(|e| unreachable_hub(base, &e))?;
    if answer.status().as_u16() == 404 {
        return Ok(());
    }
    check_profile_status(&mut answer)
}

/// The id goes into a Hub address, and a profile file from another admin can hold any `hub_id`,
/// so one with a `/`, `..` or `?` must not steer the admin's token to another Hub route. It is
/// also a folder name when a profile is loaded.
pub(crate) fn check_profile_id(id: &str) -> Result<(), Error> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(Error::Hub(
            "the Hub gave a profile id that cannot be used as a folder name".to_owned(),
        ));
    }
    Ok(())
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
        401 => {
            return Err(Error::SignedOut(
                "the sign-in ended; sign in again".to_owned(),
            ));
        }
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
    let Ok(bearer) = session.authorization() else {
        return;
    };
    let _ = agent()
        .post(format!("{base}/api/v1/logout"))
        .header(AUTHORIZATION, bearer)
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
    use super::{HubSession, safe_for_password};
    use std::path::Path;

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

    #[test]
    fn a_profile_id_cannot_steer_the_request_to_another_hub_route() {
        let session = HubSession::restore("token".to_owned(), "a@example.test".to_owned());
        // Port 1 is closed: a request that was sent would say "cannot reach", not "profile id".
        let hub = "http://127.0.0.1:1";
        let profile = super::HubProfile {
            name: String::new(),
            settings: serde_json::from_value(serde_json::json!({})).unwrap_or_default(),
            stick_key_window: None,
            wallpaper: None,
        };
        for id in ["../me", "a/b", "a?b", ""] {
            let fetched = super::fetch_profile(hub, &session, id)
                .unwrap_err()
                .to_string();
            let sent = super::upload_profile(hub, &session, id, &profile)
                .unwrap_err()
                .to_string();
            let removed = super::delete_profile(hub, &session, id)
                .unwrap_err()
                .to_string();
            assert!(
                fetched.contains("profile id")
                    && sent.contains("profile id")
                    && removed.contains("profile id"),
                "{id}"
            );
        }
    }

    #[test]
    fn the_authorization_header_is_sensitive_and_hidden_from_debug() {
        let session = HubSession::restore("secret-token".to_owned(), "a@example.test".to_owned());
        let header = session.authorization().unwrap();
        assert!(header.is_sensitive());
        assert_eq!(header.to_str().unwrap(), "Bearer secret-token");
        assert!(!format!("{header:?}").contains("secret-token"));
    }

    #[test]
    fn a_token_the_header_cannot_hold_is_refused_without_printing_it() {
        let session = HubSession::restore(
            "bad
token"
                .to_owned(),
            "a@example.test".to_owned(),
        );
        let error = session.authorization().unwrap_err();
        assert!(!format!("{error} {error:?}").contains("bad"));
    }

    #[test]
    fn no_source_builds_a_bearer_header_outside_hub_rs() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(src).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("hub.rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(!text.contains("\"Bearer "), "{}", path.display());
                assert!(!text.contains("\"authorization\""), "{}", path.display());
            }
        }
    }
}
