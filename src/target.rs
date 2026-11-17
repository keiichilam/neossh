use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub user: Option<String>,
    pub host: String,
    pub port: u16,
}

impl std::str::FromStr for Target {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.is_empty()
            || input.chars().any(char::is_whitespace)
            || input.chars().any(char::is_control)
        {
            return Err("Enter a host without whitespace or control characters.".into());
        }
        let (user, address) = match input.split_once('@') {
            Some((user, address)) if !user.is_empty() && !address.contains('@') => {
                (Some(user.to_owned()), address)
            }
            Some(_) => return Err("Use user@host with one nonempty username.".into()),
            None => (None, input),
        };
        let (host, port) = if let Some(rest) = address.strip_prefix('[') {
            let (host, suffix) = rest
                .split_once(']')
                .ok_or("Missing closing IPv6 bracket.")?;
            host.parse::<std::net::Ipv6Addr>()
                .map_err(|_| "Invalid bracketed IPv6 address.")?;
            let port = if suffix.is_empty() {
                22
            } else {
                parse_port(
                    suffix
                        .strip_prefix(':')
                        .ok_or("Unexpected text after IPv6 address.")?,
                )?
            };
            (host, port)
        } else if address.matches(':').count() > 1 {
            address
                .parse::<std::net::Ipv6Addr>()
                .map_err(|_| "Invalid IPv6 address; use [address]:port for a port.")?;
            (address, 22)
        } else if let Some((host, port)) = address.split_once(':') {
            (host, parse_port(port)?)
        } else {
            (address, 22)
        };
        if host.is_empty() || host.contains(['[', ']', '/', '\\']) || host.starts_with('-') {
            return Err("Enter a valid nonempty host name or IP address.".into());
        }
        Ok(Self {
            user,
            host: host.into(),
            port,
        })
    }
}

fn parse_port(value: &str) -> Result<u16, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Port must be a number from 1 to 65535.".into());
    }
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| "Port must be a number from 1 to 65535.".into())
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(user) = &self.user {
            write!(f, "{user}@")?;
        }
        if self.host.contains(':') {
            write!(f, "[{}]", self.host)?;
        } else {
            write!(f, "{}", self.host)?;
        }
        if self.port != 22 {
            write!(f, ":{}", self.port)?;
        }
        Ok(())
    }
}

pub fn tmux_command(name: &str) -> Result<Vec<u8>, String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(
            "Session name must contain 1–64 letters, digits, hyphens, or underscores.".into(),
        );
    }
    Ok(format!("tmux new-session -A -s {name}\r").into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_forms_round_trip() {
        for (input, expected) in [
            ("server", "server"),
            ("ops@server:2222", "ops@server:2222"),
            ("[::1]:2200", "[::1]:2200"),
            ("ops@::1", "ops@[::1]"),
            ("host:22", "host"),
        ] {
            let target: Target = input.parse().unwrap();
            assert_eq!(target.to_string(), expected);
            assert_eq!(target.to_string().parse::<Target>().unwrap(), target);
        }
    }

    #[test]
    fn rejects_ambiguous_or_invalid_targets() {
        for input in [
            "",
            "@host",
            "user@",
            "a@b@c",
            "host:",
            "host:0",
            "host:65536",
            "host:+22",
            "host:abc",
            "host name",
            "host\x1b",
            "[::1",
            "[host]",
            "[::1]junk",
            "-host",
            "host/path",
        ] {
            assert!(input.parse::<Target>().is_err(), "{input:?}");
        }
    }

    #[test]
    fn tmux_input_cannot_inject_commands_or_target_delimiters() {
        for name in ["", "a;b", "a\nb", "a.b", "a:b", "$(id)", "é"] {
            assert!(tmux_command(name).is_err());
        }
        assert!(tmux_command(&"a".repeat(65)).is_err());
        assert_eq!(
            tmux_command("deploy_1").unwrap(),
            b"tmux new-session -A -s deploy_1\r"
        );
    }
}
