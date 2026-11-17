mod clipboard_image;
mod command_line;
mod desktop;
mod file_browser;
mod key_install_dialog;
mod keygen_dialog;
mod local_browser;
mod settings_dialog;
use clap::{ArgGroup, Parser};
use neo_ssh::config::Config;
use neo_ssh::preferences::{MAX_SCROLLBACK_LINES, MIN_SCROLLBACK_LINES};

// Compatibility aliases keep ssh4 (-k/--key, -u, --tmux-session) and ssh4gui
// (--identity-file, --local, --remote, --dynamic) scripts working. Secrets are
// never accepted as arguments, so ssh4's --password has no equivalent.
#[derive(Parser)]
#[command(name = "neo-ssh", version, about = "A native SSH workspace")]
#[command(group(ArgGroup::new("destination").args(["target", "profile"]).multiple(true)))]
struct Args {
    /// Generate an Ed25519 key and .pub file; add --protect-key for passphrase protection.
    #[arg(long, conflicts_with_all = ["target", "profile", "gui"])]
    generate_key: Option<std::path::PathBuf>,
    /// Prompt for a passphrase to protect a generated key (never accepted on the command line).
    #[arg(long, requires = "generate_key")]
    protect_key: bool,
    /// SSH target; omit to open the desktop workspace.
    target: Option<String>,
    /// Configuration path (default: ~/.neo-ssh.toml; imports ~/.ssh4.toml on first run).
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    /// Open the GUI even when a target is supplied.
    #[arg(long)]
    gui: bool,
    #[arg(short = 'P', long)]
    /// Use a saved connection profile.
    profile: Option<String>,
    #[arg(short = 'l', long, visible_short_alias = 'u')]
    /// Override the remote username.
    user: Option<String>,
    #[arg(short = 'i', long, visible_short_alias = 'k', visible_aliases = ["key", "identity-file"])]
    /// Authenticate with this private key.
    identity: Option<std::path::PathBuf>,
    /// Forward a local port: [bind-address:]port:host:port.
    #[arg(short = 'L', long = "local-forward", visible_alias = "local")]
    local_forwards: Vec<String>,
    /// Open a SOCKS5 proxy: [bind-address:]port.
    #[arg(short = 'D', long = "dynamic-forward", visible_alias = "dynamic")]
    dynamic_forwards: Vec<String>,
    /// Forward a server port to a local service: [bind-address:]port:host:port.
    #[arg(short = 'R', long = "remote-forward", visible_alias = "remote")]
    remote_forwards: Vec<String>,
    #[arg(long)]
    /// Append raw remote output to a file.
    log: Option<std::path::PathBuf>,
    #[arg(long)]
    /// Exit after disconnect instead of offering to reconnect.
    no_reconnect: bool,
    #[arg(long, num_args = 0..=1, default_missing_value = "neo-ssh", require_equals = true, conflicts_with = "no_tmux")]
    /// Attach/create a named tmux session (default name: neo-ssh); a name must use --tmux=NAME.
    tmux: Option<String>,
    #[arg(long, value_name = "NAME", conflicts_with_all = ["tmux", "no_tmux"])]
    /// Attach/create tmux session NAME; same as --tmux=NAME (ssh4 compatibility).
    tmux_session: Option<String>,
    #[arg(long, conflicts_with = "tmux")]
    /// Disable tmux inherited from the profile.
    no_tmux: bool,
    /// Connect through comma-separated SSH jump hosts or imported aliases; 'none' disables jumps.
    #[arg(short = 'J', long = "jump")]
    jump: Option<String>,
    /// Upload the clipboard image as DIR/clip_YYYYmmdd_HHMMSS.png (UTC), print the remote
    /// path, and exit. DIR is --remote-dir, the profile's remote_dir, or /tmp. Existing
    /// remote files are never replaced; tmux and port forwards are not started.
    #[arg(
        long,
        requires = "destination",
        conflicts_with_all = ["gui", "log", "tmux", "tmux_session", "local_forwards", "remote_forwards", "dynamic_forwards"]
    )]
    once: bool,
    /// Remote folder (absolute POSIX path) for --once and the default for Ctrl+P image
    /// uploads; overrides the profile's remote_dir and is stored by --save-profile.
    #[arg(
        long,
        value_name = "DIR",
        value_parser = parse_remote_dir,
        requires = "destination",
        conflicts_with = "gui"
    )]
    remote_dir: Option<String>,
    /// Save the resolved target and options (never passwords) as profile NAME, then
    /// continue. An existing profile is only replaced when it was loaded with -P NAME.
    #[arg(
        long,
        value_name = "NAME",
        value_parser = parse_profile_name,
        requires = "destination",
        conflicts_with = "gui"
    )]
    save_profile: Option<String>,
    /// Connect timeout in seconds (1-300); overrides the profile's connect timeout and the
    /// ssh_config ConnectTimeout of the destination.
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..=300))]
    timeout: Option<u64>,
    /// Terminal history length in lines, clamped to 100-50000; overrides the profile and
    /// global setting. Scrollback stays off when disabled in Preferences.
    #[arg(long, value_name = "LINES", value_parser = parse_scrollback_lines)]
    scrollback_lines: Option<u32>,
}

impl Args {
    /// The tmux session requested by --tmux[=NAME] or its --tmux-session alias.
    fn tmux_name(&self) -> Option<String> {
        self.tmux.clone().or_else(|| self.tmux_session.clone())
    }
}

fn parse_remote_dir(value: &str) -> Result<String, String> {
    neo_ssh::remote_directory::validate(value)?;
    Ok(value.to_owned())
}

fn parse_profile_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("Profile name is required.".into());
    }
    Ok(name.to_owned())
}

fn parse_scrollback_lines(value: &str) -> Result<u32, String> {
    let lines = value.trim().parse::<u64>().map_err(|_| {
        format!("Expected a whole number of lines ({MIN_SCROLLBACK_LINES}-{MAX_SCROLLBACK_LINES}).")
    })?;
    if lines == 0 {
        return Err(
            "0 is not allowed; turn terminal scrollback off in Preferences instead.".into(),
        );
    }
    Ok(lines.clamp(MIN_SCROLLBACK_LINES.into(), MAX_SCROLLBACK_LINES.into()) as u32)
}

fn main() {
    let args = Args::parse();
    let mut detached_console = false;
    let result = (|| {
        let path = args
            .config
            .clone()
            .map(Ok)
            .unwrap_or_else(Config::path)
            .map_err(|e| e.to_string())?;
        if let Err(error) = neo_ssh::diagnostics::install(&path) {
            eprintln!("neo-ssh: crash logging unavailable: {error}");
        }
        if let Some(path) = &args.generate_key {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(path)
            };
            let password = if args.protect_key {
                let password = command_line::secret_prompt("New key passphrase")?;
                let confirmation = command_line::secret_prompt("Confirm key passphrase")?;
                if password.is_empty() || password.as_str() != confirmation.as_str() {
                    return Err("Passphrases must be nonempty and match.".into());
                }
                Some(password)
            } else {
                None
            };
            let generated = neo_ssh::keygen::generate_with_passphrase(
                &path,
                password.as_ref().map(|password| password.as_str()),
            )?;
            println!(
                "Private key: {}\nPublic key: {}\n{}\n{}",
                generated.private_path.display(),
                generated.public_path.display(),
                generated.fingerprint,
                generated.public_key
            );
            if let Some(warning) = generated.warning {
                eprintln!("{warning}");
            }
            return Ok(());
        }
        let config = Config::load(&path)?;
        if !args.gui && (args.target.is_some() || args.profile.is_some()) {
            return command_line::run(args, config, &path);
        }
        let mut draft = desktop::Draft::default();
        if let Some(name) = &args.profile {
            let profiles = config.profiles()?;
            let profile = profiles
                .get(name)
                .ok_or_else(|| format!("Unknown profile: {name}"))?;
            draft.target = neo_ssh::target::Target {
                host: profile.host.clone(),
                port: profile.port,
                user: None,
            }
            .to_string();
            draft.user = profile.user.clone();
            draft.key_path = profile.key_path.clone();
            draft.name = name.clone();
            draft.original_name = name.clone();
            draft.tmux = profile.tmux_session.clone();
            draft.strict = profile.strict_host_key;
            draft.options = profile.clone();
            draft.saved_profile = true;
        }
        if let Some(target) = args.target {
            draft.target = target;
        }
        if let Some(user) = args.user {
            draft.user = user;
        }
        if let Some(path) = args.identity {
            draft.key_path = path.to_string_lossy().into_owned();
        }
        if args.no_tmux {
            draft.tmux.clear();
        } else if let Some(tmux) = args.tmux.or(args.tmux_session) {
            draft.tmux = tmux;
        }
        if let Some(seconds) = args.timeout {
            draft.options.connect_timeout_secs = seconds;
        }
        if let Some(lines) = args.scrollback_lines {
            draft.options.extensions.insert(
                "scrollback_lines".into(),
                toml::Value::Integer(lines.into()),
            );
        }
        draft.options.local_forwards.extend(args.local_forwards);
        if let Some(jump) = args.jump {
            draft.options.jump = jump;
        }
        draft.options.dynamic_forwards.extend(args.dynamic_forwards);
        draft.options.remote_forwards.extend(args.remote_forwards);
        detached_console = detach_owned_console();
        desktop::run(config, path, draft)
    })();
    if let Err(error) = result {
        if detached_console {
            rfd::MessageDialog::new()
                .set_title("neo-ssh could not start")
                .set_description(&error)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        eprintln!("neo-ssh: {error}");
        std::process::exit(1);
    }
}

// Double-click launch allocates a console belonging only to this process.
// A shell's shared console stays attached, preserving its output and CLI use.
fn detach_owned_console() -> bool {
    #[cfg(windows)]
    unsafe {
        let mut process = 0u32;
        if windows_sys::Win32::System::Console::GetConsoleProcessList(&mut process, 1) == 1
            && process == std::process::id()
        {
            return windows_sys::Win32::System::Console::FreeConsole() != 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tmux_flag_does_not_consume_target_and_conflicts_are_rejected() {
        assert!(Args::try_parse_from(["neo-ssh", "--protect-key"]).is_err());
        let protected =
            Args::try_parse_from(["neo-ssh", "--generate-key", "new-key", "--protect-key"])
                .unwrap();
        assert!(protected.protect_key);
        let args = Args::try_parse_from(["neo-ssh", "--tmux", "user@host"]).unwrap();
        assert_eq!(args.target.as_deref(), Some("user@host"));
        assert_eq!(args.tmux.as_deref(), Some("neo-ssh"));
        assert!(Args::try_parse_from(["neo-ssh", "--tmux=ops", "--no-tmux"]).is_err());
        let args = Args::try_parse_from(["neo-ssh", "--gui", "-P", "prod"]).unwrap();
        assert!(args.gui);
        assert_eq!(args.profile.as_deref(), Some("prod"));
        let args =
            Args::try_parse_from(["neo-ssh", "-J", "edge,ops@inner:2222", "destination"]).unwrap();
        assert_eq!(args.jump.as_deref(), Some("edge,ops@inner:2222"));
        assert_eq!(args.target.as_deref(), Some("destination"));
    }

    fn parse(arguments: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("neo-ssh").chain(arguments.iter().copied()))
    }

    #[test]
    fn ssh4_and_ssh4gui_aliases_map_to_existing_options() {
        let args = parse(&["-u", "ops", "-k", "id_a", "host"]).unwrap();
        assert_eq!(args.user.as_deref(), Some("ops"));
        assert_eq!(args.identity.as_deref(), Some(std::path::Path::new("id_a")));
        assert_eq!(args.target.as_deref(), Some("host"));
        for flag in ["--key", "--identity-file", "--identity", "-i"] {
            let args = parse(&[flag, "id_b", "host"]).unwrap();
            assert_eq!(args.identity.as_deref(), Some(std::path::Path::new("id_b")));
        }
        let args = parse(&["-l", "admin", "--user", "root", "host"]);
        assert!(
            args.is_err(),
            "user given twice must not be silently merged"
        );
        let args = parse(&[
            "--local",
            "8080:db:5432",
            "-L",
            "8081:db:5432",
            "--remote",
            "9000:localhost:80",
            "--dynamic",
            "1080",
            "host",
        ])
        .unwrap();
        assert_eq!(args.local_forwards, ["8080:db:5432", "8081:db:5432"]);
        assert_eq!(args.remote_forwards, ["9000:localhost:80"]);
        assert_eq!(args.dynamic_forwards, ["1080"]);
        let args = parse(&["--tmux-session", "work", "host"]).unwrap();
        assert_eq!(args.tmux_name().as_deref(), Some("work"));
        assert_eq!(args.target.as_deref(), Some("host"));
        assert_eq!(
            parse(&["--tmux=ops", "host"])
                .unwrap()
                .tmux_name()
                .as_deref(),
            Some("ops")
        );
        assert!(parse(&["--tmux-session", "work", "--no-tmux", "host"]).is_err());
        assert!(parse(&["--tmux-session", "work", "--tmux=ops", "host"]).is_err());
        // Secrets are never accepted as arguments.
        assert!(parse(&["--password", "secret", "host"]).is_err());
    }

    #[test]
    fn tmux_still_leaves_the_target_positional_with_new_options() {
        let args = parse(&["--tmux", "user@host", "--timeout", "5"]).unwrap();
        assert_eq!(args.target.as_deref(), Some("user@host"));
        assert_eq!(args.tmux_name().as_deref(), Some("neo-ssh"));
        assert_eq!(args.timeout, Some(5));
        let args = parse(&["--remote-dir", "/srv/drop", "--tmux", "-P", "prod"]).unwrap();
        assert_eq!(args.target, None);
        assert_eq!(args.profile.as_deref(), Some("prod"));
        assert_eq!(args.remote_dir.as_deref(), Some("/srv/drop"));
    }

    #[test]
    fn once_and_profile_saving_need_a_cli_destination() {
        for arguments in [
            &["--once"][..],
            &["--remote-dir", "/tmp"],
            &["--save-profile", "prod"],
            &["--once", "--gui", "host"],
            &["--save-profile", "prod", "--gui", "host"],
            &["--remote-dir", "/tmp", "--gui", "host"],
            &["--once", "-L", "8080:db:5432", "host"],
            &["--once", "--tmux", "host"],
            &["--once", "--tmux-session", "ops", "host"],
            &["--once", "--log", "out.log", "host"],
        ] {
            assert!(parse(arguments).is_err(), "{arguments:?}");
        }
        assert!(parse(&["--once", "host"]).unwrap().once);
        let args = parse(&["--once", "-P", "prod", "--remote-dir", "/data/"]).unwrap();
        assert!(args.once);
        assert_eq!(args.remote_dir.as_deref(), Some("/data/"));
        let args = parse(&["--save-profile", " prod ", "-P", "prod"]).unwrap();
        assert_eq!(args.save_profile.as_deref(), Some("prod"));
        assert!(parse(&["--save-profile", "  ", "host"]).is_err());
        // --timeout and --scrollback-lines also apply to the desktop workspace.
        assert!(parse(&["--gui", "--timeout", "9", "--scrollback-lines", "500"]).is_ok());
    }

    #[test]
    fn remote_dir_must_be_an_absolute_posix_path() {
        for valid in ["/", "/tmp", "/srv/a b/", "/home/user/界"] {
            let args = parse(&["--remote-dir", valid, "host"]).unwrap();
            assert_eq!(args.remote_dir.as_deref(), Some(valid));
        }
        let too_long = format!("/{}", "a".repeat(4096));
        for invalid in [
            "tmp",
            "~/uploads",
            "C:\\Temp",
            "/tmp\nx",
            "",
            too_long.as_str(),
        ] {
            assert!(
                parse(&["--remote-dir", invalid, "host"]).is_err(),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn scrollback_lines_are_clamped_and_zero_is_rejected() {
        let lines = |value: &str| {
            parse(&["--scrollback-lines", value, "host"]).map(|args| args.scrollback_lines.unwrap())
        };
        assert_eq!(lines("2500").unwrap(), 2500);
        assert_eq!(lines("1").unwrap(), MIN_SCROLLBACK_LINES);
        assert_eq!(lines("99").unwrap(), MIN_SCROLLBACK_LINES);
        assert_eq!(lines("100").unwrap(), MIN_SCROLLBACK_LINES);
        assert_eq!(lines("50000").unwrap(), MAX_SCROLLBACK_LINES);
        assert_eq!(lines("100000").unwrap(), MAX_SCROLLBACK_LINES);
        assert_eq!(lines("99999999999").unwrap(), MAX_SCROLLBACK_LINES);
        for invalid in ["0", "-5", "ten", "1.5", ""] {
            assert!(lines(invalid).is_err(), "{invalid:?}");
        }
        let zero = parse(&["--scrollback-lines", "0", "host"]).err().unwrap();
        assert!(zero.to_string().contains("Preferences"), "{zero}");
    }

    #[test]
    fn timeout_accepts_one_to_three_hundred_seconds() {
        assert_eq!(parse(&["--timeout", "1", "h"]).unwrap().timeout, Some(1));
        assert_eq!(
            parse(&["--timeout", "300", "h"]).unwrap().timeout,
            Some(300)
        );
        assert_eq!(parse(&["h"]).unwrap().timeout, None);
        for invalid in ["0", "301", "-1", "soon"] {
            assert!(parse(&["--timeout", invalid, "h"]).is_err(), "{invalid:?}");
        }
    }
}
