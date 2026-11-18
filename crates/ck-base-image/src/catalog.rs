//! The base images Keeper provides. The list is part of the program, so an admin can only
//! download from addresses that were reviewed and released with it.

use serde::{Deserialize, Serialize};

use crate::Error;

const CATALOG: &str = include_str!("../data/catalog.json");

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub label: String,
    /// The official image to start from.
    pub url: String,
    /// What the download must hash to, so a changed or swapped file is refused.
    pub sha256: String,
    /// Size of the finished image. The stick must be at least this big.
    pub disk_gb: u32,
    /// Name of the script that makes the image a contest desktop.
    pub provision: String,
}

impl Entry {
    /// The provisioning script, which is compiled into the program like the catalog.
    pub fn script(&self) -> Result<&'static str, Error> {
        match self.provision.as_str() {
            "ubuntu-24.04.sh" => Ok(include_str!("../data/ubuntu-24.04.sh")),
            other => Err(Error::UnknownDistro(other.to_owned())),
        }
    }
}

pub fn catalog() -> Result<Vec<Entry>, Error> {
    Ok(serde_json::from_str(CATALOG)?)
}

pub fn find(id: &str) -> Result<Entry, Error> {
    for entry in catalog()? {
        if entry.id == id {
            return Ok(entry);
        }
    }
    Err(Error::UnknownDistro(id.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_complete_and_has_a_script() {
        let entries = catalog().expect("the catalog parses");
        find("ubuntu-24.04").expect("the first catalog entry");
        for entry in entries {
            assert!(entry.url.starts_with("https://"), "{}", entry.id);
            assert_eq!(entry.sha256.len(), 64, "{}", entry.id);
            assert!(entry.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(entry.script().expect("script").starts_with("#!/bin/bash"));
        }
    }

    #[test]
    fn unknown_id_is_refused() {
        assert!(matches!(find("nope"), Err(Error::UnknownDistro(_))));
    }
}
