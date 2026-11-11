use std::path::Path;

use ck_protocol::{LocalDateTime, StickId, Token};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::file::{read_json, write_json_atomically};

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
}

/// Every stick made so far. It holds the sticks' secrets, so the file must be kept private.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Registry {
    sticks: Vec<RegistryEntry>,
}

impl Registry {
    /// Reads the registry. A missing file is an empty registry.
    ///
    /// # Errors
    /// Fails when the file exists but cannot be read.
    pub fn load(path: &Path) -> Result<Self, Error> {
        Ok(read_json(path)?.unwrap_or_default())
    }

    /// # Errors
    /// Fails when the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        write_json_atomically(path, self)
    }

    /// # Errors
    /// Fails when a stick with the same id is already there.
    pub fn add(&mut self, entry: RegistryEntry) -> Result<(), Error> {
        if self.find(&entry.stick_id).is_some() {
            return Err(Error::DuplicateStick(entry.stick_id));
        }
        self.sticks.push(entry);
        Ok(())
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
