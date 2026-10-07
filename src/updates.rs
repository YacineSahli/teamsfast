//! Self-update from GitHub releases, through fastframe-update.
//!
//! The crate checks for a newer release, refuses package-managed copies,
//! downloads and verifies the update against the publisher signature, and
//! hands it to a helper that installs it, relaunches and rolls back if the
//! new version does not start. TeamsFast keeps its names, its key and the
//! interface.

pub use fastframe_update::{
    DownloadState, Installation, Kind, Prepared, Release, Unsupported, Updater,
};
use fastframe_update::{MacConfig, ReqwestTransport, UpdateConfig};

/// TeamsFast's releases and the names its installations have had.
pub const CONFIG: UpdateConfig = UpdateConfig {
    macos: MacConfig {
        bundle_ids: &[],
        executable_names: &[],
        legacy_bundle_names: &[],
    },
    publisher_key: Some(include_str!("../assets/update-public-key.hex")),
    ..UpdateConfig::new(
        "YacineSahli/teamsfast",
        "TeamsFast",
        "teamsfast",
        env!("CARGO_PKG_VERSION"),
    )
};

/// An updater on a plain blocking client (no proxy configuration yet).
pub fn updater() -> anyhow::Result<Updater> {
    Ok(Updater::new(
        CONFIG,
        ReqwestTransport::new(reqwest::blocking::Client::builder())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_config_is_valid() {
        CONFIG.validate().unwrap();
        assert_eq!(CONFIG.current_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(CONFIG.slug, "teamsfast");
    }

    #[test]
    fn the_updater_starts_on_github() {
        assert!(updater().unwrap().source().is_github());
    }
}
