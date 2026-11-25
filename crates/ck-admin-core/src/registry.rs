use std::path::Path;

use ck_protocol::{LocalDateTime, StickId, Token};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::Error;
use crate::file::{Lock, read_json, write_json_atomically};

/// The contest and UTC validity interval of a newly assigned stick key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialScope {
    pub contest_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub valid_from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub valid_until: OffsetDateTime,
}

/// One stick the admin has made. The Hub checks signatures with its token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub stick_id: StickId,
    pub token: Token,
    pub team: String,
    pub site: String,
    pub contest: String,
    pub image: String,
    pub created_at: LocalDateTime,
    /// The stick's number among the machines of its contest. Older entries have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_scope: Option<CredentialScope>,
}

/// Every stick made so far. It holds the sticks' secrets, so the file must be kept private.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Registry {
    sticks: Vec<RegistryEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    enrolments: Vec<EnrolmentReservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EnrolmentReservation {
    stick_id: StickId,
    contest: String,
    machine_number: u32,
}

impl Registry {
    /// A missing file is an empty registry.
    pub fn load(path: &Path) -> Result<Self, Error> {
        Ok(read_json(path)?.unwrap_or_default())
    }

    pub fn save(&self, path: &Path) -> Result<(), Error> {
        write_json_atomically(path, self)
    }

    /// Refuses a stick whose id is already there.
    pub fn add(&mut self, entry: RegistryEntry) -> Result<(), Error> {
        if self.contains(&entry.stick_id) {
            return Err(Error::DuplicateStick(entry.stick_id));
        }
        self.sticks.push(entry);
        Ok(())
    }

    /// Adds a stick to the registry file and saves it. Safe when other programs do the same at the
    /// same time: the file is locked, then read again, so nobody's sticks are lost.
    pub fn register(path: &Path, entry: RegistryEntry) -> Result<(), Error> {
        let _lock = Lock::acquire(path)?;
        let mut registry = Self::load(path)?;
        registry.add(entry)?;
        registry.save(path)
    }

    pub(crate) fn reserve_enrolment(
        path: &Path,
        stick_id: &StickId,
        contest: &str,
    ) -> Result<u32, Error> {
        let _lock = Lock::acquire(path)?;
        let mut registry = Self::load(path)?;
        if registry.contains(stick_id) {
            return Err(Error::DuplicateStick(stick_id.clone()));
        }
        let machine_number = registry.next_machine_number(contest);
        if i32::try_from(machine_number).is_err() {
            return Err(Error::EnrolmentNumbersExhausted);
        }
        registry.enrolments.push(EnrolmentReservation {
            stick_id: stick_id.clone(),
            contest: contest.to_owned(),
            machine_number,
        });
        registry.save(path)?;
        Ok(machine_number)
    }

    /// Updates the team, site and contest of a stick that is already in the registry file, for
    /// example after its settings were changed. Returns false when the file or the stick is not there.
    pub fn update_labels(
        path: &Path,
        id: &StickId,
        team: &str,
        site: &str,
        contest: &str,
    ) -> Result<bool, Error> {
        if !path.exists() {
            return Ok(false);
        }
        let _lock = Lock::acquire(path)?;
        let mut registry = Self::load(path)?;
        let Some(entry) = registry.sticks.iter_mut().find(|e| &e.stick_id == id) else {
            return Ok(false);
        };
        team.clone_into(&mut entry.team);
        site.clone_into(&mut entry.site);
        contest.clone_into(&mut entry.contest);
        registry.save(path)?;
        Ok(true)
    }

    /// Takes a stick out of the registry file. A stick whose write failed must not stay there: every
    /// later push sends the whole file, so the Hub would take in a stick that was never written.
    /// Returns false when the file or the stick is not there.
    pub fn remove(path: &Path, id: &StickId) -> Result<bool, Error> {
        if !path.exists() {
            return Ok(false);
        }
        let _lock = Lock::acquire(path)?;
        let mut registry = Self::load(path)?;
        let before = registry.sticks.len();
        registry.sticks.retain(|entry| &entry.stick_id != id);
        if registry.sticks.len() == before {
            return Ok(false);
        }
        registry.save(path)?;
        Ok(true)
    }

    /// The number for the next stick of `contest`: one more than any number used so far, and never
    /// less than the count of sticks already made for it, so older entries without a number are
    /// not given the same number twice.
    #[must_use]
    pub fn next_machine_number(&self, contest: &str) -> u32 {
        let mut count = 0_u32;
        let mut highest = 0_u32;
        let old_numbers = self
            .sticks
            .iter()
            .map(|entry| (entry.contest.as_str(), entry.machine_number.unwrap_or(0)));
        let enrolment_numbers = self
            .enrolments
            .iter()
            .map(|entry| (entry.contest.as_str(), entry.machine_number));
        for (name, number) in old_numbers.chain(enrolment_numbers) {
            if name != contest {
                continue;
            }
            count += 1;
            highest = highest.max(number);
        }
        count.max(highest).saturating_add(1)
    }

    #[must_use]
    pub fn list(&self) -> &[RegistryEntry] {
        &self.sticks
    }

    #[must_use]
    pub fn find(&self, id: &StickId) -> Option<&RegistryEntry> {
        self.sticks.iter().find(|entry| &entry.stick_id == id)
    }

    fn contains(&self, id: &StickId) -> bool {
        self.find(id).is_some() || self.enrolments.iter().any(|entry| &entry.stick_id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> RegistryEntry {
        serde_json::from_value(serde_json::json!({
            "stick_id": id, "token": "0123456789abcdef", "team": "", "site": "",
            "contest": "Contest", "image": "", "created_at": "2026-11-19T11:08:13"
        }))
        .expect("entry")
    }

    #[test]
    fn a_removed_stick_is_gone_from_the_file_and_the_others_stay() {
        let folder = tempfile::tempdir().expect("temp folder");
        let path = folder.path().join("registry.json");
        let (a, b) = (
            StickId::parse("stick-a").expect("id"),
            StickId::parse("stick-b").expect("id"),
        );
        assert!(!Registry::remove(&path, &a).expect("no file"));
        Registry::register(&path, entry("stick-a")).expect("add a");
        Registry::register(&path, entry("stick-b")).expect("add b");
        assert!(Registry::remove(&path, &a).expect("remove a"));
        assert!(!Registry::remove(&path, &a).expect("remove a again"));
        let left = Registry::load(&path).expect("registry");
        assert!(left.find(&a).is_none() && left.find(&b).is_some());
    }

    #[test]
    fn enrolment_reservations_number_each_machine_without_storing_a_credential() {
        let folder = tempfile::tempdir().expect("temp folder");
        let path = folder.path().join("registry.json");
        let first = StickId::parse("first").expect("stick id");
        let second = StickId::parse("second").expect("stick id");
        let mut numbers = std::thread::scope(|scope| {
            let a = scope.spawn(|| Registry::reserve_enrolment(&path, &first, "Contest"));
            let b = scope.spawn(|| Registry::reserve_enrolment(&path, &second, "Contest"));
            [
                a.join().expect("first thread").expect("first number"),
                b.join().expect("second thread").expect("second number"),
            ]
        });
        numbers.sort_unstable();
        assert_eq!(numbers, [1, 2]);
        assert_eq!(
            Registry::load(&path)
                .expect("registry")
                .next_machine_number("Contest"),
            3
        );
        assert!(matches!(
            Registry::reserve_enrolment(&path, &first, "Contest"),
            Err(Error::DuplicateStick(_))
        ));
        let document = std::fs::read_to_string(&path).expect("registry JSON");
        assert!(!document.contains("token"));
        assert!(!document.contains("code"));
    }
}
