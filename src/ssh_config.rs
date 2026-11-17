//! Read-only OpenSSH configuration import. No directives execute local commands.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
};

#[derive(Clone, Default)]
pub struct SshConfig {
    requests_final: bool,
    entries: Vec<Entry>,
    warnings: Vec<String>,
}
#[derive(Clone)]
struct Entry {
    scope: Arc<Scope>,
    guards: Vec<Arc<Scope>>,
    include_paths: Vec<String>,
    key: String,
    value: String,
}

#[derive(Clone)]
enum Scope {
    Host(Vec<String>),
    Match(Vec<Criterion>),
    Unsupported,
}
#[derive(Clone)]
struct Criterion {
    key: String,
    patterns: Vec<String>,
    negated: bool,
}
impl Scope {
    fn host_patterns(&self) -> &[String] {
        match self {
            Self::Host(patterns) => patterns,
            _ => &[],
        }
    }
}

pub struct ResolvedConfig {
    pub values: BTreeMap<String, String>,
    repeated: BTreeMap<String, Vec<String>>,
}
impl ResolvedConfig {
    pub fn all_values(&self, key: &str) -> Vec<String> {
        self.repeated.get(key).cloned().unwrap_or_default()
    }
}

impl SshConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        Self::load_with_environment(path, &mut |name| std::env::var(name).ok())
    }

    fn load_with_environment(
        path: &Path,
        environment: &mut impl FnMut(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let mut budget = LoadBudget {
            requests_final: false,
            files: 0,
            bytes: 0,
            stack: Vec::new(),
            warnings: Vec::new(),
        };
        let entries = load_entries(path, base, &[], &mut budget, environment)?;
        Ok(Self {
            entries,
            warnings: budget.warnings,
            requests_final: budget.requests_final,
        })
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut entries = Vec::new();
        let mut scope = Arc::new(Scope::Host(vec!["*".to_owned()]));
        let mut warnings = Vec::new();
        let mut requests_final = false;
        for (line_number, line) in text.lines().enumerate() {
            let tokens = tokenize(line).map_err(|_| {
                format!("Invalid quoting in SSH config at line {}.", line_number + 1)
            })?;
            let Some(key) = tokens.first() else {
                continue;
            };
            let key = key.to_ascii_lowercase();
            let values = &tokens[1..];
            if key == "host" {
                scope = Arc::new(Scope::Host(values.to_vec()));
                continue;
            }
            if key == "match" {
                scope = Arc::new(parse_match(values, &mut warnings, &mut requests_final)?);
                continue;
            }
            if !values.is_empty() {
                entries.push(Entry {
                    scope: scope.clone(),
                    guards: Vec::new(),
                    include_paths: if key == "include" {
                        values.to_vec()
                    } else {
                        Vec::new()
                    },
                    key,
                    value: values.join(" "),
                });
            }
        }
        Ok(Self {
            entries,
            warnings,
            requests_final,
        })
    }

    pub fn aliases(&self) -> Vec<String> {
        let mut aliases = std::collections::BTreeSet::new();
        for entry in &self.entries {
            for pattern in entry
                .scope
                .host_patterns()
                .iter()
                .chain(entry.guards.iter().flat_map(|guard| guard.host_patterns()))
            {
                if !pattern.contains(['*', '?', '!'])
                    && scope_allows_alias(&entry.scope, pattern)
                    && entry
                        .guards
                        .iter()
                        .all(|guard| scope_allows_alias(guard, pattern))
                {
                    aliases.insert(pattern.clone());
                }
            }
        }
        aliases.into_iter().collect()
    }

    pub fn resolve(&self, alias: &str) -> BTreeMap<String, String> {
        self.resolve_with_context(alias, None, "").values
    }

    pub fn resolve_with_context(
        &self,
        alias: &str,
        explicit_user: Option<&str>,
        local_user: &str,
    ) -> ResolvedConfig {
        let mut result = ResolvedConfig {
            values: BTreeMap::new(),
            repeated: BTreeMap::new(),
        };
        let mut applied = HashSet::new();
        let mut final_host = None;
        for final_pass in [false, true] {
            if final_pass {
                if !self.requests_final {
                    break;
                }
                let host = result
                    .values
                    .entry("hostname".into())
                    .or_insert_with(|| alias.to_owned());
                final_host = Some(
                    expand(host, alias, alias, local_user, 22, Path::new(""))
                        .unwrap_or_else(|_| host.clone()),
                );
            }
            // Each pass freezes a block's condition at its entry point. Values
            // chosen on the initial pass retain precedence on the final pass.
            let mut matched = HashMap::new();
            for (index, entry) in self.entries.iter().enumerate() {
                let mut active = true;
                for scope in entry.guards.iter().chain(std::iter::once(&entry.scope)) {
                    let id = Arc::as_ptr(scope) as usize;
                    let applies = *matched.entry(id).or_insert_with(|| {
                        scope_matches(
                            scope,
                            alias,
                            explicit_user,
                            local_user,
                            &result.values,
                            final_host.as_deref(),
                        )
                    });
                    active &= applies;
                    if !active {
                        break;
                    }
                }
                if active && entry.key != "include" && applied.insert(index) {
                    result
                        .values
                        .entry(entry.key.clone())
                        .or_insert_with(|| entry.value.clone());
                    let repeated = result.repeated.entry(entry.key.clone()).or_default();
                    // OpenSSH also suppresses duplicate forwarding declarations
                    // in different matching blocks (including the final pass).
                    if !matches!(
                        entry.key.as_str(),
                        "localforward" | "remoteforward" | "dynamicforward"
                    ) || !repeated.contains(&entry.value)
                    {
                        repeated.push(entry.value.clone());
                    }
                }
            }
        }
        result
    }

    pub fn all_values(&self, alias: &str, key: &str) -> Vec<String> {
        self.resolve_with_context(alias, None, "").all_values(key)
    }
}

fn scope_allows_alias(scope: &Scope, alias: &str) -> bool {
    match scope {
        Scope::Host(patterns) => matches_patterns(patterns, alias),
        Scope::Match(_) => true,
        Scope::Unsupported => false,
    }
}

fn parse_match(
    values: &[String],
    warnings: &mut Vec<String>,
    requests_final: &mut bool,
) -> Result<Scope, String> {
    if values.is_empty() {
        return Err("SSH Match requires a condition.".into());
    }
    if values.len() == 1 && values[0].eq_ignore_ascii_case("all") {
        return Ok(Scope::Match(Vec::new()));
    }
    if values.len() == 1 && values[0].eq_ignore_ascii_case("!all") {
        return Ok(Scope::Unsupported);
    }
    let mut criteria = Vec::new();
    let mut index = 0;
    while index < values.len() {
        let raw = &values[index];
        let attribute = raw.strip_prefix('!').unwrap_or(raw);
        let (key, inline_argument) = attribute
            .split_once('=')
            .map_or((attribute, None), |(key, argument)| (key, Some(argument)));
        let key = key.to_ascii_lowercase();
        if key == "final" && inline_argument.is_none() {
            *requests_final |= !raw.starts_with('!');
            criteria.push(Criterion {
                key,
                patterns: Vec::new(),
                negated: raw.starts_with('!'),
            });
            index += 1;
            continue;
        }
        if key == "all" {
            if inline_argument.is_some()
                || index + 1 != values.len()
                || criteria.len() != 1
                || criteria[0].key != "final"
            {
                return Err("SSH Match all must appear alone or immediately after final.".into());
            }
            if raw.starts_with('!') {
                return Ok(Scope::Unsupported);
            }
            break;
        }
        if !matches!(key.as_str(), "host" | "originalhost" | "user" | "localuser") {
            if warnings.len() < 128 {
                warnings.push(format!(
                    "Skipped SSH Match block: condition {key:?} is not supported."
                ));
            }
            return Ok(Scope::Unsupported);
        }
        let argument = inline_argument
            .or_else(|| values.get(index + 1).map(String::as_str))
            .ok_or_else(|| format!("SSH Match {key} requires a pattern list."))?;
        let patterns: Vec<String> = argument.split(',').map(str::to_owned).collect();
        if patterns
            .iter()
            .any(|pattern| pattern.is_empty() || pattern == "!")
        {
            return Err(format!("Invalid pattern list in SSH Match {key}."));
        }
        criteria.push(Criterion {
            key,
            patterns,
            negated: raw.starts_with('!'),
        });
        index += if inline_argument.is_some() { 1 } else { 2 };
    }
    Ok(Scope::Match(criteria))
}

fn scope_matches(
    scope: &Scope,
    alias: &str,
    explicit_user: Option<&str>,
    local_user: &str,
    values: &BTreeMap<String, String>,
    final_host: Option<&str>,
) -> bool {
    match scope {
        Scope::Host(patterns) => matches_patterns(patterns, final_host.unwrap_or(alias)),
        Scope::Unsupported => false,
        Scope::Match(criteria) => {
            let host = values
                .get("hostname")
                .map(|host| {
                    expand(host, alias, alias, local_user, 22, Path::new(""))
                        .unwrap_or_else(|_| host.clone())
                })
                .unwrap_or_else(|| alias.to_owned());
            let user = explicit_user
                .or_else(|| values.get("user").map(String::as_str))
                .unwrap_or(local_user);
            criteria.iter().all(|criterion| {
                if criterion.key == "final" {
                    return final_host.is_some() != criterion.negated;
                }
                let (value, insensitive) = match criterion.key.as_str() {
                    "host" => (final_host.unwrap_or(&host), true),
                    "originalhost" => (alias, true),
                    "user" => (user, false),
                    "localuser" => (local_user, false),
                    _ => return false,
                };
                let matches = matches_patterns_case(&criterion.patterns, value, insensitive);
                matches != criterion.negated
            })
        }
    }
}

fn matches_patterns(patterns: &[String], alias: &str) -> bool {
    matches_patterns_case(patterns, alias, true)
}

fn matches_patterns_case(patterns: &[String], alias: &str, insensitive: bool) -> bool {
    patterns
        .iter()
        .any(|pattern| !pattern.starts_with('!') && wildcard(pattern, alias, insensitive))
        && !patterns.iter().any(|pattern| {
            pattern
                .strip_prefix('!')
                .is_some_and(|pattern| wildcard(pattern, alias, insensitive))
        })
}

struct LoadBudget {
    requests_final: bool,
    files: usize,
    bytes: usize,
    stack: Vec<std::path::PathBuf>,
    warnings: Vec<String>,
}

fn load_entries(
    path: &Path,
    base: &Path,
    guards: &[Arc<Scope>],
    budget: &mut LoadBudget,
    environment: &mut impl FnMut(&str) -> Option<String>,
) -> Result<Vec<Entry>, String> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "Cannot read SSH configuration {}: {error}",
                path.display()
            ))
        }
    };
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if budget.stack.contains(&canonical) {
        return Err("SSH configuration Include cycle detected.".into());
    }
    budget.files += 1;
    if budget.stack.len() >= 16 || budget.files > 128 {
        return Err("SSH configuration Include limit exceeded.".into());
    }
    let mut text = String::new();
    file.take((8 * 1024 * 1024 - budget.bytes + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    budget.bytes += text.len();
    if budget.bytes > 8 * 1024 * 1024 {
        return Err("SSH configuration exceeds the 8 MiB import limit.".into());
    }
    budget.stack.push(canonical);
    let parsed = SshConfig::parse(&text)?;
    budget.requests_final |= parsed.requests_final;
    budget.warnings.extend(
        parsed
            .warnings
            .iter()
            .take(128usize.saturating_sub(budget.warnings.len()))
            .cloned(),
    );
    let mut entries = Vec::new();
    for mut entry in parsed.entries {
        if entry.key != "include" {
            entry.guards = guards.to_vec();
            entries.push(entry);
            continue;
        }
        let mut inherited = guards.to_vec();
        inherited.push(entry.scope.clone());
        for pattern in entry.include_paths {
            let pattern = match expand_include(&pattern, environment)? {
                Ok(pattern) => pattern,
                Err(warning) => {
                    if budget.warnings.len() < 128 {
                        budget.warnings.push(warning);
                    }
                    continue;
                }
            };
            if pattern.starts_with('~') && !pattern.starts_with("~/") && !pattern.starts_with("~\\")
            {
                return Err(
                    "SSH Include paths using another user's home are not supported yet.".into(),
                );
            }
            let pattern = if pattern.starts_with("~/") || pattern.starts_with("~\\") {
                base.parent().unwrap_or(base).join(&pattern[2..])
            } else if Path::new(&pattern).is_absolute() {
                pattern.into()
            } else {
                base.join(pattern)
            };
            let pattern = pattern
                .to_str()
                .ok_or("SSH Include path is not valid Unicode.")?;
            let mut paths = glob::glob(pattern)
                .map_err(|_| "Invalid SSH Include glob pattern.")?
                .take(129)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            paths.sort();
            if paths.len() > 128 {
                return Err("SSH Include matches too many files.".into());
            }
            for path in paths {
                entries.extend(load_entries(&path, base, &inherited, budget, environment)?);
            }
        }
    }
    budget.stack.pop();
    Ok(entries)
}

// Missing variables skip just this Include path, as OpenSSH does. Malformed
// expressions and unsupported host-dependent tokens remain explicit errors.
fn expand_include(
    input: &str,
    environment: &mut impl FnMut(&str) -> Option<String>,
) -> Result<Result<String, String>, String> {
    let mut expanded = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '$' if chars.peek() == Some(&'{') => {
                chars.next();
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch)
                            if name.len() < 256 && (ch.is_ascii_alphanumeric() || ch == '_') =>
                        {
                            name.push(ch)
                        }
                        _ => {
                            return Err(
                                "Invalid environment variable expression in SSH Include.".into()
                            )
                        }
                    }
                }
                if name.is_empty() {
                    return Err("Empty environment variable in SSH Include.".into());
                }
                let Some(value) = environment(&name) else {
                    return Ok(Err(format!("Skipped SSH Include: environment variable {name} is missing or is not valid Unicode.")));
                };
                expanded.push_str(&value);
            }
            '%' => match chars.next() {
                Some('%') => expanded.push('%'),
                _ => {
                    return Err(
                        "Host-dependent percent tokens in SSH Include are not supported yet."
                            .into(),
                    )
                }
            },
            ch => expanded.push(ch),
        }
        if expanded.len() > 32768 || expanded.contains('\0') {
            return Err("Expanded SSH Include path is invalid or exceeds 32 KiB.".into());
        }
    }
    if expanded.is_empty() {
        return Err("Expanded SSH Include path is empty.".into());
    }
    Ok(Ok(expanded))
}

fn tokenize(line: &str) -> Result<Vec<String>, ()> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Preserve Windows paths; only consume escapes used for delimiters.
            if chars
                .peek()
                .is_some_and(|next| matches!(*next, '"' | '\'' | '#' | ' ' | '\\'))
            {
                word.push(chars.next().ok_or(())?);
            } else {
                word.push(ch);
            }
        } else if Some(ch) == quote {
            quote = None;
        } else if quote.is_none() && matches!(ch, '"' | '\'') {
            quote = Some(ch);
        } else if quote.is_none() && ch == '#' {
            break;
        } else if quote.is_none()
            && (ch.is_whitespace()
                || (ch == '=' && (result.is_empty() || result.len() == 1 && word.is_empty())))
        {
            if !word.is_empty() {
                result.push(std::mem::take(&mut word));
            }
        } else {
            word.push(ch);
        }
    }
    if quote.is_some() {
        return Err(());
    }
    if !word.is_empty() {
        result.push(word);
    }
    Ok(result)
}

fn wildcard(pattern: &str, value: &str, insensitive: bool) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len()
            && (pattern[p] == b'?'
                || pattern[p] == value[v]
                || insensitive && pattern[p].eq_ignore_ascii_case(&value[v]))
        {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(at) = star {
            retry += 1;
            v = retry;
            p = at + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

pub fn expand(
    value: &str,
    alias: &str,
    host: &str,
    user: &str,
    port: u16,
    home: &Path,
) -> Result<String, String> {
    let value = if value == "~" {
        home.to_string_lossy().into_owned()
    } else if value.starts_with("~/") || value.starts_with("~\\") {
        home.join(&value[2..]).to_string_lossy().into_owned()
    } else {
        value.to_owned()
    };
    let mut chars = value.chars();
    let mut expanded = String::new();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            expanded.push(ch);
            continue;
        }
        match chars.next() {
            Some('%') => expanded.push('%'),
            Some('n') => expanded.push_str(alias),
            Some('h') => expanded.push_str(host),
            Some('r') => expanded.push_str(user),
            Some('p') => expanded.push_str(&port.to_string()),
            Some('d') => expanded.push_str(&home.to_string_lossy()),
            _ => return Err("Unsupported percent token in SSH configuration.".into()),
        }
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_pass_uses_resolved_hostname_original_alias_and_first_values() {
        let config = SshConfig::parse("Host prod\nHostName %h.example\nUser deploy\nPort 2200\nLocalForward 9000 localhost:90\nMatch final host prod.example originalhost prod user deploy\nServerAliveInterval 13\nPort 9999\nLocalForward 9000 localhost:90\nLocalForward 9001 localhost:91\nHost prod.example\nConnectTimeout 7\nMatch final all\nServerAliveInterval 19\n").unwrap();
        let resolved = config.resolve_with_context("prod", None, "local");
        assert_eq!(resolved.values["port"], "2200");
        assert_eq!(resolved.values["serveraliveinterval"], "13");
        assert_eq!(resolved.values["connecttimeout"], "7");
        assert_eq!(
            resolved.all_values("localforward"),
            ["9000 localhost:90", "9001 localhost:91"]
        );
        assert_eq!(
            config
                .resolve_with_context("prod", Some("root"), "local")
                .values["serveraliveinterval"],
            "19"
        );
        assert!(config.warnings().is_empty());
    }

    #[test]
    fn empty_final_requests_reparse_but_negated_final_does_not() {
        let text = "Host alias\nHostName actual\nHost actual\nPort 2222\nMatch !final\nServerAliveInterval 9\n";
        let config = SshConfig::parse(text).unwrap();
        assert!(!config.resolve("alias").contains_key("port"));
        let config = SshConfig::parse(&format!("{text}Match final\n")).unwrap();
        assert_eq!(config.resolve("alias")["port"], "2222");
        let config =
            SshConfig::parse("Match final\nHostName cannot-change-final-host\nUser final-user\n")
                .unwrap();
        assert_eq!(config.resolve("alias")["hostname"], "alias");
        assert_eq!(config.resolve("alias")["user"], "final-user");
        for text in [
            "Match final all user root",
            "Match user root all",
            "Match final all=anything",
        ] {
            assert!(SshConfig::parse(text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn final_conditions_in_includes_retain_scope_and_do_not_duplicate_forwards() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config"), "Host prod\nHostName actual\nMatch originalhost prod\nInclude final.conf\nMatch final all\nPort 2222\n").unwrap();
        std::fs::write(directory.path().join("final.conf"), "LocalForward 9000 localhost:90\nMatch final host actual\nServerAliveInterval 17\nLocalForward 9001 localhost:91\n").unwrap();
        let config = SshConfig::load(&directory.path().join("config")).unwrap();
        let prod = config.resolve_with_context("prod", None, "local");
        assert_eq!(prod.values["serveraliveinterval"], "17");
        assert_eq!(prod.all_values("localforward").len(), 2);
        let other = config.resolve_with_context("other", None, "local");
        assert!(!other.values.contains_key("serveraliveinterval"));
        assert!(other.all_values("localforward").is_empty());
    }

    #[test]
    fn match_conditions_use_ordered_context_negation_and_case_sensitive_users() {
        let config = SshConfig::parse("Host prod\nHostName %h.example\nUser deploy\nMatch host *.EXAMPLE,!bad.example originalhost prod user deploy localuser Local\nPort 2222\nLocalForward 9000 localhost:90\nMatch !user root host=prod.example\nLocalForward 9001 localhost:91\nMatch user Deploy\nIdentityFile wrong-case\nMatch !all\nIdentityFile never\nMatch all\nServerAliveInterval 17\nPort 22\n").unwrap();
        let resolved = config.resolve_with_context("prod", None, "Local");
        assert_eq!(resolved.values["port"], "2222");
        assert_eq!(resolved.values["serveraliveinterval"], "17");
        assert!(!resolved.values.contains_key("identityfile"));
        assert_eq!(
            resolved.all_values("localforward"),
            ["9000 localhost:90", "9001 localhost:91"]
        );
        let root = config.resolve_with_context("prod", Some("root"), "Local");
        assert_eq!(root.values["port"], "22");
        assert!(root.all_values("localforward").is_empty());
        assert_eq!(
            config.resolve_with_context("prod", None, "local").values["port"],
            "22"
        );
        assert_eq!(
            config.resolve_with_context("other", None, "Local").values["port"],
            "22"
        );
        assert!(config.warnings().is_empty());
    }

    #[test]
    fn match_block_is_frozen_before_its_user_and_hostname_directives() {
        let config = SshConfig::parse("Match user local host alias\nUser changed\nHostName changed.example\nPort 2222\nMatch user changed host changed.example\nServerAliveInterval 9\nMatch all\nPort 22\n").unwrap();
        let resolved = config.resolve_with_context("alias", None, "local");
        assert_eq!(resolved.values["port"], "2222");
        assert_eq!(resolved.values["serveraliveinterval"], "9");
    }

    #[test]
    fn conditional_includes_keep_match_context_and_propagate_warnings() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config"), "Host prod\nUser deploy\nMatch originalhost prod user deploy\nInclude matched.conf\nPort 2222\nMatch all\nPort 22\n").unwrap();
        std::fs::write(directory.path().join("matched.conf"), "LocalForward 9000 localhost:90\nMatch localuser local\nServerAliveInterval 11\nMatch exec unsafe\nHostName must-not-leak\nMatch all\nLocalForward 9001 localhost:91\n").unwrap();
        let config = SshConfig::load(&directory.path().join("config")).unwrap();
        let prod = config.resolve_with_context("prod", None, "local");
        assert_eq!(prod.values["port"], "2222");
        assert_eq!(prod.values["serveraliveinterval"], "11");
        assert_eq!(prod.all_values("localforward").len(), 2);
        assert!(!prod.values.contains_key("hostname"));
        let other = config.resolve_with_context("other", None, "local");
        assert_eq!(other.values["port"], "22");
        assert!(other.all_values("localforward").is_empty());
        assert!(!other.values.contains_key("serveraliveinterval"));
        assert_eq!(config.warnings().len(), 1);
        assert!(config.warnings()[0].contains("exec"));
        for invalid in ["Match", "Match host", "Match user a,,b", "Match host="] {
            assert!(SshConfig::parse(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn environment_includes_expand_before_globbing_and_preserve_scope_and_order() {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path();
        std::fs::create_dir(base.join("parts with spaces")).unwrap();
        std::fs::write(base.join("config"), "Host prod\nInclude \"${PARTS}/*.conf\" ${MISSING}/none ${NESTED}\nPort 2022\nHost next\nUser next\n").unwrap();
        std::fs::write(
            base.join("parts with spaces/01.conf"),
            "User first\nInclude ${NESTED}\n",
        )
        .unwrap();
        std::fs::write(base.join("parts with spaces/02.conf"), "User second\n").unwrap();
        std::fs::write(base.join("literal%name.conf"), "ServerAliveInterval 19\n").unwrap();
        let mut environment = |name: &str| match name {
            "PARTS" => Some("parts with spaces".into()),
            "NESTED" => Some("literal%name.conf".into()),
            _ => None,
        };
        let config =
            SshConfig::load_with_environment(&base.join("config"), &mut environment).unwrap();
        let prod = config.resolve("prod");
        assert_eq!(prod["user"], "first");
        assert_eq!(prod["port"], "2022");
        assert_eq!(prod["serveraliveinterval"], "19");
        assert_eq!(config.resolve("next")["user"], "next");
        assert!(!config.resolve("next").contains_key("serveraliveinterval"));
        assert_eq!(config.warnings().len(), 1);
        assert!(config.warnings()[0].contains("MISSING"));
        std::fs::write(base.join("literal%name.conf"), "Include config\n").unwrap();
        assert!(
            SshConfig::load_with_environment(&base.join("config"), &mut environment)
                .err()
                .unwrap()
                .contains("cycle")
        );
    }

    #[test]
    fn include_expansion_is_single_pass_bounded_and_rejects_malformed_variables() {
        let mut environment = |name: &str| match name {
            "VALUE" => Some("${OTHER}/%h".into()),
            "EMPTY" => Some(String::new()),
            "HUGE" => Some("x".repeat(32769)),
            _ => None,
        };
        assert_eq!(
            expand_include("${VALUE}/%%/$literal", &mut environment)
                .unwrap()
                .unwrap(),
            "${OTHER}/%h/%/$literal"
        );
        assert!(expand_include("${MISSING}/a", &mut environment)
            .unwrap()
            .is_err());
        for input in ["${}", "${VALUE", "${BAD=NAME}", "${EMPTY}", "${HUGE}", "%h"] {
            assert!(expand_include(input, &mut environment).is_err(), "{input}");
        }
    }
    #[test]
    fn includes_are_ordered_inherit_scope_and_do_not_change_the_parent_scope() {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path();
        std::fs::create_dir(base.join("parts")).unwrap();
        std::fs::write(
            base.join("config"),
            "Host prod\nInclude parts/*.conf missing.conf\nPort 2222\nHost next\nUser next-user\n",
        )
        .unwrap();
        std::fs::write(
            base.join("parts/01.conf"),
            "User first\nInclude nested.conf\nHost foreign\nHostName excluded\n",
        )
        .unwrap();
        std::fs::write(base.join("parts/02.conf"), "User second\n").unwrap();
        std::fs::write(base.join("nested.conf"), "ServerAliveInterval 17\n").unwrap();
        let config = SshConfig::load(&base.join("config")).unwrap();
        let prod = config.resolve("prod");
        assert_eq!(prod["user"], "first");
        assert_eq!(prod["port"], "2222");
        assert_eq!(prod["serveraliveinterval"], "17");
        assert!(!prod.contains_key("hostname"));
        assert!(config.resolve("foreign").is_empty());
        assert_eq!(config.resolve("next")["user"], "next-user");
        assert_eq!(config.aliases(), ["next", "prod"]);
        std::fs::write(base.join("nested.conf"), "Include config\n").unwrap();
        assert!(SshConfig::load(&base.join("config"))
            .err()
            .unwrap()
            .contains("cycle"));
    }
    #[test]
    fn first_value_wins_patterns_exclude_and_match_sections_do_not_leak() {
        let config = SshConfig::parse("User global\nHost prod !prod-old\nHostName = 10.0.0.1\nUser deploy\nPort 2222\nHost *\nPort 22\nMatch exec unsafe\nHostName malicious\nHost next\nHostName other\n").unwrap();
        let values = config.resolve("prod");
        assert_eq!(values["user"], "global");
        assert_eq!(values["hostname"], "10.0.0.1");
        assert_eq!(values["port"], "2222");
        assert!(!config.resolve("prod-old").contains_key("hostname"));
        assert_eq!(config.resolve("next")["hostname"], "other");
        assert_eq!(config.aliases(), ["next", "prod"]);
    }
    #[test]
    fn quoted_paths_and_comments_preserve_spaces_and_backslashes() {
        let values = SshConfig::parse(
            "Host a\n IdentityFile \"C:\\Users\\A B\\key\" # ignored\n HostName example\n",
        )
        .unwrap()
        .resolve("a");
        assert_eq!(values["identityfile"], "C:\\Users\\A B\\key");
        assert!(SshConfig::parse("IdentityFile \"unfinished").is_err());
    }
    #[test]
    fn expansion_is_literal_and_does_not_recursively_expand_inserted_values() {
        let expanded = expand(
            "~/keys/%n-%r-%p-%%",
            "alias",
            "host",
            "user%h",
            22,
            Path::new("/home/test"),
        )
        .unwrap();
        assert!(expanded.ends_with("alias-user%h-22-%"));
        assert!(expand("%x", "a", "h", "u", 22, Path::new("/")).is_err());
    }
}
