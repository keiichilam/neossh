//! Web and remote-path hit testing in terminal cell coordinates, including soft wraps.
pub type Position = (u16, u16);
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    Web(String),
    RemotePath(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub destination: Destination,
    pub start: Position,
    pub end: Position,
}
impl Link {
    pub fn text(&self) -> &str {
        match &self.destination {
            Destination::Web(value) | Destination::RemotePath(value) => value,
        }
    }
    pub fn contains(&self, position: Position) -> bool {
        self.start <= position && position <= self.end
    }
}

pub fn at(screen: &vt100::Screen, point: Position) -> Option<Link> {
    let (rows, columns) = screen.size();
    if point.0 >= rows || point.1 >= columns {
        return None;
    }
    if let Some(raw) = screen.cell(point.0, point.1)?.hyperlink() {
        // Explicit destinations take precedence over URL-looking label text.
        let destination = web_destination(raw).map(Destination::Web).or_else(|| {
            crate::remote_directory::from_file_uri(raw)
                // Check after percent decoding: the tooltip displays this path.
                .filter(|path| !has_direction_controls(path))
                .map(Destination::RemotePath)
        })?;
        let mut start = point.1;
        let mut end = point.1;
        while start > 0 && screen.cell(point.0, start - 1)?.hyperlink() == Some(raw) {
            start -= 1;
        }
        while end + 1 < columns && screen.cell(point.0, end + 1)?.hyperlink() == Some(raw) {
            end += 1;
        }
        return Some(Link {
            destination,
            start: (point.0, start),
            end: (point.0, end),
        });
    }
    let mut first = point.0;
    let mut last = point.0;
    while first > 0 && screen.row_wrapped(first - 1) {
        first -= 1;
        if usize::from(point.0 - first + 1) * usize::from(columns) > 8192 {
            return None;
        }
    }
    while last + 1 < rows && screen.row_wrapped(last) {
        last += 1;
        if usize::from(last - first + 1) * usize::from(columns) > 8192 {
            return None;
        }
    }
    if usize::from(last - first + 1) * usize::from(columns) > 8192 {
        return None;
    }
    // A wrapped row at the viewport bottom has an unseen continuation. Do not
    // turn its visible prefix into a different, truncated destination.
    if last + 1 == rows && screen.row_wrapped(last) {
        return None;
    }
    let mut text = String::new();
    let mut positions = Vec::new();
    for row in first..=last {
        for column in 0..columns {
            let value = screen.cell(row, column)?;
            if value.is_wide_continuation() {
                continue;
            }
            let contents = value.contents();
            let contents = if contents.is_empty() { " " } else { &contents };
            let end = (
                row,
                if value.is_wide() {
                    (column + 1).min(columns - 1)
                } else {
                    column
                },
            );
            text.push_str(contents);
            positions.extend(std::iter::repeat_n(((row, column), end), contents.len()));
            if text.len() > 32768 {
                return None;
            }
        }
    }
    let lower = text.to_ascii_lowercase();
    for (start, _) in lower.match_indices("http") {
        if start > 0
            && text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '/' | ':' | '-'))
        {
            continue;
        }
        if !lower[start..].starts_with("http://") && !lower[start..].starts_with("https://") {
            continue;
        }
        let candidate = &text[start..];
        let end = candidate
            .find(|c: char| {
                c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '\'' | '`')
            })
            .unwrap_or(candidate.len());
        let mut raw = &candidate[..end];
        if raw.len() > 4096 {
            continue;
        }
        let mut closing = [0i32; 3];
        for c in raw.chars() {
            match c {
                '(' => closing[0] -= 1,
                ')' => closing[0] += 1,
                '[' => closing[1] -= 1,
                ']' => closing[1] += 1,
                '{' => closing[2] -= 1,
                '}' => closing[2] += 1,
                _ => {}
            }
        }
        while let Some(tail) = raw.chars().next_back() {
            let unmatched = match tail {
                ')' => closing[0] > 0,
                ']' => closing[1] > 0,
                '}' => closing[2] > 0,
                _ => false,
            };
            if matches!(tail, '.' | ',' | ';') || unmatched {
                match tail {
                    ')' => closing[0] -= 1,
                    ']' => closing[1] -= 1,
                    '}' => closing[2] -= 1,
                    _ => {}
                }
                raw = &raw[..raw.len() - tail.len_utf8()];
            } else {
                break;
            }
        }
        if raw.len() > 4096 || has_direction_controls(raw) {
            continue;
        }
        let Ok(url) = url::Url::parse(raw) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            continue;
        }
        let link = Link {
            destination: Destination::Web(url.into()),
            start: positions[start].0,
            end: positions[start + raw.len() - 1].1,
        };
        if link.contains(point) {
            return Some(link);
        }
    }
    for (start, _) in text.match_indices('/') {
        let before = text[..start].chars().next_back();
        if before.is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '{' | '\'' | '"'))
        {
            continue;
        }
        let candidate = &text[start..];
        if candidate.starts_with("//") {
            continue;
        }
        let quoted = before.filter(|c| matches!(c, '\'' | '"'));
        let end = if let Some(quote) = quoted {
            let Some(end) = candidate.find(quote) else {
                continue;
            };
            end
        } else {
            candidate
                .find(|c: char| {
                    c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '\'' | '"' | '`')
                })
                .unwrap_or(candidate.len())
        };
        let raw = if quoted.is_some() {
            &candidate[..end]
        } else {
            candidate[..end].trim_end_matches(['.', ',', ';', ':'])
        };
        let closing = match before {
            Some('(') => Some(')'),
            Some('[') => Some(']'),
            Some('{') => Some('}'),
            _ => None,
        };
        let raw = closing
            .and_then(|closing| raw.strip_suffix(closing))
            .unwrap_or(raw);
        if raw.len() < 2
            || crate::remote_directory::validate(raw).is_err()
            || has_direction_controls(raw)
        {
            continue;
        }
        let link = Link {
            destination: Destination::RemotePath(raw.into()),
            start: positions[start].0,
            end: positions[start + raw.len() - 1].1,
        };
        if link.contains(point) {
            return Some(link);
        }
    }
    None
}

fn web_destination(raw: &str) -> Option<String> {
    if raw.len() > 4096
        || raw.chars().any(|c| c.is_control() || c.is_whitespace())
        || has_direction_controls(raw)
    {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some()).then(|| url.into())
}

fn has_direction_controls(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_links_reject_decoded_direction_controls_without_rejecting_unicode_names() {
        let mut terminal = crate::terminal::Terminal::new(3, 80, 0);
        for control in [
            '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}',
            '\u{2068}', '\u{2069}',
        ] {
            let encoded: String = control
                .to_string()
                .bytes()
                .map(|byte| format!("%{byte:02X}"))
                .collect();
            for path in [
                format!("/tmp/{control}name.txt"),
                format!("/tmp/{encoded}name.txt"),
            ] {
                terminal
                    .feed(format!("\r\x1b]8;;file://remote{path}\x07label\x1b]8;;\x07").as_bytes());
                assert!(at(terminal.screen(), (0, 0)).is_none(), "{path:?}");
            }
        }
        terminal.feed(
            "\r\x1b]8;;file://remote/tmp/%E6%96%87%20name.txt\x07label\x1b]8;;\x07".as_bytes(),
        );
        assert_eq!(
            at(terminal.screen(), (0, 0)).unwrap().destination,
            Destination::RemotePath("/tmp/文 name.txt".into())
        );
    }

    #[test]
    fn explicit_links_survive_fragments_wrap_history_and_cell_edits() {
        let bytes =
            "\x1b]8;id=test;https://example.org/a;b;c\x1b\\A界BCDEF\x1b]8;;\x07!".as_bytes();
        for split in 0..=bytes.len() {
            let mut terminal = crate::terminal::Terminal::new(3, 6, 10);
            terminal.feed(&bytes[..split]);
            terminal.feed(&bytes[split..]);
            for point in [(0, 0), (0, 1), (0, 2), (0, 5), (1, 0), (1, 1)] {
                assert_eq!(
                    at(terminal.screen(), point).unwrap().text(),
                    "https://example.org/a;b;c",
                    "split {split}"
                );
            }
            assert!(at(terminal.screen(), (1, 2)).is_none());
            terminal.feed(b"\x1b[1;1HX\x1b[1;5H\x1b[K");
            assert!(at(terminal.screen(), (0, 0)).is_none());
            assert!(at(terminal.screen(), (0, 4)).is_none());
            assert!(at(terminal.screen(), (0, 2)).is_some());
            terminal.feed(b"\x1b[3;1H\r\n\r\n");
            terminal.scrollback(2);
            assert!(at(terminal.screen(), (0, 2)).is_some());
            let snapshot = terminal.screen().clone();
            terminal.feed(b"\x1bcplain");
            assert!(at(terminal.screen(), (0, 2)).is_none());
            assert!(at(&snapshot, (0, 2)).is_some());
        }
    }

    #[test]
    fn explicit_destinations_are_complete_and_reject_unsafe_schemes() {
        let mut terminal = crate::terminal::Terminal::new(3, 80, 0);
        let destination = format!("https://example.org/{}", "a;".repeat(30));
        terminal.feed(format!("\x1b]8;;{destination}\x07label\x1b]8;;\x07").as_bytes());
        assert_eq!(at(terminal.screen(), (0, 0)).unwrap().text(), destination);
        for destination in [
            "javascript:alert(1)",
            "file://user@host/tmp/file",
            "https://example.org/\u{202e}fake",
        ] {
            terminal.feed(
                format!("\r\x1b]8;;{destination}\x07https://example.com\x1b]8;;\x07").as_bytes(),
            );
            assert!(at(terminal.screen(), (0, 0)).is_none());
        }
        terminal.feed(
            format!(
                "\r\x1b]8;;https://example.org/{}\x07label\x1b]8;;\x07\x1b[K",
                "a".repeat(4096)
            )
            .as_bytes(),
        );
        assert!(at(terminal.screen(), (0, 0)).is_none());
    }

    #[test]
    fn web_links_share_wide_cell_and_soft_wrap_coordinates() {
        let mut parser = vt100::Parser::new(5, 20, 10);
        parser.process(
            "界 https://example.com/a(b).\r\nfile:///tmp/a javascript:alert(1)".as_bytes(),
        );
        let link = at(parser.screen(), (0, 5)).unwrap();
        assert_eq!(link.start, (0, 3));
        assert_eq!(link.text(), "https://example.com/a(b)");
        assert_eq!(at(parser.screen(), link.end), Some(link));
        assert!(at(parser.screen(), (0, 1)).is_none());
        assert!(at(parser.screen(), (2, 3)).is_none());
        let mut parser = vt100::Parser::new(2, 80, 0);
        parser.process("https://example.org/界".as_bytes());
        let link = at(parser.screen(), (0, 21)).unwrap();
        assert_eq!(link.end, (0, 21));
        assert_eq!(link.text(), "https://example.org/%E7%95%8C");
    }
    #[test]
    fn hard_newlines_punctuation_ipv6_and_invalid_hosts_are_handled() {
        let mut parser = vt100::Parser::new(8, 80, 0);
        parser.process(b"(https://[::1]:8080/a), https://example.org\r\n/path\r\nhttp:// https://[invalid] xhttps://example.org");
        assert_eq!(
            at(parser.screen(), (0, 3)).unwrap().text(),
            "https://[::1]:8080/a"
        );
        assert_eq!(
            at(parser.screen(), (0, 26)).unwrap().text(),
            "https://example.org/"
        );
        assert_eq!(
            at(parser.screen(), (1, 2)).unwrap().destination,
            Destination::RemotePath("/path".into())
        );
        for col in [1, 10, 31] {
            assert!(at(parser.screen(), (2, col)).is_none());
        }
    }

    #[test]
    fn remote_paths_use_cell_coordinates_and_never_become_web_urls() {
        let mut parser = vt100::Parser::new(5, 14, 0);
        parser.process("界 /tmp/long/path.txt".as_bytes());
        let link = at(parser.screen(), (1, 2)).unwrap();
        assert_eq!(
            link.destination,
            Destination::RemotePath("/tmp/long/path.txt".into())
        );
        assert_eq!(link.start, (0, 3));
        assert_eq!(at(parser.screen(), link.end), Some(link));
        let mut parser = crate::terminal::Terminal::new(5, 100, 0);
        parser.feed("'/tmp/a b;$(literal) 界' https://host/tmp/x\r\n\x1b]8;;file://remote/tmp/a%20b%23c\x07label\x1b]8;;\x07\r\nx/tmp/no //network/share".as_bytes());
        assert_eq!(
            at(parser.screen(), (0, 10)).unwrap().destination,
            Destination::RemotePath("/tmp/a b;$(literal) 界".into())
        );
        assert_eq!(
            at(parser.screen(), (1, 1)).unwrap().destination,
            Destination::RemotePath("/tmp/a b#c".into())
        );
        for column in 0..23 {
            assert!(
                at(parser.screen(), (2, column)).is_none(),
                "column {column}"
            );
        }
    }
}
