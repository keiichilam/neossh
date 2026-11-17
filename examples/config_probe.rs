//! Read-only config evaluator used by tests/ssh_config_fixture.py.
use neo_ssh::ssh_config::{expand, SshConfig};
use std::path::Path;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("Usage: config_probe CONFIG ALIAS EXPLICIT_USER_OR_EMPTY LOCAL_USER".into());
    }
    let config = SshConfig::load(Path::new(&args[0]))?;
    let explicit_user = (!args[2].is_empty()).then_some(args[2].as_str());
    let resolved = config.resolve_with_context(&args[1], explicit_user, &args[3]);
    let forwards = resolved.all_values("localforward").join("\n");
    let mut values = resolved.values;
    values.insert("probe_localforward".into(), forwards);
    if let Some(user) = explicit_user {
        values.insert("user".into(), user.into());
    }
    values
        .entry("user".into())
        .or_insert_with(|| args[3].clone());
    let host = values.get("hostname").unwrap_or(&args[1]);
    let host = expand(host, &args[1], &args[1], &args[3], 22, Path::new(""))?;
    values.insert("hostname".into(), host);
    print!(
        "{}",
        toml::to_string(&values).map_err(|error| error.to_string())?
    );
    Ok(())
}
