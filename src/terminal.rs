//! Ordered terminal protocol processing shared by desktop and CLI sessions.
use std::collections::VecDeque;

const CARRY_LIMIT: usize = 16 * 1024;
const CAPTURE_LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Modes {
    pub bracketed_paste: bool,
    pub click: bool,
    pub drag: bool,
    pub motion: bool,
    pub sgr: bool,
    pub utf8_mouse: bool,
    pub alternate_scroll: bool,
}

#[derive(Default)]
pub struct Output {
    pub display: Vec<u8>,
    pub replies: Vec<u8>,
}

pub struct Terminal {
    pub colors: crate::preferences::Colors,
    parser: vt100::Parser,
    carry: Vec<u8>,
    capture: VecDeque<u8>,
    pub modes: Modes,
    pub received: u64,
    pub remote_directory: Option<String>,
}

impl Terminal {
    pub fn new(rows: u16, columns: u16, history: usize) -> Self {
        Self {
            colors: crate::preferences::Colors::default(),
            parser: vt100::Parser::new(rows.max(1), columns.max(1), history),
            carry: Vec::new(),
            capture: VecDeque::new(),
            modes: Modes::default(),
            received: 0,
            remote_directory: None,
        }
    }
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }
    pub fn resize(&mut self, rows: u16, columns: u16) {
        self.parser.set_size(rows.max(1), columns.max(1));
    }
    pub fn scrollback(&mut self, offset: usize) {
        self.parser.set_scrollback(offset);
    }
    pub fn recent_output(&self) -> Vec<u8> {
        self.capture.iter().copied().collect()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Output {
        self.received = self.received.saturating_add(bytes.len() as u64);
        let retain = bytes.len().min(CAPTURE_LIMIT);
        let excess = self
            .capture
            .len()
            .saturating_add(retain)
            .saturating_sub(CAPTURE_LIMIT);
        self.capture.drain(..excess);
        self.capture.extend(&bytes[bytes.len() - retain..]);
        let mut input = std::mem::take(&mut self.carry);
        input.extend_from_slice(bytes);
        let mut result = Output::default();
        let mut at = 0;
        while at < input.len() {
            if input[at] != 0x1b {
                let length = input[at..]
                    .iter()
                    .position(|b| *b == 0x1b)
                    .unwrap_or(input.len() - at);
                self.display(&input[at..at + length], &mut result);
                at += length;
                continue;
            }
            let tail = &input[at..];
            let Some(length) = escape_len(tail) else {
                if tail.len() <= CARRY_LIMIT {
                    self.carry.extend_from_slice(tail);
                } else {
                    self.display(tail, &mut result);
                }
                break;
            };
            let escape = &tail[..length];
            if !self.intercept(escape, &mut result.replies) {
                self.display(escape, &mut result);
            }
            at += length;
        }
        result
    }

    fn display(&mut self, bytes: &[u8], output: &mut Output) {
        if let Some(body) = bytes.strip_prefix(b"\x1b]8;") {
            let destination = body
                .strip_suffix(b"\x07")
                .or_else(|| body.strip_suffix(b"\x1b\\"))
                .and_then(|body| body.splitn(2, |b| *b == b';').nth(1))
                .and_then(|uri| std::str::from_utf8(uri).ok());
            self.parser.set_hyperlink(destination);
        }
        self.parser.process(bytes);
        output.display.extend_from_slice(bytes);
    }

    fn intercept(&mut self, bytes: &[u8], replies: &mut Vec<u8>) -> bool {
        match bytes {
            b"\x1b[5n" => replies.extend_from_slice(b"\x1b[0n"),
            b"\x1b[6n" | b"\x1b[?6n" => {
                let (row, column) = self.screen().cursor_position();
                let prefix = if bytes == b"\x1b[?6n" { "?" } else { "" };
                replies.extend_from_slice(
                    format!("\x1b[{prefix}{};{}R", row + 1, column + 1).as_bytes(),
                );
            }
            b"\x1b[c" | b"\x1b[0c" => replies.extend_from_slice(b"\x1b[?1;2c"),
            b"\x1b[>c" | b"\x1b[>0c" => replies.extend_from_slice(b"\x1b[>0;10;1c"),
            b"\x1bc" => {
                self.modes = Modes::default();
                self.remote_directory = None;
                return false;
            }
            _ => {
                if bytes.starts_with(b"\x1b[") {
                    let body = &bytes[2..bytes.len() - 1];
                    let final_byte = bytes[bytes.len() - 1];
                    if final_byte == b'R' {
                        let body = body.strip_prefix(b"?").unwrap_or(body);
                        let fields: Vec<_> = body.split(|b| *b == b';').collect();
                        if fields.len() == 2
                            && fields
                                .iter()
                                .all(|f| !f.is_empty() && f.iter().all(u8::is_ascii_digit))
                        {
                            return true;
                        }
                    }
                    if let Some(body) = body.strip_prefix(b"?") {
                        if final_byte == b'h' || final_byte == b'l' {
                            let enabled = final_byte == b'h';
                            for field in body.split(|b| *b == b';') {
                                match field {
                                    b"1000" => self.modes.click = enabled,
                                    b"1002" => self.modes.drag = enabled,
                                    b"1003" => self.modes.motion = enabled,
                                    b"1005" => self.modes.utf8_mouse = enabled,
                                    b"1006" => self.modes.sgr = enabled,
                                    b"1007" => self.modes.alternate_scroll = enabled,
                                    b"2004" => self.modes.bracketed_paste = enabled,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if let Some(body) = bytes.strip_prefix(b"\x1b]") {
                    let body = body
                        .strip_suffix(b"\x07")
                        .or_else(|| body.strip_suffix(b"\x1b\\"))
                        .unwrap_or(body);
                    if let Some(uri) = body.strip_prefix(b"7;") {
                        if let Some(path) = std::str::from_utf8(uri)
                            .ok()
                            .and_then(crate::remote_directory::from_file_uri)
                        {
                            self.remote_directory = Some(path);
                        }
                    }
                    if let Some(kind) = body.strip_suffix(b";?") {
                        let color = match kind {
                            b"10" => Some(self.colors.foreground),
                            b"11" => Some(self.colors.background),
                            b"12" => Some(self.colors.cursor),
                            // OSC 4;N;? palette query (answered by ssh4gui).
                            _ => kind
                                .strip_prefix(b"4;")
                                .and_then(|index| std::str::from_utf8(index).ok())
                                .filter(|index| index.bytes().all(|b| b.is_ascii_digit()))
                                .and_then(|index| index.parse::<u8>().ok())
                                .map(|index| self.colors.indexed(index)),
                        };
                        if let Some(color) = color {
                            let color = color
                                .map(|v| format!("{:04x}", u16::from(v) * 257))
                                .join("/");
                            replies.extend_from_slice(b"\x1b]");
                            replies.extend_from_slice(kind);
                            replies.extend_from_slice(format!(";rgb:{color}\x1b\\").as_bytes());
                            return true;
                        }
                    }
                }
                return false;
            }
        }
        true
    }

    /// Bytes that paste `text`. With bracketed paste, ESC and ^C are dropped (as
    /// Alacritty does): pasted text could otherwise end the paste early with
    /// its own `ESC[201~` and run the rest as typed commands, and some shells
    /// end a paste on ^C.
    pub fn paste(&self, text: &str) -> Vec<u8> {
        if !self.modes.bracketed_paste {
            return text.as_bytes().to_vec();
        }
        let text = text.replace(['\x1b', '\x03'], "");
        if text.is_empty() {
            return Vec::new();
        }
        [b"\x1b[200~".as_slice(), text.as_bytes(), b"\x1b[201~"].concat()
    }
}

/// Paste text for a confirmation display: line breaks (CR, LF or CRLF) are
/// shown as lines, and control and bidirectional-formatting characters are
/// escaped (`\u{1b}`) so they can neither act on the local terminal nor hide or
/// reorder what is about to be sent.
pub fn paste_preview(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    for c in text.replace("\r\n", "\n").replace('\r', "\n").chars() {
        let bidi = matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
        if (c.is_control() && c != '\n' && c != '\t') || bidi {
            shown.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
        } else {
            shown.push(c);
        }
    }
    shown
}

fn escape_len(bytes: &[u8]) -> Option<usize> {
    let kind = *bytes.get(1)?;
    match kind {
        b'[' => bytes[2..]
            .iter()
            .position(|b| (0x40..=0x7e).contains(b))
            .map(|i| i + 3),
        b']' | b'P' | b'_' | b'^' | b'X' => {
            for at in 2..bytes.len() {
                if kind == b']' && bytes[at] == 7 {
                    return Some(at + 1);
                }
                if bytes[at] == b'\\' && bytes[at - 1] == 0x1b {
                    return Some(at + 1);
                }
            }
            None
        }
        0x20..=0x2f => bytes[2..]
            .iter()
            .position(|b| (0x30..=0x7e).contains(b))
            .map(|i| i + 3),
        _ => Some(2),
    }
}

pub fn needs_paste_confirmation(text: &str) -> bool {
    text.contains(['\r', '\n'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_reports_survive_all_read_boundaries_and_ignore_invalid_updates() {
        let bytes = b"before\x1b]7;file://remote/work/a%20b\x1b\\after";
        for split in 0..=bytes.len() {
            let mut terminal = Terminal::new(3, 30, 10);
            terminal.feed(&bytes[..split]);
            terminal.feed(&bytes[split..]);
            assert_eq!(terminal.remote_directory.as_deref(), Some("/work/a b"));
            assert!(terminal.screen().contents().contains("beforeafter"));
            terminal.feed(b"\x1b]7;file://remote/bad%0a\x07");
            assert_eq!(terminal.remote_directory.as_deref(), Some("/work/a b"));
            terminal.feed(b"\x1b]7;file://remote/new\x07");
            assert_eq!(terminal.remote_directory.as_deref(), Some("/new"));
            terminal.feed(b"\x1bc");
            assert!(terminal.remote_directory.is_none());
        }
    }

    #[test]
    fn all_read_boundaries_preserve_cursor_reply_order_and_unicode() {
        let bytes = "A界\x1b[6n\x1b[4;9H\x1b[6nZ\x1b[5n".as_bytes();
        for split in 0..=bytes.len() {
            let mut terminal = Terminal::new(24, 80, 100);
            let mut replies = terminal.feed(&bytes[..split]).replies;
            replies.extend(terminal.feed(&bytes[split..]).replies);
            assert_eq!(replies, b"\x1b[1;4R\x1b[4;9R\x1b[0n", "split {split}");
            assert_eq!(terminal.screen().cursor_position(), (3, 9));
        }
    }

    #[test]
    fn osc_and_device_queries_work_one_byte_at_a_time() {
        let mut terminal = Terminal::new(24, 80, 0);
        let mut replies = Vec::new();
        for byte in b"\x1b[c\x1b[>0c\x1b]10;?\x07\x1b]11;?\x1b\\\x1b]12;?\x07" {
            replies.extend(terminal.feed(&[*byte]).replies);
        }
        assert_eq!(replies, b"\x1b[?1;2c\x1b[>0;10;1c\x1b]10;rgb:dada/e3e3/eded\x1b\\\x1b]11;rgb:0d0d/1313/1c1c\x1b\\\x1b]12;rgb:5353/d2d2/b9b9\x1b\\");
        terminal.colors = crate::preferences::Colors::for_theme("Amber");
        assert_eq!(
            terminal.feed(b"\x1b]11;?\x07").replies,
            b"\x1b]11;rgb:1e1e/1818/1212\x1b\\"
        );
    }

    #[test]
    fn osc_4_palette_queries_answer_theme_and_cube_colors_one_byte_at_a_time() {
        let mut terminal = Terminal::new(24, 80, 0);
        terminal.colors = crate::preferences::Colors::for_theme("Paper");
        let mut replies = Vec::new();
        for byte in b"\x1b]4;1;?\x07\x1b]4;196;?\x1b\\\x1b]4;232;?\x07" {
            replies.extend(terminal.feed(&[*byte]).replies);
        }
        assert_eq!(
            replies,
            b"\x1b]4;1;rgb:b3b3/2d2d/2323\x1b\\\x1b]4;196;rgb:ffff/0000/0000\x1b\\\x1b]4;232;rgb:0808/0808/0808\x1b\\"
        );
        // Out-of-range or malformed indexes are not answered.
        assert!(terminal.feed(b"\x1b]4;256;?\x07").replies.is_empty());
        assert!(terminal.feed(b"\x1b]4;+1;?\x07").replies.is_empty());
    }

    #[test]
    fn unknown_sequences_pass_through_without_answering_embedded_dcs_queries() {
        let bytes = b"\x1b[31mred\x1b[0m\x1bPpayload\x1b[6n\x1b\\\x1b]99;?\x07";
        for split in 0..=bytes.len() {
            let mut terminal = Terminal::new(24, 80, 0);
            let a = terminal.feed(&bytes[..split]);
            let b = terminal.feed(&bytes[split..]);
            assert_eq!([a.display, b.display].concat(), bytes);
            assert!(a.replies.is_empty() && b.replies.is_empty());
        }
    }

    #[test]
    fn mouse_modes_are_independent_and_reset_with_terminal() {
        let mut terminal = Terminal::new(24, 80, 0);
        terminal.feed(b"\x1b[?1000;1002;1003;1006;2004h");
        terminal.feed(b"\x1b[?1002l");
        assert!(terminal.modes.click && terminal.modes.motion && terminal.modes.sgr);
        assert!(!terminal.modes.drag);
        assert_eq!(
            terminal.paste(" a\r\n界 "),
            "\x1b[200~ a\r\n界 \x1b[201~".as_bytes()
        );
        assert!(terminal.paste("").is_empty());
        terminal.feed(b"\x1bc");
        assert_eq!(terminal.modes, Modes::default());
    }

    #[test]
    fn carry_and_diagnostic_capture_are_bounded() {
        let mut terminal = Terminal::new(24, 80, 0);
        let bytes = [b"\x1b]".as_slice(), &vec![b'a'; CAPTURE_LIMIT * 2]].concat();
        terminal.feed(&bytes);
        assert!(terminal.carry.len() <= CARRY_LIMIT);
        assert_eq!(terminal.recent_output().len(), CAPTURE_LIMIT);
        assert_eq!(terminal.received, bytes.len() as u64);
    }

    #[test]
    fn strips_cpr_responses_and_confirms_all_line_endings() {
        let mut terminal = Terminal::new(24, 80, 0);
        assert_eq!(terminal.feed(b"a\x1b[12;20Rb").display, b"ab");
        for text in ["a\nb", "a\rb", "a\r\nb", "a\n"] {
            assert!(needs_paste_confirmation(text));
        }
        assert!(!needs_paste_confirmation(" a界 "));
    }

    #[test]
    fn bracketed_paste_cannot_end_early_or_interrupt() {
        let mut terminal = Terminal::new(24, 80, 0);
        // Without bracketed paste the text is typed exactly.
        assert_eq!(terminal.paste("a\x1b[201~b"), b"a\x1b[201~b");
        terminal.feed(b"\x1b[?2004h");
        // ESC and ^C are dropped, so no end marker can form, even by nesting.
        for (text, sent) in [
            ("ls\x1b[201~\ncurl x|sh\n", "ls[201~\ncurl x|sh\n"),
            ("a\x1b[20\x1b[201~1~b", "a[20[201~1~b"),
            ("x\x03y", "xy"),
        ] {
            assert_eq!(
                terminal.paste(text),
                format!("\x1b[200~{sent}\x1b[201~").as_bytes()
            );
        }
        assert!(terminal.paste("\x1b\x03").is_empty());
    }

    #[test]
    fn paste_preview_shows_hidden_characters() {
        assert_eq!(paste_preview("a\r\nb\rc\n\td"), "a\nb\nc\n\td");
        assert_eq!(
            paste_preview("rm -rf ~ \x1b[2K\x7f echo \u{202e}x\u{9b}"),
            "rm -rf ~ \\u{1b}[2K\\u{7f} echo \\u{202e}x\\u{9b}"
        );
    }
}
