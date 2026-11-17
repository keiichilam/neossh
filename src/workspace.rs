//! Persisted tab descriptions; authentication credentials have no field here.
use crate::config::{Config, Profile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTab {
    pub tab_color: String,
    pub title: String,
    pub profile_name: String,
    pub target: String,
    pub remote_directory: String,
    #[serde(flatten)]
    pub connection: Profile,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub tabs: Vec<SavedTab>,
    pub active: usize,
}

impl Snapshot {
    pub fn load(config: &Config) -> Result<Self, String> {
        config
            .get("last_workspace")
            .map(|value| {
                value
                    .clone()
                    .try_into()
                    .map_err(|_| "Saved workspace has invalid tab settings.".into())
            })
            .unwrap_or_else(|| Ok(Self::default()))
    }
    pub fn store(&self, config: &mut Config) -> Result<(), String> {
        config.set(
            "last_workspace",
            toml::Value::try_from(self).map_err(|error| error.to_string())?,
        );
        Ok(())
    }
}

pub fn enabled(config: &Config, key: &str, default: bool) -> bool {
    config
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_flat_tabs_load_and_saved_extensions_cannot_reintroduce_credentials() {
        let mut config = Config::parse("[last_workspace]\nactive = 0\n[[last_workspace.tabs]]\ntitle = 'prod'\nprofile_name = 'prod'\nhost = 'example.test'\nport = 2222\ntmux_session = 'work'\npassword = 'discard-me'\n").unwrap();
        let snapshot = Snapshot::load(&config).unwrap();
        assert_eq!(snapshot.tabs[0].connection.port, 2222);
        assert_eq!(snapshot.tabs[0].connection.tmux_session, "work");
        snapshot.store(&mut config).unwrap();
        assert!(!config.encode().unwrap().contains("discard-me"));
        assert_eq!(
            Snapshot::load(&config).unwrap().tabs[0].profile_name,
            "prod"
        );
    }
}
