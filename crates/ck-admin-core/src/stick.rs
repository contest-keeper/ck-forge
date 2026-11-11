use ck_crypto::generate_stick_token;
use ck_protocol::{LocalDateTime, SCHEMA, Settings, StickConfig, StickId};
use uuid::Uuid;

/// The file for a new stick: the contest settings with a new id and secret.
///
/// `now` is passed in so callers decide what the time is.
///
/// # Panics
/// Never: a UUID is always a valid stick id.
#[must_use]
pub fn make_stick_config(settings: &Settings, now: LocalDateTime) -> StickConfig {
    let stick_id = StickId::parse(&Uuid::new_v4().to_string()).expect("a UUID is a valid stick id");
    StickConfig {
        schema: SCHEMA,
        stick_id,
        created_at: now,
        token: generate_stick_token(),
        settings: settings.clone(),
    }
}
