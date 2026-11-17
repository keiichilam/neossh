//! UI-independent input encoding. UI adapters own shortcuts and modal focus.
use crate::terminal::Modes;

#[derive(Clone, Copy, Default, Debug)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

impl Modifiers {
    fn parameter(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Key {
    Character(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    Function(u8),
}

pub fn encode_key(key: Key, modifiers: Modifiers, application_cursor: bool) -> Vec<u8> {
    let parameter = modifiers.parameter();
    let mut bytes = match key {
        Key::Character(character) if modifiers.control => {
            let code = match character {
                'a'..='z' => character as u8 - b'a' + 1,
                'A'..='Z' => character as u8 - b'A' + 1,
                ' ' | '@' | '2' => 0,
                '[' | '3' => 27,
                '\\' | '4' => 28,
                ']' | '5' => 29,
                '^' | '6' => 30,
                // Ctrl+_ arrives as Ctrl+Shift+- from GUI keyboards; xterm
                // also sends ^_ for Ctrl+/.
                '_' | '7' | '-' | '/' => 31,
                '?' | '8' => 127,
                _ => return Vec::new(),
            };
            vec![code]
        }
        Key::Character(character) => character.to_string().into_bytes(),
        Key::Enter => vec![b'\r'],
        Key::Backspace => vec![127],
        Key::Escape => vec![27],
        Key::Tab if modifiers.shift => return b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        _ => {
            let cursor = match key {
                Key::Up => Some('A'),
                Key::Down => Some('B'),
                Key::Right => Some('C'),
                Key::Left => Some('D'),
                Key::Home => Some('H'),
                Key::End => Some('F'),
                _ => None,
            };
            if let Some(final_byte) = cursor {
                return if parameter != 1 {
                    format!("\x1b[1;{parameter}{final_byte}").into_bytes()
                } else {
                    format!(
                        "\x1b{}{final_byte}",
                        if application_cursor { 'O' } else { '[' }
                    )
                    .into_bytes()
                };
            }
            if let Key::Function(number @ 1..=4) = key {
                let final_byte = (b'P' + number - 1) as char;
                return if parameter == 1 {
                    format!("\x1bO{final_byte}").into_bytes()
                } else {
                    format!("\x1b[1;{parameter}{final_byte}").into_bytes()
                };
            }
            let code = match key {
                Key::Insert => 2,
                Key::Delete => 3,
                Key::PageUp => 5,
                Key::PageDown => 6,
                Key::Function(5) => 15,
                Key::Function(6) => 17,
                Key::Function(7) => 18,
                Key::Function(8) => 19,
                Key::Function(9) => 20,
                Key::Function(10) => 21,
                Key::Function(11) => 23,
                Key::Function(12) => 24,
                _ => return Vec::new(),
            };
            return if parameter == 1 {
                format!("\x1b[{code}~").into_bytes()
            } else {
                format!("\x1b[{code};{parameter}~").into_bytes()
            };
        }
    };
    if modifiers.alt {
        bytes.insert(0, 27);
    }
    bytes
}

#[derive(Clone, Copy)]
pub enum MouseAction {
    Press(u8),
    Release(u8),
    Move(Option<u8>),
    WheelUp,
    WheelDown,
}

/// Tracks remote button ownership so releasing Shift or leaving the grid
/// cannot turn a local selection into a remote drag (or strand a held button).
#[derive(Default)]
pub struct MouseTracker {
    pressed: [bool; 3],
    last: Option<(u16, u16)>,
}

impl MouseTracker {
    pub fn captured(&self) -> bool {
        self.pressed.iter().any(|pressed| *pressed)
    }

    pub fn release_all(&mut self, modes: Modes) -> Vec<u8> {
        let (column, row) = self.last.unwrap_or((0, 0));
        let mut bytes = Vec::new();
        for button in 0..3 {
            bytes.extend(self.event(
                modes,
                MouseAction::Release(button),
                column,
                row,
                Modifiers::default(),
            ));
        }
        self.last = None;
        bytes
    }

    pub fn event(
        &mut self,
        modes: Modes,
        action: MouseAction,
        column: u16,
        row: u16,
        mut modifiers: Modifiers,
    ) -> Vec<u8> {
        let action = match action {
            MouseAction::Press(button) if button < 3 => {
                let bytes = encode_mouse(modes, action, column, row, modifiers);
                if !bytes.is_empty() {
                    self.pressed[button as usize] = true;
                    self.last = Some((column, row));
                }
                return bytes;
            }
            MouseAction::Release(button) if button < 3 => {
                if !std::mem::take(&mut self.pressed[button as usize]) {
                    return Vec::new();
                }
                modifiers.shift = false;
                action
            }
            MouseAction::Move(_) => {
                if self.last == Some((column, row)) {
                    return Vec::new();
                }
                self.last = Some((column, row));
                MouseAction::Move(
                    self.pressed
                        .iter()
                        .position(|pressed| *pressed)
                        .map(|button| button as u8),
                )
            }
            _ => action,
        };
        encode_mouse(modes, action, column, row, modifiers)
    }
}

pub fn encode_mouse(
    modes: Modes,
    action: MouseAction,
    column: u16,
    row: u16,
    modifiers: Modifiers,
) -> Vec<u8> {
    // Shift always belongs to local selection, including wheel scrollback.
    if modifiers.shift || !(modes.click || modes.drag || modes.motion) {
        return Vec::new();
    }
    let (button, release) = match action {
        MouseAction::Press(button) if button < 3 => (u16::from(button), false),
        MouseAction::Release(button) if button < 3 => (u16::from(button), true),
        MouseAction::Move(Some(button)) if button < 3 && (modes.drag || modes.motion) => {
            (32 + u16::from(button), false)
        }
        MouseAction::Move(None) if modes.motion => (35, false),
        MouseAction::WheelUp => (64, false),
        MouseAction::WheelDown => (65, false),
        _ => return Vec::new(),
    };
    let modifier_bits = 8 * u16::from(modifiers.alt) + 16 * u16::from(modifiers.control);
    let x = u32::from(column) + 1;
    let y = u32::from(row) + 1;
    if modes.sgr {
        return format!(
            "\x1b[<{};{x};{y}{}",
            button + modifier_bits,
            if release { 'm' } else { 'M' }
        )
        .into_bytes();
    }
    let code = if release { 3 } else { button } + modifier_bits + 32;
    let mut bytes = b"\x1b[M".to_vec();
    if modes.utf8_mouse {
        // Extended coordinates are bounded by the UTF-8 mouse protocol.
        if x > 2015 || y > 2015 {
            return Vec::new();
        }
        for scalar in [u32::from(code), x + 32, y + 32] {
            bytes.extend(char::from_u32(scalar).unwrap().to_string().as_bytes());
        }
    } else {
        if x > 223 || y > 223 {
            return Vec::new();
        }
        bytes.extend([code as u8, (x + 32) as u8, (y + 32) as u8]);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracked_mouse_releases_owned_buttons_and_ignores_local_drags() {
        let modes = Modes {
            drag: true,
            sgr: true,
            ..Default::default()
        };
        let mut mouse = MouseTracker::default();
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert!(mouse
            .event(modes, MouseAction::Press(0), 2, 3, shift)
            .is_empty());
        assert!(mouse
            .event(modes, MouseAction::Move(None), 3, 3, Modifiers::default())
            .is_empty());
        assert!(!mouse.captured());
        assert_eq!(
            mouse.event(modes, MouseAction::Press(0), 2, 3, Modifiers::default()),
            b"\x1b[<0;3;4M"
        );
        assert_eq!(
            mouse.event(modes, MouseAction::Move(None), 3, 3, Modifiers::default()),
            b"\x1b[<32;4;4M"
        );
        assert!(mouse
            .event(modes, MouseAction::Move(None), 3, 3, Modifiers::default())
            .is_empty());
        assert_eq!(
            mouse.event(modes, MouseAction::Release(0), 3, 3, shift),
            b"\x1b[<0;4;4m"
        );
        assert!(!mouse.captured());
        mouse.event(modes, MouseAction::Press(2), 7, 8, Modifiers::default());
        assert_eq!(mouse.release_all(modes), b"\x1b[<2;8;9m");
        assert!(mouse.release_all(modes).is_empty());
    }
    #[test]
    fn modifiers_and_cursor_modes_have_distinct_encodings() {
        assert_eq!(encode_key(Key::Up, Modifiers::default(), true), b"\x1bOA");
        assert_eq!(
            encode_key(
                Key::Up,
                Modifiers {
                    control: true,
                    ..Default::default()
                },
                true
            ),
            b"\x1b[1;5A"
        );
        assert_eq!(
            encode_key(
                Key::Character('c'),
                Modifiers {
                    control: true,
                    alt: true,
                    shift: false
                },
                false
            ),
            b"\x1b\x03"
        );
        assert_eq!(
            encode_key(
                Key::Function(12),
                Modifiers {
                    shift: true,
                    ..Default::default()
                },
                false
            ),
            b"\x1b[24;2~"
        );
        assert_eq!(
            encode_key(Key::Character('界'), Modifiers::default(), false),
            "界".as_bytes()
        );
    }
    #[test]
    fn mouse_release_and_motion_respect_modes_and_local_override() {
        let modes = Modes {
            click: true,
            sgr: true,
            ..Default::default()
        };
        assert_eq!(
            encode_mouse(modes, MouseAction::Release(0), 9, 3, Modifiers::default()),
            b"\x1b[<0;10;4m"
        );
        assert!(encode_mouse(
            modes,
            MouseAction::Move(Some(0)),
            1,
            1,
            Modifiers::default()
        )
        .is_empty());
        assert!(encode_mouse(
            modes,
            MouseAction::Press(0),
            1,
            1,
            Modifiers {
                shift: true,
                ..Default::default()
            }
        )
        .is_empty());
        assert_eq!(
            encode_mouse(
                Modes {
                    motion: true,
                    ..modes
                },
                MouseAction::Move(None),
                0,
                0,
                Modifiers::default()
            ),
            b"\x1b[<35;1;1M"
        );
        assert!(encode_mouse(
            Modes {
                click: true,
                ..Default::default()
            },
            MouseAction::Press(0),
            300,
            1,
            Modifiers::default()
        )
        .is_empty());
    }
}
