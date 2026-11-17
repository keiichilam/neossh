//! Remote POSIX working-directory reports and literal startup input.
pub fn validate(path: &str) -> Result<(), String> {
    if !path.starts_with('/') || path.len() > 4096 || path.chars().any(|c| c.is_control()) {
        return Err("Remote directory must be an absolute POSIX path of at most 4096 bytes without control characters.".into());
    }
    Ok(())
}

pub fn from_file_uri(uri: &str) -> Option<String> {
    if uri.len() > 16 * 1024 || uri.contains(['?', '#', '\\']) {
        return None;
    }
    let rest = uri.strip_prefix("file://")?;
    let slash = rest.find('/')?;
    let authority = &rest[..slash];
    if authority.contains(['@', '%']) || authority.chars().any(char::is_whitespace) {
        return None;
    }
    let parsed = url::Url::parse(uri).ok()?;
    if parsed.scheme() != "file" || parsed.port().is_some() {
        return None;
    }
    // Decode the original path, preserving literal dot segments and trailing spaces.
    let raw = &rest.as_bytes()[slash..];
    let mut decoded = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        if raw[at] == b'%' {
            let high = char::from(*raw.get(at + 1)?).to_digit(16)?;
            let low = char::from(*raw.get(at + 2)?).to_digit(16)?;
            decoded.push((high * 16 + low) as u8);
            at += 3;
        } else {
            decoded.push(raw[at]);
            at += 1;
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    validate(&path).ok()?;
    Some(path)
}

pub fn startup(directory: Option<&str>, tmux: Option<&str>) -> Result<Vec<u8>, String> {
    let mut command = String::new();
    if let Some(path) = directory {
        validate(path)?;
        command.push_str("cd -- '");
        command.push_str(&path.replace('\'', "'\\''"));
        command.push('\'');
        if tmux.is_some() {
            command.push_str(" && ");
        } else {
            command.push('\r');
        }
    }
    let mut bytes = command.into_bytes();
    if let Some(name) = tmux {
        bytes.extend(crate::target::tmux_command(name)?);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_reports_decode_without_normalizing_paths_or_accepting_controls() {
        assert_eq!(
            from_file_uri("file://remote/home/a%20b/%E7%95%8C/../"),
            Some("/home/a b/界/../".into())
        );
        assert_eq!(
            from_file_uri("file:///tmp/a%23b%25"),
            Some("/tmp/a#b%".into())
        );
        for value in [
            "https://host/tmp",
            "file://host/tmp%0aevil",
            "file://host/tmp%00",
            "file://host/tmp%ff",
            "file://host/tmp%2",
            "file://host/tmp?q",
            "file://user@host/tmp",
            "file://host:22/tmp",
        ] {
            assert!(from_file_uri(value).is_none(), "{value}");
        }
        assert!(from_file_uri(&format!("file:///{}", "a".repeat(4096))).is_none());
    }
    #[test]
    fn startup_quotes_paths_and_never_sends_cd_into_an_attached_tmux_pane() {
        assert_eq!(
            startup(Some("/tmp/a'b;$(id) `x`"), None).unwrap(),
            b"cd -- '/tmp/a'\\''b;$(id) `x`'\r"
        );
        assert_eq!(
            startup(Some("/work"), Some("daily")).unwrap(),
            b"cd -- '/work' && tmux new-session -A -s daily\r"
        );
        assert_eq!(
            startup(None, Some("daily")).unwrap(),
            crate::target::tmux_command("daily").unwrap()
        );
        assert!(startup(None, None).unwrap().is_empty());
        assert!(startup(Some("/tmp\rwhoami"), None).is_err());
        assert!(startup(Some("relative"), None).is_err());
    }
}
