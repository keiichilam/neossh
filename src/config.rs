//! Lossless configuration document with typed profiles and atomic persistence.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Default configuration file name in the user's home directory.
pub const CONFIG_FILE: &str = ".neo-ssh.toml";
/// File shared by ssh4, ssh4gui and ssh5; imported read-only on first run.
pub const LEGACY_CONFIG_FILE: &str = ".ssh4.toml";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub host: String,
    pub user: String,
    pub port: u16,
    #[serde(alias = "identity_file")]
    pub key_path: String,
    pub use_agent: bool,
    pub strict_host_key: bool,
    pub tmux_session: String,
    pub jump: String,
    pub term: String,
    pub remote_env: String,
    pub keep_alive_secs: u64,
    pub connect_timeout_secs: u64,
    pub preferred_auth: String,
    pub local_forwards: Vec<String>,
    pub remote_forwards: Vec<String>,
    pub dynamic_forwards: Vec<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, toml::Value>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            host: String::new(),
            user: String::new(),
            port: 22,
            key_path: String::new(),
            use_agent: true,
            strict_host_key: false,
            tmux_session: String::new(),
            jump: String::new(),
            term: String::new(),
            remote_env: String::new(),
            keep_alive_secs: 0,
            connect_timeout_secs: 0,
            preferred_auth: String::new(),
            local_forwards: Vec::new(),
            remote_forwards: Vec::new(),
            dynamic_forwards: Vec::new(),
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
pub struct Config {
    root: toml::Table,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Snippet {
    pub name: String,
    #[serde(alias = "body")]
    pub command: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, toml::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self::parse("ui_zoom = 1.0\nterminal_font_size = 15.0\ntheme = 'Midnight'\nterminal_scrollback = true\nrestore_workspace = true\nask_before_reconnect_on_restore = true\n[profiles]\n").expect("built-in config")
    }
}

impl Config {
    pub fn path() -> io::Result<PathBuf> {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .filter(|path| !path.is_empty())
            .map(|home| PathBuf::from(home).join(CONFIG_FILE))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "Home directory is unavailable.")
            })
    }

    /// Configuration written by ssh4, ssh4gui and ssh5, read when the NEO-SSH file
    /// does not exist yet: the `.ssh4.toml` in the same folder. Only a file with
    /// the default name `.neo-ssh.toml` imports; any other `--config` file name
    /// never does.
    pub fn legacy_path(path: &Path) -> Option<PathBuf> {
        (path.file_name()? == CONFIG_FILE).then(|| path.with_file_name(LEGACY_CONFIG_FILE))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut root: toml::Table = toml::from_str(text).map_err(|error: toml::de::Error| {
            format!(
                "Invalid configuration near byte {}. Correct the TOML before saving.",
                error.span().map_or(0, |span| span.start)
            )
        })?;
        // Older builds used an array; preserve every field while indexing by name.
        if let Some(toml::Value::Array(profiles)) = root.get("profiles") {
            let mut table = toml::Table::new();
            for profile in profiles {
                let mut fields = profile
                    .as_table()
                    .ok_or("Each profile must be a table.")?
                    .clone();
                let name = fields
                    .remove("name")
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .filter(|name| !name.trim().is_empty())
                    .ok_or("Profile is missing its name.")?;
                if table
                    .insert(name.clone(), toml::Value::Table(fields))
                    .is_some()
                {
                    return Err(format!("Duplicate profile name: {name}"));
                }
            }
            root.insert("profiles".into(), toml::Value::Table(table));
        }
        // ssh4 wrote `identity_file`, later apps `key_path`, and they shared one
        // file, so a profile can hold both; serde rejects that as a duplicate
        // field, which would refuse the whole file. Keep one: a non-empty
        // `key_path`, else the `identity_file` value.
        if let Some(toml::Value::Table(profiles)) = root.get_mut("profiles") {
            for (_, profile) in profiles.iter_mut() {
                let toml::Value::Table(fields) = profile else {
                    continue;
                };
                if let Some(legacy) = fields.remove("identity_file") {
                    let empty = fields
                        .get("key_path")
                        .and_then(toml::Value::as_str)
                        .is_none_or(str::is_empty);
                    if empty {
                        fields.insert("key_path".into(), legacy);
                    }
                }
            }
        }
        let mut value = toml::Value::Table(root);
        strip_secrets(&mut value);
        let config = Self {
            root: value.as_table().unwrap().clone(),
        };
        config.profiles()?;
        config.snippets()?;
        Ok(config)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::import_legacy(path)),
            Err(error) => Err(format!("Cannot read configuration: {error}")),
        }
    }

    /// First run: adopt profiles, snippets and preferences from `~/.ssh4.toml`.
    /// The legacy file is only read; the first save writes the NEO-SSH file.
    /// An unreadable or invalid legacy file is ignored rather than blocking startup.
    fn import_legacy(path: &Path) -> Self {
        let Some(text) = Self::legacy_path(path).and_then(|legacy| fs::read_to_string(legacy).ok())
        else {
            return Self::default();
        };
        // ssh4gui resolved `port = 0` through the ssh_config next to it.
        let imports = path
            .parent()
            .and_then(|home| crate::ssh_config::SshConfig::load(&home.join(".ssh/config")).ok());
        let configured_port = |alias: &str| {
            imports
                .as_ref()?
                .resolve(alias)
                .get("port")?
                .parse::<u16>()
                .ok()
                .filter(|port| *port != 0)
        };
        Self::parse(&text)
            .map(|config| config.migrate_legacy_with(&configured_port))
            .unwrap_or_default()
    }

    /// Translate settings the older apps expressed differently (see
    /// [`Self::migrate_legacy_with`]); no ssh_config is consulted for ports.
    pub fn migrate_legacy(self) -> Self {
        self.migrate_legacy_with(&|_| None)
    }

    /// Translate settings the older apps expressed differently:
    /// - ssh4 `auto_tmux = true` without a session name attached to "ssh4";
    /// - ssh4gui's global `reconnect_countdown_secs` becomes each profile's
    ///   `reconnect_delay_secs` unless the profile already sets one;
    /// - ssh4gui accepted `user@host:port` in `host` and saved `port = 0` for
    ///   "the ssh_config Port, else the port in `host`, else 22". The host is
    ///   split into its parts and the port resolved with `configured_port`
    ///   (ssh_config `Port` for a host alias), because NEO-SSH profiles need a
    ///   real port.
    ///
    /// The legacy file itself is never rewritten.
    pub fn migrate_legacy_with(mut self, configured_port: &dyn Fn(&str) -> Option<u16>) -> Self {
        let countdown = self
            .root
            .get("reconnect_countdown_secs")
            .and_then(toml::Value::as_integer)
            .filter(|seconds| *seconds > 0)
            .map(|seconds| seconds.min(300));
        if let Some(profiles) = self
            .root
            .get_mut("profiles")
            .and_then(toml::Value::as_table_mut)
        {
            for (_, profile) in profiles.iter_mut() {
                let Some(profile) = profile.as_table_mut() else {
                    continue;
                };
                let auto_tmux = profile
                    .get("auto_tmux")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false);
                let named = profile
                    .get("tmux_session")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|name| !name.trim().is_empty());
                if auto_tmux && !named {
                    profile.insert("tmux_session".into(), "ssh4".into());
                }
                if let Some(seconds) = countdown {
                    profile
                        .entry("reconnect_delay_secs")
                        .or_insert(toml::Value::Integer(seconds));
                }
                let spec = profile
                    .get("host")
                    .and_then(toml::Value::as_str)
                    .and_then(|host| host.parse::<crate::target::Target>().ok());
                if let Some(spec) = spec {
                    let no_user = profile
                        .get("user")
                        .and_then(toml::Value::as_str)
                        .is_none_or(|user| user.trim().is_empty());
                    if let Some(user) = spec.user.filter(|_| no_user) {
                        profile.insert("user".into(), user.into());
                    }
                    if profile.get("port").and_then(toml::Value::as_integer) == Some(0) {
                        let port = configured_port(&spec.host).unwrap_or(spec.port);
                        profile.insert("port".into(), toml::Value::Integer(port.into()));
                    }
                    profile.insert("host".into(), spec.host.into());
                }
            }
        }
        self
    }

    pub fn profiles(&self) -> Result<BTreeMap<String, Profile>, String> {
        self.root
            .get("profiles")
            .map(|value| {
                value
                    .clone()
                    .try_into()
                    .map_err(|error| format!("Invalid profiles: {error}"))
            })
            .unwrap_or_else(|| Ok(BTreeMap::new()))
    }

    pub fn set_profile(&mut self, name: &str, profile: &Profile) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("Profile name is required.".into());
        }
        if profile.host.trim().is_empty() || profile.port == 0 {
            return Err("Profile requires a host and nonzero port.".into());
        }
        if !profile.tmux_session.trim().is_empty() {
            crate::target::tmux_command(&profile.tmux_session)?;
        }
        let mut value = toml::Value::try_from(profile).map_err(|error| error.to_string())?;
        strip_secrets(&mut value);
        self.root
            .entry("profiles")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or("Profiles must be a table.")?
            .insert(name.to_owned(), value);
        Ok(())
    }

    pub fn remove_profile(&mut self, name: &str) {
        if let Some(table) = self
            .root
            .get_mut("profiles")
            .and_then(toml::Value::as_table_mut)
        {
            table.remove(name);
        }
    }

    /// Keep the live profile list unchanged unless persistence succeeds.
    pub fn save_profile(
        &mut self,
        path: &Path,
        name: &str,
        profile: &Profile,
    ) -> Result<(), String> {
        let mut updated = self.clone();
        updated.set_profile(name, profile)?;
        updated.save(path)?;
        *self = updated;
        Ok(())
    }

    /// Rename and update together without losing the original on write failure.
    pub fn rename_profile(
        &mut self,
        path: &Path,
        original: &str,
        name: &str,
        profile: &Profile,
    ) -> Result<(), String> {
        let profiles = self.profiles()?;
        if !profiles.contains_key(original) {
            return Err(format!("No saved connection named '{original}'."));
        }
        if original != name && profiles.contains_key(name) {
            return Err(format!("A saved connection named '{name}' already exists."));
        }
        let mut updated = self.clone();
        updated.set_profile(name, profile)?;
        if original != name {
            updated.remove_profile(original);
        }
        updated.save(path)?;
        *self = updated;
        Ok(())
    }

    /// Remove only the saved profile, committing live state after the file write.
    pub fn delete_profile(&mut self, path: &Path, name: &str) -> Result<(), String> {
        if !self.profiles()?.contains_key(name) {
            return Err(format!("No saved connection named '{name}'."));
        }
        let mut updated = self.clone();
        updated.remove_profile(name);
        updated.save(path)?;
        *self = updated;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&toml::Value> {
        self.root.get(key)
    }

    pub fn snippets(&self) -> Result<Vec<Snippet>, String> {
        self.root
            .get("snippets")
            .map(|value| {
                value.clone().try_into().map_err(|_| {
                    "Invalid snippets: expected an array with name and command fields.".into()
                })
            })
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    pub fn set_snippets(&mut self, snippets: &[Snippet]) -> Result<(), String> {
        if snippets
            .iter()
            .any(|snippet| snippet.name.trim().is_empty() || snippet.command.is_empty())
        {
            return Err("Each snippet needs a name and command.".into());
        }
        let value = toml::Value::try_from(snippets).map_err(|error| error.to_string())?;
        self.set("snippets", value);
        Ok(())
    }

    pub fn set(&mut self, key: &str, mut value: toml::Value) {
        if secret_key(key) {
            return;
        }
        strip_secrets(&mut value);
        self.root.insert(key.into(), value);
    }

    pub fn encode(&self) -> Result<String, String> {
        let mut value = toml::Value::Table(self.root.clone());
        strip_secrets(&mut value);
        toml::to_string_pretty(&value).map_err(|error| error.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let encoded = self.encode()?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        // The caller creates directories intentionally; failed saves never truncate the original.
        let mut staging =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staging
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| error.to_string())?;
        }
        staging
            .write_all(encoded.as_bytes())
            .and_then(|_| staging.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        staging.persist(path).map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn secret_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "password" | "passphrase" | "private_key" | "private_key_contents"
    )
}

fn strip_secrets(value: &mut toml::Value) {
    match value {
        toml::Value::Table(table) => {
            table.retain(|key, _| !secret_key(key));
            for (_, value) in table.iter_mut() {
                strip_secrets(value);
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                strip_secrets(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rename_preserves_original_on_failure_and_never_overwrites_another_profile() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        let blocked = root.path().join("blocked");
        fs::create_dir(&blocked).unwrap();
        let mut config = Config::parse("theme='Forest'\n[profiles.prod]\nhost='old.example'\nfuture_option=42\n[profiles.other]\nhost='other.example'\n").unwrap();
        config.save(&path).unwrap();
        let original = config.encode().unwrap();
        let bytes = fs::read(&path).unwrap();
        let mut profile = config.profiles().unwrap()["prod"].clone();
        profile.host = "edited.example".into();
        for (destination, old, new) in [
            (&blocked, "prod", "renamed"),
            (&path, "prod", "other"),
            (&path, "missing", "renamed"),
            (&path, "prod", " "),
        ] {
            assert!(config
                .rename_profile(destination, old, new, &profile)
                .is_err());
            assert_eq!(config.encode().unwrap(), original);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        config
            .rename_profile(&path, "prod", "renamed", &profile)
            .unwrap();
        config.save_profile(&path, "renamed", &profile).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.encode().unwrap(), config.encode().unwrap());
        let profiles = loaded.profiles().unwrap();
        assert_eq!(profiles.len(), 2);
        assert!(!profiles.contains_key("prod"));
        assert_eq!(profiles["renamed"].host, "edited.example");
        assert_eq!(profiles["other"].host, "other.example");
        assert!(loaded.encode().unwrap().contains("future_option = 42"));
        assert_eq!(loaded.get("theme").unwrap().as_str(), Some("Forest"));
    }

    #[test]
    fn profile_deletion_is_transactional_and_preserves_other_configuration() {
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join("blocked.toml");
        fs::create_dir(&blocked).unwrap();
        let marker = blocked.join("keep");
        fs::write(&marker, b"unchanged").unwrap();
        let mut config = Config::parse("theme='Forest'\n[profiles.prod]\nhost='prod.example'\n[profiles.other]\nhost='other.example'\nfuture_option=42\n").unwrap();
        let original = config.encode().unwrap();
        assert!(config.delete_profile(&blocked, "prod").is_err());
        assert_eq!(config.encode().unwrap(), original);
        assert_eq!(fs::read(&marker).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        let path = root.path().join("config.toml");
        config.delete_profile(&path, "prod").unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.encode().unwrap(), config.encode().unwrap());
        let profiles = loaded.profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles["other"].host, "other.example");
        assert_eq!(loaded.get("theme").unwrap().as_str(), Some("Forest"));
        assert!(loaded.encode().unwrap().contains("future_option = 42"));
        let saved = fs::read(&path).unwrap();
        assert!(config.delete_profile(&path, "prod").is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(config.encode().unwrap(), loaded.encode().unwrap());
    }

    #[test]
    fn failed_profile_save_preserves_live_state_and_retry_persists_once() {
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join("config.toml");
        fs::create_dir(&blocked).unwrap();
        let marker = blocked.join("existing-data");
        fs::write(&marker, b"preserve").unwrap();
        let mut config = Config::parse(
            "theme='Forest'\n[profiles.prod]\nhost='old.example'\nport=22\nfuture_option=42\n",
        )
        .unwrap();
        let original = config.encode().unwrap();
        let mut profile = config.profiles().unwrap()["prod"].clone();
        profile.host = "new.example".into();
        for name in ["prod", "new"] {
            assert!(config.save_profile(&blocked, name, &profile).is_err());
            assert_eq!(config.encode().unwrap(), original);
            assert_eq!(fs::read(&marker).unwrap(), b"preserve");
            assert_eq!(
                fs::read_dir(root.path()).unwrap().count(),
                1,
                "Staging file survived failure"
            );
        }
        let path = root.path().join("writable.toml");
        config.save_profile(&path, "prod", &profile).unwrap();
        config.save_profile(&path, "prod", &profile).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.encode().unwrap(), config.encode().unwrap());
        assert_eq!(loaded.profiles().unwrap().len(), 1);
        assert_eq!(loaded.profiles().unwrap()["prod"].host, "new.example");
        assert_eq!(loaded.get("theme").unwrap().as_str(), Some("Forest"));
        let saved = fs::read(&path).unwrap();
        profile.port = 0;
        assert!(config.save_profile(&path, "prod", &profile).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(config.encode().unwrap(), loaded.encode().unwrap());
    }

    #[test]
    fn snippets_round_trip_both_schemas_and_preserve_other_configuration() {
        let mut config = Config::parse("theme = 'custom'\n[[snippets]]\nname = 'logs'\ncommand = 'journalctl -f'\ncategory = 'ops'\n[[snippets]]\nname = 'status'\nbody = 'uname -a'\n").unwrap();
        let mut snippets = config.snippets().unwrap();
        assert_eq!(snippets[1].command, "uname -a");
        snippets[0].command = "journalctl -n 20\nuname -a".into();
        config.set_snippets(&snippets).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        config.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(
            loaded.get("theme").and_then(toml::Value::as_str),
            Some("custom")
        );
        let snippets = loaded.snippets().unwrap();
        assert_eq!(snippets[0].extensions["category"].as_str(), Some("ops"));
        assert_eq!(snippets[0].command, "journalctl -n 20\nuname -a");
        assert!(config.set_snippets(&[Snippet::default()]).is_err());
        config.set_snippets(&[]).unwrap();
        assert!(config.snippets().unwrap().is_empty());
    }

    #[test]
    fn editing_profile_preserves_extensions_workspace_and_snippets() {
        let mut config = Config::parse("future_setting = 'keep'\n[[snippets]]\nname = 'list'\ncommand = 'ls -la'\n[profiles.prod]\nhost = 'server'\ncolor = 'blue'\nfuture_option = 42\npassword = 'secret'\n[last_workspace]\nactive = 0\n[[last_workspace.tabs]]\nhost = 'server'\ntmux_session = 'deploy'\n").unwrap();
        let mut profile = config.profiles().unwrap().remove("prod").unwrap();
        profile.port = 2222;
        config.set_profile("prod", &profile).unwrap();
        let text = config.encode().unwrap();
        assert!(!text.contains("secret"));
        let loaded = Config::parse(&text).unwrap();
        let profile = loaded.profiles().unwrap().remove("prod").unwrap();
        assert_eq!(profile.port, 2222);
        assert_eq!(profile.extensions["future_option"].as_integer(), Some(42));
        assert_eq!(loaded.get("future_setting").unwrap().as_str(), Some("keep"));
        assert_eq!(loaded.get("snippets"), config.get("snippets"));
        assert_eq!(loaded.get("last_workspace"), config.get("last_workspace"));
    }

    #[test]
    fn accepts_both_profile_schemas_without_silent_duplicate_loss() {
        let config = Config::parse(
            "[[profiles]]\nname = 'dev'\nhost = 'localhost'\nidentity_file = '/key'\n",
        )
        .unwrap();
        assert_eq!(config.profiles().unwrap()["dev"].key_path, "/key");
        assert!(Config::parse("[[profiles]]\nname='x'\n[[profiles]]\nname='x'").is_err());
        assert!(Config::parse("profiles = false").is_err());
    }

    #[test]
    fn profiles_with_both_key_names_still_load() {
        // ssh4 wrote `identity_file` and later apps `key_path` into the shared
        // file; one profile can carry both. The non-empty `key_path` wins.
        for (text, expected) in [
            (
                "[profiles.dev]\nhost = 'h'\nkey_path = '/new'\nidentity_file = '/old'\n",
                "/new",
            ),
            (
                "[profiles.dev]\nhost = 'h'\nkey_path = ''\nidentity_file = '/old'\n",
                "/old",
            ),
            (
                "[[profiles]]\nname = 'dev'\nhost = 'h'\nidentity_file = '/old'\nkey_path = '/new'\n",
                "/new",
            ),
        ] {
            let config = Config::parse(text).unwrap();
            let profiles = config.profiles().unwrap();
            assert_eq!(profiles["dev"].key_path, expected, "{text}");
            let saved = toml::to_string(&config.root).unwrap();
            assert!(!saved.contains("identity_file"), "{saved}");
        }
    }

    #[test]
    fn legacy_nested_credentials_are_removed_on_disk_without_losing_extensions() {
        let mut config = Config::parse(
            r#"
Password = "TOP_SECRET_SENTINEL"
[[profiles]]
name = "legacy"
host = "server"
key_path = "/safe/key-path"
Passphrase = "PROFILE_SECRET_SENTINEL"
[profiles.extra]
keep = "extension survives"
Private_Key = "KEY_SECRET_SENTINEL"
[[last_workspace.tabs]]
host = "server"
password = "WORKSPACE_SECRET_SENTINEL"
[[last_workspace.tabs.extra]]
PRIVATE_KEY_CONTENTS = "NESTED_SECRET_SENTINEL"
keep = 42
"#,
        )
        .unwrap();
        config.set(
            "future_extension",
            toml::Value::Array(vec![toml::Value::Table(toml::Table::from_iter([
                (
                    "password".into(),
                    toml::Value::String("SET_SECRET_SENTINEL".into()),
                ),
                ("keep".into(), toml::Value::Boolean(true)),
            ]))]),
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "old settings").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        config.save(&path).unwrap();
        let encoded = std::fs::read_to_string(&path).unwrap();
        assert!(!encoded.contains("SECRET_SENTINEL"));
        let loaded = Config::load(&path).unwrap();
        assert_eq!(
            loaded.profiles().unwrap()["legacy"].key_path,
            "/safe/key-path"
        );
        assert!(encoded.contains("extension survives"));
        assert_eq!(
            loaded.get("future_extension").unwrap()[0]["keep"].as_bool(),
            Some(true)
        );
        assert_eq!(
            loaded.get("last_workspace").unwrap()["tabs"][0]["extra"][0]["keep"].as_integer(),
            Some(42)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn malformed_credentials_never_echo_source_in_configuration_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let source = "password = 'MALFORMED_SECRET_SENTINEL' unexpected";
        std::fs::write(&path, source).unwrap();
        let error = Config::load(&path).err().unwrap();
        assert!(error.starts_with("Invalid configuration near byte "));
        assert!(!error.contains("MALFORMED_SECRET_SENTINEL"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    }

    #[test]
    fn malformed_file_is_not_silently_replaced_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "broken = [").unwrap();
        assert!(Config::load(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken = [");
    }

    #[test]
    fn saves_replace_atomically_and_clean_up_staging() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.save(&path).unwrap();
        config.set("theme", "Forest".into());
        config.save(&path).unwrap();
        assert_eq!(
            Config::load(&path).unwrap().get("theme").unwrap().as_str(),
            Some("Forest")
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(config.save(&dir.path().join("absent/config.toml")).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn first_run_imports_legacy_ssh4_config_without_touching_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        let legacy = dir.path().join(LEGACY_CONFIG_FILE);
        let source = "theme = 'Forest'\n[profiles.home]\nhost = 'example.com'\nuser = 'keith'\n";
        fs::write(&legacy, source).unwrap();
        let config = Config::load(&path).unwrap();
        assert_eq!(config.get("theme").unwrap().as_str(), Some("Forest"));
        assert_eq!(config.profiles().unwrap()["home"].host, "example.com");
        assert!(!path.exists(), "import must not write until the first save");
        config.save(&path).unwrap();
        assert_eq!(fs::read_to_string(&legacy).unwrap(), source);
        // Once the NEO-SSH file exists it wins over the legacy file.
        fs::write(&legacy, "theme = 'Slate'\n").unwrap();
        assert_eq!(
            Config::load(&path).unwrap().get("theme").unwrap().as_str(),
            Some("Forest")
        );
    }

    #[test]
    fn legacy_tmux_and_reconnect_settings_are_translated_on_import() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(LEGACY_CONFIG_FILE),
            "reconnect_countdown_secs = 8
             [profiles.auto]
host = 'a'
auto_tmux = true
             [profiles.named]
host = 'b'
auto_tmux = true
tmux_session = 'work'
reconnect_delay_secs = 3
             [profiles.plain]
host = 'c'
",
        )
        .unwrap();
        let profiles = Config::load(&dir.path().join(CONFIG_FILE))
            .unwrap()
            .profiles()
            .unwrap();
        assert_eq!(profiles["auto"].tmux_session, "ssh4");
        assert_eq!(profiles["named"].tmux_session, "work");
        assert_eq!(profiles["plain"].tmux_session, "");
        let delay = |name: &str| profiles[name].extensions["reconnect_delay_secs"].as_integer();
        assert_eq!(delay("auto"), Some(8));
        assert_eq!(delay("named"), Some(3));
        assert_eq!(delay("plain"), Some(8));
    }

    #[test]
    fn legacy_ssh4gui_host_specs_and_port_zero_become_real_ports() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".ssh")).unwrap();
        fs::write(
            dir.path().join(".ssh/config"),
            "Host aliased\n  HostName 10.0.0.5\n  Port 2201\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(LEGACY_CONFIG_FILE),
            "[profiles.default]
host = 'plain.example'
port = 0
[profiles.spec]
host = 'alice@spec.example:2200'
port = 0
[profiles.override]
host = 'bob@over.example:2200'
user = 'carol'
port = 2022
[profiles.aliased]
host = 'aliased'
port = 0
[profiles.v6]
host = '[::1]:2300'
port = 0
",
        )
        .unwrap();
        let profiles = Config::load(&dir.path().join(CONFIG_FILE))
            .unwrap()
            .profiles()
            .unwrap();
        let summary = |name: &str| {
            let profile = &profiles[name];
            (profile.user.as_str(), profile.host.as_str(), profile.port)
        };
        assert_eq!(summary("default"), ("", "plain.example", 22));
        assert_eq!(summary("spec"), ("alice", "spec.example", 2200));
        assert_eq!(summary("override"), ("carol", "over.example", 2022));
        assert_eq!(summary("aliased"), ("", "aliased", 2201));
        assert_eq!(summary("v6"), ("", "::1", 2300));
        // Every migrated profile can be saved again.
        let mut config = Config::default();
        for (name, profile) in &profiles {
            config.set_profile(name, profile).unwrap();
        }
    }

    #[test]
    fn legacy_import_is_limited_to_the_default_name_and_tolerates_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LEGACY_CONFIG_FILE), "broken = [").unwrap();
        let config = Config::load(&dir.path().join(CONFIG_FILE)).unwrap();
        assert!(config.profiles().unwrap().is_empty());
        fs::write(dir.path().join(LEGACY_CONFIG_FILE), "theme = 'Slate'\n").unwrap();
        let explicit = Config::load(&dir.path().join("custom.toml")).unwrap();
        assert_eq!(explicit.get("theme").unwrap().as_str(), Some("Midnight"));
    }
}
