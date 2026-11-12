use ck_crypto::generate_stick_token;
use ck_protocol::{LocalDateTime, Problem, SCHEMA, Settings, StickConfig, StickId};
use uuid::Uuid;

use crate::error::Error;

/// The file for a new stick: the contest settings with a new id and secret, and its number among
/// the machines of the contest.
///
/// `now` is passed in so callers decide what the time is.
///
/// # Errors
/// Fails when the settings or the number have problems, so a stick the agent would refuse is never
/// made.
///
/// # Panics
/// Never: a UUID is always a valid stick id.
pub fn make_stick_config(
    settings: &Settings,
    machine_number: u32,
    now: LocalDateTime,
) -> Result<StickConfig, Error> {
    let stick_id = StickId::parse(&Uuid::new_v4().to_string()).expect("a UUID is a valid stick id");
    let config = StickConfig {
        schema: SCHEMA,
        stick_id,
        created_at: now,
        token: generate_stick_token(),
        machine_number: Some(machine_number),
        settings: settings.clone(),
    };
    let problems: Vec<Problem> = config.validate();
    if !problems.is_empty() {
        return Err(Error::InvalidProfile(problems));
    }
    Ok(config)
}
