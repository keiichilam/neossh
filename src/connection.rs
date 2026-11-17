//! Connection resolution shared by GUI, CLI, and one-shot operations.
use crate::{
    config::Profile,
    session::Connect,
    ssh_config::{self, SshConfig},
    target::Target,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMethod {
    Agent,
    Key,
    Password,
    Interactive,
}

pub fn auth_methods(spec: &str) -> Result<Vec<AuthMethod>, String> {
    if spec.trim().is_empty() {
        return Ok(vec![
            AuthMethod::Agent,
            AuthMethod::Key,
            AuthMethod::Interactive,
            AuthMethod::Password,
        ]);
    }
    let mut methods = Vec::new();
    for token in spec.split(',').map(str::trim) {
        let additions: &[AuthMethod] = match token {
            "agent" => &[AuthMethod::Agent],
            "key" => &[AuthMethod::Key],
            "publickey" => &[AuthMethod::Agent, AuthMethod::Key],
            "password" => &[AuthMethod::Password],
            "keyboard-interactive" | "interactive" => &[AuthMethod::Interactive],
            _ => return Err(format!("Unsupported authentication method: {token}")),
        };
        for method in additions {
            if !methods.contains(method) {
                methods.push(*method);
            }
        }
    }
    Ok(methods)
}

#[derive(Default)]
pub struct Overrides {
    pub target: Option<String>,
    pub user: Option<String>,
    pub key: Option<PathBuf>,
    pub tmux: Option<String>,
    pub no_tmux: bool,
    pub jump: Option<String>,
}

pub fn resolve(
    profile: Option<&Profile>,
    overrides: Overrides,
    imports: &SshConfig,
    home: &Path,
    local_user: &str,
    global_keepalive: bool,
) -> Result<Connect, String> {
    resolve_route(
        profile,
        overrides,
        imports,
        home,
        local_user,
        global_keepalive,
        0,
    )
}

fn resolve_route(
    profile: Option<&Profile>,
    overrides: Overrides,
    imports: &SshConfig,
    home: &Path,
    local_user: &str,
    global_keepalive: bool,
    depth: usize,
) -> Result<Connect, String> {
    if depth > 8 {
        return Err("ProxyJump route is cyclic or exceeds eight hops.".into());
    }
    let (mut connect, jump) = resolve_single(
        profile,
        overrides,
        imports,
        home,
        local_user,
        global_keepalive,
    )?;
    if !jump.trim().is_empty() && !jump.trim().eq_ignore_ascii_case("none") {
        for name in jump.split(',').map(str::trim) {
            if name.is_empty() {
                return Err("ProxyJump contains an empty host.".into());
            }
            let mut hop = resolve_route(
                None,
                Overrides {
                    target: Some(name.into()),
                    ..Default::default()
                },
                imports,
                home,
                local_user,
                global_keepalive,
                depth + 1,
            )?;
            connect.jumps.append(&mut hop.jumps);
            connect.jumps.push(hop);
            if connect.jumps.len() > 8 {
                return Err("ProxyJump supports at most eight hops.".into());
            }
        }
    }
    if profile.is_some_and(|p| p.strict_host_key) {
        require_known_host_keys(&mut connect);
    }
    let mut peers = std::collections::HashSet::new();
    for step in connect.jumps.iter().chain(std::iter::once(&connect)) {
        if !peers.insert((step.target.host.to_ascii_lowercase(), step.target.port)) {
            return Err("ProxyJump route repeats a host and port.".into());
        }
    }
    Ok(connect)
}

fn resolve_single(
    profile: Option<&Profile>,
    overrides: Overrides,
    imports: &SshConfig,
    home: &Path,
    local_user: &str,
    global_keepalive: bool,
) -> Result<(Connect, String), String> {
    let raw = overrides
        .target
        .as_deref()
        .or_else(|| profile.map(|p| p.host.as_str()))
        .ok_or("A host is required.")?;
    let mut target: Target = raw.parse()?;
    let alias = target.host.clone();
    let explicit_user = overrides
        .user
        .as_deref()
        .filter(|user| !user.is_empty())
        .or(target.user.as_deref())
        .or_else(|| {
            profile
                .filter(|profile| !profile.user.is_empty())
                .map(|profile| profile.user.as_str())
        });
    let resolved_imports = imports.resolve_with_context(&alias, explicit_user, local_user);
    let imported = &resolved_imports.values;
    let get = |key: &str| imported.get(key).filter(|value| !value.is_empty());
    if get("proxycommand").is_some_and(|value| !value.eq_ignore_ascii_case("none")) {
        return Err("ProxyCommand is not supported. Configure a ProxyJump route instead.".into());
    }
    if let Some(host) = get("hostname") {
        target.host = ssh_config::expand(host, &alias, &alias, local_user, target.port, home)?;
    }
    let address = raw.rsplit('@').next().unwrap_or(raw);
    let explicit_port = address.starts_with('[') && address.contains("]:")
        || !address.starts_with('[') && address.matches(':').count() == 1;
    if !explicit_port {
        target.port = if let Some(profile) = profile {
            profile.port
        } else if let Some(port) = get("port") {
            port.parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or("SSH config Port must be from 1 to 65535.")?
        } else {
            22
        };
    }
    target.user = overrides
        .user
        .filter(|v| !v.is_empty())
        .or(target.user)
        .or_else(|| {
            profile
                .filter(|p| !p.user.is_empty())
                .map(|p| p.user.clone())
        })
        .or_else(|| get("user").cloned())
        .or_else(|| (!local_user.is_empty()).then(|| local_user.to_owned()));
    let user = target.user.as_deref().ok_or("A username is required.")?;
    // Validate the resolved destination independently of the alias.
    let _: Target = target.to_string().parse()?;
    let key = overrides
        .key
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| {
            profile
                .filter(|p| !p.key_path.is_empty())
                .map(|p| p.key_path.clone())
        })
        .or_else(|| get("identityfile").cloned());
    let key_path = key
        .map(|value| {
            ssh_config::expand(&value, &alias, &target.host, user, target.port, home)
                .map(PathBuf::from)
        })
        .transpose()?
        .or_else(|| {
            ["id_ed25519", "id_rsa"]
                .iter()
                .map(|name| home.join(".ssh").join(name))
                .find(|p| p.is_file())
        });
    let jump = overrides
        .jump
        .or_else(|| {
            profile
                .filter(|p| !p.jump.is_empty())
                .map(|p| p.jump.clone())
                .or_else(|| get("proxyjump").cloned())
        })
        .unwrap_or_default();
    let numeric = |key: &str, fallback| -> Result<u64, String> {
        get(key)
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| format!("SSH config {key} must be a nonnegative integer."))
            })
            .unwrap_or(Ok(fallback))
    };
    let timeout = profile
        .filter(|p| p.connect_timeout_secs != 0)
        .map(|p| Ok(p.connect_timeout_secs))
        .unwrap_or_else(|| numeric("connecttimeout", 15))?;
    let keepalive = profile
        .filter(|p| p.keep_alive_secs != 0)
        .map(|p| Ok(p.keep_alive_secs))
        .unwrap_or_else(|| numeric("serveraliveinterval", if global_keepalive { 30 } else { 0 }))?;
    let preferred = profile
        .filter(|p| !p.preferred_auth.is_empty())
        .map(|p| p.preferred_auth.as_str())
        .or_else(|| get("preferredauthentications").map(String::as_str))
        .unwrap_or("");
    let auth = auth_methods(preferred)?;
    let strict = profile.map(|p| p.strict_host_key).unwrap_or_else(|| {
        get("stricthostkeychecking").is_some_and(|v| v.eq_ignore_ascii_case("yes"))
    });
    let accept_new = !strict
        && get("stricthostkeychecking").is_some_and(|v| v.eq_ignore_ascii_case("accept-new"));
    let tmux = if overrides.no_tmux {
        None
    } else {
        overrides.tmux.or_else(|| {
            profile
                .filter(|p| !p.tmux_session.is_empty())
                .map(|p| p.tmux_session.clone())
        })
    };
    if let Some(name) = &tmux {
        crate::target::tmux_command(name)?;
    }
    let env = environment(profile.map_or("", |p| &p.remote_env))?;
    Ok((Connect {
        jumps: Vec::new(),
        target,
        password: Zeroizing::new(String::new()),
        key_path,
        known_hosts: home.join(".ssh/known_hosts"),
        strict,
        accept_new,
        tmux,
        remote_directory: None,
        term: profile
            .filter(|p| !p.term.is_empty())
            .map_or_else(|| "xterm-256color".into(), |p| p.term.clone()),
        timeout: Duration::from_secs(if timeout == 0 { 15 } else { timeout }),
        keepalive: (keepalive != 0).then(|| Duration::from_secs(keepalive)),
        environment: env,
        auth,
        use_agent: profile.is_none_or(|p| p.use_agent),
        remote_forwards: profile.filter(|p| !p.remote_forwards.is_empty())
            .map(|p| p.remote_forwards.iter().map(|value| value.parse()).collect::<Result<Vec<_>, String>>())
            .unwrap_or_else(|| resolved_imports.all_values("remoteforward").iter().map(|value| {
                let fields: Vec<_> = value.split_whitespace().collect();
                match fields.as_slice() {
                    [bind, destination] => format!("{bind}:{destination}").parse(),
                    _ => Err("Imported RemoteForward must specify a bind port and host:port destination.".into()),
                }
            }).collect())?,
        dynamic_forwards: profile.filter(|p| !p.dynamic_forwards.is_empty()).map(|p| p.dynamic_forwards.clone()).unwrap_or_else(|| resolved_imports.all_values("dynamicforward")).iter().map(|value| value.parse()).collect::<Result<Vec<_>, _>>()?,
        local_forwards: profile
            .filter(|p| !p.local_forwards.is_empty())
            .map(|p| p.local_forwards.iter().map(|value| value.parse::<crate::forward::LocalForward>()).collect::<Result<Vec<_>, String>>())
            .unwrap_or_else(|| resolved_imports.all_values("localforward").iter().map(|value| {
                let fields: Vec<_> = value.split_whitespace().collect();
                match fields.as_slice() {
                    [bind, destination] => format!("{bind}:{destination}").parse(),
                    _ => Err("Imported LocalForward must specify a bind port and host:port destination.".into()),
                }
            }).collect())?,
    }, jump))
}

/// Requires known host keys on the whole route: the destination and every
/// ProxyJump hop reject unknown keys instead of prompting or accepting new
/// ones, so "Require a known host key" also covers the hosts it passes through.
pub fn require_known_host_keys(connect: &mut Connect) {
    connect.strict = true;
    connect.accept_new = false;
    for hop in &mut connect.jumps {
        require_known_host_keys(hop);
    }
}

/// Gives a profile's forward lists the precedence `resolve` gives a saved
/// profile: each nonempty list replaces the resolved list of its kind, and an
/// empty one keeps the imported forwards. For forms resolved without a saved
/// profile, so their listeners match what saving the form would produce.
pub fn apply_forwards(connect: &mut Connect, profile: &Profile) -> Result<(), String> {
    fn parse<T: std::str::FromStr<Err = String>>(specs: &[String]) -> Result<Vec<T>, String> {
        specs.iter().map(|spec| spec.parse()).collect()
    }
    if !profile.local_forwards.is_empty() {
        connect.local_forwards = parse(&profile.local_forwards)?;
    }
    if !profile.remote_forwards.is_empty() {
        connect.remote_forwards = parse(&profile.remote_forwards)?;
    }
    if !profile.dynamic_forwards.is_empty() {
        connect.dynamic_forwards = parse(&profile.dynamic_forwards)?;
    }
    Ok(())
}

pub fn environment(spec: &str) -> Result<Vec<(String, String)>, String> {
    let mut values = std::collections::BTreeMap::new();
    for pair in spec
        .split([';', '\n'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let (name, value) = pair
            .split_once('=')
            .ok_or("Environment entries must use NAME=value.")?;
        let name = name.trim();
        if name.is_empty()
            || !name
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
            || value.contains('\0')
        {
            return Err("Invalid remote environment name or value.".into());
        }
        values.insert(name.to_owned(), value.to_owned());
    }
    Ok(values.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jump_routes_resolve_hop_identity_and_reject_cycles_without_direct_fallback() {
        let imports = SshConfig::parse("Host destination\n HostName private.example\n User final-user\n ProxyJump second\nHost second\n HostName inner.example\n User inner-user\n Port 2222\n IdentityFile ~/.ssh/inner\n ProxyJump first\nHost first\n HostName edge.example\n User edge-user\n").unwrap();
        let connect = resolve(
            None,
            Overrides {
                target: Some("destination".into()),
                ..Default::default()
            },
            &imports,
            Path::new("/home/test"),
            "local",
            false,
        )
        .unwrap();
        assert_eq!(connect.target.user.as_deref(), Some("final-user"));
        assert_eq!(connect.jumps.len(), 2);
        assert_eq!(
            connect.jumps[0].target.to_string(),
            "edge-user@edge.example"
        );
        assert_eq!(
            connect.jumps[1].target.to_string(),
            "inner-user@inner.example:2222"
        );
        assert!(connect.jumps[1]
            .key_path
            .as_ref()
            .unwrap()
            .ends_with(".ssh/inner"));
        assert!(connect
            .jumps
            .iter()
            .all(|hop| hop.password.is_empty() && hop.jumps.is_empty()));
        let direct = resolve(
            None,
            Overrides {
                target: Some("destination".into()),
                jump: Some("none".into()),
                ..Default::default()
            },
            &imports,
            Path::new("/home/test"),
            "local",
            false,
        )
        .unwrap();
        assert!(direct.jumps.is_empty());
        for jump in ["destination", "first,,second", "first,first"] {
            assert!(resolve(
                None,
                Overrides {
                    target: Some("destination".into()),
                    jump: Some(jump.into()),
                    ..Default::default()
                },
                &imports,
                Path::new("/home/test"),
                "local",
                false
            )
            .is_err());
        }
    }
    #[test]
    fn match_uses_cli_and_profile_user_for_settings_and_repeated_forwards() {
        let imports = SshConfig::parse("Host dev\nUser imported\nMatch user operator originalhost dev\nServerAliveInterval 13\nLocalForward 8080 db:5432\nLocalForward 8081 cache:6379\nMatch all\nServerAliveInterval 30\n").unwrap();
        let profile = Profile {
            host: "dev".into(),
            user: "operator".into(),
            ..Default::default()
        };
        for (profile, user, count) in [
            (None, None, 0),
            (Some(&profile), None, 2),
            (Some(&profile), Some("other"), 0),
            (None, Some("operator"), 2),
        ] {
            let connect = resolve(
                profile,
                Overrides {
                    target: Some("dev".into()),
                    user: user.map(str::to_owned),
                    ..Default::default()
                },
                &imports,
                Path::new("."),
                "local",
                false,
            )
            .unwrap();
            assert_eq!(connect.local_forwards.len(), count);
        }
        let connect = resolve(
            None,
            Overrides {
                target: Some("operator@dev".into()),
                ..Default::default()
            },
            &imports,
            Path::new("."),
            "local",
            false,
        )
        .unwrap();
        assert_eq!(connect.local_forwards.len(), 2);
    }

    #[test]
    fn requiring_known_host_keys_covers_every_jump_hop() {
        let imports = SshConfig::parse(
            "Host edge\nStrictHostKeyChecking accept-new\nHost inner\nProxyJump edge\n",
        )
        .unwrap();
        let resolve_with = |profile: Option<&Profile>| {
            resolve(
                profile,
                Overrides {
                    target: Some("dest".into()),
                    jump: Some("inner".into()),
                    ..Default::default()
                },
                &imports,
                Path::new("/home/test"),
                "local",
                false,
            )
            .unwrap()
        };
        // ssh_config alone keeps per-host policy.
        let open = resolve_with(None);
        assert_eq!(open.jumps.len(), 2);
        assert!(!open.strict && open.jumps.iter().all(|hop| !hop.strict));
        assert!(open.jumps[0].accept_new);
        // A profile that requires known keys requires them on the whole route.
        let profile = Profile {
            strict_host_key: true,
            ..Default::default()
        };
        let strict = resolve_with(Some(&profile));
        assert!(strict.strict);
        assert!(strict.jumps.iter().all(|hop| hop.strict && !hop.accept_new));
        // Forms set the same policy after resolving.
        let mut form = resolve_with(None);
        require_known_host_keys(&mut form);
        assert!(form.strict && !form.accept_new);
        assert!(form.jumps.iter().all(|hop| hop.strict && !hop.accept_new));
    }

    #[test]
    fn repeated_imported_local_forwards_are_preserved() {
        let imports = SshConfig::parse(
            "Host dev\nLocalForward 8080 db:5432\nLocalForward [::1]:8081 [::1]:443\n",
        )
        .unwrap();
        let connect = resolve(
            None,
            Overrides {
                target: Some("dev".into()),
                ..Default::default()
            },
            &imports,
            Path::new("."),
            "user",
            false,
        )
        .unwrap();
        assert_eq!(connect.local_forwards.len(), 2);
        assert_eq!(connect.local_forwards[0].host, "db");
        assert_eq!(connect.local_forwards[1].bind, "::1");
    }
    #[test]
    fn cli_overrides_profile_which_overrides_imported_values() {
        let imports = SshConfig::parse(
            "Host prod\nHostName server\nUser imported\nPort 2200\nIdentityFile ~/.ssh/%n\n",
        )
        .unwrap();
        let profile = Profile {
            host: "prod".into(),
            user: "saved".into(),
            port: 2222,
            ..Default::default()
        };
        let connect = resolve(
            Some(&profile),
            Overrides {
                target: Some("override@prod:2223".into()),
                ..Default::default()
            },
            &imports,
            Path::new("/home/test"),
            "local",
            false,
        )
        .unwrap();
        assert_eq!(connect.target.to_string(), "override@server:2223");
        assert!(connect.key_path.unwrap().ends_with(".ssh/prod"));
        let connect = resolve(
            None,
            Overrides {
                target: Some("prod".into()),
                ..Default::default()
            },
            &imports,
            Path::new("/home/test"),
            "local",
            false,
        )
        .unwrap();
        assert_eq!(connect.target.to_string(), "imported@server:2200");
        assert_eq!(connect.keepalive, None);
    }
    #[test]
    fn profile_options_and_auth_restrictions_survive_resolution() {
        let profile = Profile {
            host: "server".into(),
            term: "vt100".into(),
            remote_env: "LANG=C.UTF-8; X=a=b".into(),
            preferred_auth: "password,key".into(),
            keep_alive_secs: 8,
            connect_timeout_secs: 4,
            strict_host_key: true,
            use_agent: false,
            ..Default::default()
        };
        let connect = resolve(
            Some(&profile),
            Overrides::default(),
            &SshConfig::default(),
            Path::new("/tmp"),
            "user",
            true,
        )
        .unwrap();
        assert_eq!(
            connect.environment,
            [
                ("LANG".into(), "C.UTF-8".into()),
                ("X".into(), "a=b".into())
            ]
        );
        assert_eq!(connect.auth, [AuthMethod::Password, AuthMethod::Key]);
        assert_eq!(connect.term, "vt100");
        assert_eq!(connect.timeout, Duration::from_secs(4));
        assert!(!connect.use_agent);
        assert!(connect.strict);
    }
    #[test]
    fn remote_forwards_import_all_listeners_and_profile_takes_precedence() {
        let imports = SshConfig::parse(
            "Host dev\nRemoteForward 8000 localhost:80\nRemoteForward [::1]:8001 [::1]:443\n",
        )
        .unwrap();
        let imported = resolve(
            None,
            Overrides {
                target: Some("dev".into()),
                ..Default::default()
            },
            &imports,
            Path::new("/tmp"),
            "user",
            true,
        )
        .unwrap();
        assert_eq!(imported.remote_forwards.len(), 2);
        assert_eq!(imported.remote_forwards[1].bind, "::1");
        assert_eq!(imported.remote_forwards[1].destination_port, 443);
        let profile = Profile {
            host: "dev".into(),
            remote_forwards: vec!["0:localhost:9000".into()],
            ..Default::default()
        };
        let configured = resolve(
            Some(&profile),
            Overrides::default(),
            &imports,
            Path::new("/tmp"),
            "user",
            true,
        )
        .unwrap();
        assert_eq!(configured.remote_forwards.len(), 1);
        assert_eq!(configured.remote_forwards[0].destination_port, 9000);
        assert_eq!(configured.remote_forwards[0].port, 0);
    }

    #[test]
    fn rejects_unsafe_or_unimplemented_routing_without_direct_fallback() {
        for directive in ["ProxyJump h", "ProxyCommand ssh bastion", "Port 0"] {
            let imports = SshConfig::parse(&format!("Host h\n{directive}")).unwrap();
            assert!(resolve(
                None,
                Overrides {
                    target: Some("h".into()),
                    ..Default::default()
                },
                &imports,
                Path::new("/tmp"),
                "u",
                true
            )
            .is_err());
        }
        assert!(environment("1BAD=x").is_err());
        assert!(environment("X=x\0").is_err());
        assert!(auth_methods("password,unknown").is_err());
    }
}
