use std::path::Path;

use ck_protocol::{LocalDateTime, StickId, Token};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::file::{Lock, read_json, write_json_atomically};

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
}

/// Every stick made so far. It holds the sticks' secrets, so the file must be kept private.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Registry {
    sticks: Vec<RegistryEntry>,
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
        if self.find(&entry.stick_id).is_some() {
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

    /// The number for the next stick of `contest`: one more than any number used so far, and never
    /// less than the count of sticks already made for it, so older entries without a number are
    /// not given the same number twice.
    #[must_use]
    pub fn next_machine_number(&self, contest: &str) -> u32 {
        let mut count = 0_u32;
        let mut highest = 0_u32;
        for entry in self.sticks.iter().filter(|e| e.contest == contest) {
            count += 1;
            highest = highest.max(entry.machine_number.unwrap_or(0));
        }
        count.max(highest) + 1
    }

    #[must_use]
    pub fn list(&self) -> &[RegistryEntry] {
        &self.sticks
    }

    #[must_use]
    pub fn find(&self, id: &StickId) -> Option<&RegistryEntry> {
        self.sticks.iter().find(|entry| &entry.stick_id == id)
    }
}
