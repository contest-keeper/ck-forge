//! The "Contest profile" page, laid out as `ContestPage.svelte` of the earlier web window.

mod datetime;
mod forms;
pub mod hub_profiles;
mod preview;
pub mod profile;
#[cfg(test)]
mod shots;

pub use profile::Screen as Page;
