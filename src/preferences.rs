use crate::config::{Config, Profile};

/// Default and bounds for the scrollback length (lines kept above the screen).
pub const DEFAULT_SCROLLBACK_LINES: u32 = 10_000;
pub const MIN_SCROLLBACK_LINES: u32 = 100;
/// A vt100 cell takes about 56 bytes, so 50,000 lines of 200 columns already
/// hold roughly 560 MB per tab.
pub const MAX_SCROLLBACK_LINES: u32 = 50_000;

fn scrollback_value(value: &toml::Value) -> Option<u32> {
    value
        .as_integer()
        .map(|lines| lines.clamp(MIN_SCROLLBACK_LINES as i64, MAX_SCROLLBACK_LINES as i64) as u32)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    pub zoom: f32,
    pub font_size: f32,
    pub theme: String,
    pub keep_alive: bool,
    pub scrollback: bool,
    /// `scrollback_lines` (ssh4): how many lines to keep when scrollback is on.
    pub scrollback_lines: u32,
}
impl Preferences {
    pub fn load(config: &Config) -> Self {
        let number = |key, default, min, max| {
            config
                .get(key)
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|v| v as f64)))
                .filter(|v| v.is_finite())
                .unwrap_or(default)
                .clamp(min, max) as f32
        };
        Self {
            zoom: number("ui_zoom", 1.0, 0.75, 2.0),
            font_size: number("terminal_font_size", 15.0, 10.0, 28.0),
            theme: config
                .get("theme")
                .and_then(toml::Value::as_str)
                .unwrap_or("Midnight")
                .into(),
            keep_alive: config
                .get("keep_alive")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
            scrollback: config
                .get("terminal_scrollback")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            scrollback_lines: config
                .get("scrollback_lines")
                .and_then(scrollback_value)
                .unwrap_or(DEFAULT_SCROLLBACK_LINES),
        }
    }

    /// Scrollback length for a session: 0 when scrollback is off, otherwise the
    /// profile's own `scrollback_lines` (ssh4 profiles) or the global setting.
    pub fn history_lines(&self, profile: Option<&Profile>) -> usize {
        if !self.scrollback {
            return 0;
        }
        profile
            .and_then(|profile| profile.extensions.get("scrollback_lines"))
            .and_then(scrollback_value)
            .unwrap_or(self.scrollback_lines) as usize
    }
    pub fn apply(&self, config: &mut Config) {
        config.set("ui_zoom", (self.zoom as f64).into());
        config.set("terminal_font_size", (self.font_size as f64).into());
        config.set("theme", self.theme.clone().into());
        config.set("keep_alive", self.keep_alive.into());
        config.set("terminal_scrollback", self.scrollback.into());
        config.set("scrollback_lines", i64::from(self.scrollback_lines).into());
    }
}

/// Terminal themes offered in Preferences, in picker order. Midnight, Forest and
/// Amber come from ssh5, Slate from ssh4gui, and the Phosphor/Paper themes from ssh4.
pub const THEMES: [&str; 7] = [
    "Midnight",
    "Forest",
    "Amber",
    "Slate",
    "Amber Phosphor",
    "Green Phosphor",
    "Paper",
];

/// The built-in theme name matching `name` (ignoring ASCII case), if any.
pub fn known_theme(name: &str) -> Option<&'static str> {
    THEMES
        .iter()
        .copied()
        .find(|theme| theme.eq_ignore_ascii_case(name.trim()))
}

/// Shared 16-colour palette for the dark themes that do not define their own.
const DARK_ANSI: [[u8; 3]; 16] = [
    [25, 32, 43],
    [235, 111, 117],
    [130, 205, 151],
    [233, 195, 119],
    [117, 167, 236],
    [189, 151, 222],
    [100, 203, 205],
    [204, 214, 226],
    [105, 121, 141],
    [250, 144, 147],
    [158, 228, 174],
    [248, 218, 149],
    [153, 191, 251],
    [215, 179, 247],
    [137, 225, 226],
    [244, 247, 251],
];

const AMBER_PHOSPHOR_ANSI: [[u8; 3]; 16] = [
    [0x21, 0x1c, 0x16],
    [0xe3, 0x59, 0x4f],
    [0x9b, 0xb9, 0x53],
    [0xe5, 0xb5, 0x66],
    [0x6e, 0x9c, 0xc6],
    [0xc5, 0x85, 0xc2],
    [0x6c, 0xb5, 0xa8],
    [0xd8, 0xcf, 0xc0],
    [0x6e, 0x65, 0x57],
    [0xf2, 0x77, 0x6d],
    [0xb2, 0xcc, 0x6e],
    [0xf5, 0xcc, 0x80],
    [0x8c, 0xb5, 0xd9],
    [0xd9, 0xa0, 0xd4],
    [0x87, 0xcc, 0xbf],
    [0xf2, 0xea, 0xd9],
];

const GREEN_PHOSPHOR_ANSI: [[u8; 3]; 16] = [
    [0x16, 0x1f, 0x17],
    [0xe3, 0x59, 0x4f],
    [0x5f, 0xc7, 0x5f],
    [0xd6, 0xc9, 0x5e],
    [0x66, 0x9c, 0xce],
    [0xc0, 0x84, 0xc4],
    [0x5e, 0xc1, 0xae],
    [0xc8, 0xd6, 0xc8],
    [0x5c, 0x6e, 0x5d],
    [0xf2, 0x77, 0x6d],
    [0x86, 0xe8, 0x86],
    [0xe8, 0xdd, 0x82],
    [0x8a, 0xb8, 0xe0],
    [0xd8, 0xa4, 0xda],
    [0x84, 0xd8, 0xc8],
    [0xe4, 0xf0, 0xe4],
];

/// Ink colours that stay readable on a light background.
const PAPER_ANSI: [[u8; 3]; 16] = [
    [0x2c, 0x28, 0x20],
    [0xb3, 0x2d, 0x23],
    [0x4a, 0x77, 0x1f],
    [0x9a, 0x6e, 0x00],
    [0x1f, 0x5c, 0x99],
    [0x8f, 0x3f, 0x8c],
    [0x1c, 0x7a, 0x6e],
    [0x8a, 0x82, 0x70],
    [0x5a, 0x54, 0x48],
    [0xd4, 0x4d, 0x42],
    [0x63, 0x96, 0x33],
    [0xb8, 0x88, 0x10],
    [0x3a, 0x7c, 0xc0],
    [0xb0, 0x5c, 0xac],
    [0x32, 0x99, 0x8c],
    [0x6b, 0x64, 0x57],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub cursor: [u8; 3],
    /// Colours for SGR indexes 0-15; higher indexes use the standard cube/ramp.
    pub ansi: [[u8; 3]; 16],
}
impl Colors {
    pub fn for_theme(theme: &str) -> Self {
        let colors = |foreground, background, cursor, ansi| Self {
            foreground,
            background,
            cursor,
            ansi,
        };
        match known_theme(theme).unwrap_or("Midnight") {
            "Forest" => colors([220, 235, 217], [15, 29, 24], [151, 219, 166], DARK_ANSI),
            "Amber" => colors([242, 216, 172], [30, 24, 18], [250, 195, 105], DARK_ANSI),
            "Slate" => colors([210, 216, 224], [16, 18, 24], [120, 160, 220], DARK_ANSI),
            "Amber Phosphor" => colors(
                [0xdd, 0xd4, 0xc0],
                [0x0f, 0x0d, 0x09],
                [0xff, 0xb0, 0x00],
                AMBER_PHOSPHOR_ANSI,
            ),
            "Green Phosphor" => colors(
                [0xc4, 0xdc, 0xc4],
                [0x09, 0x0e, 0x0a],
                [0x3f, 0xff, 0x6e],
                GREEN_PHOSPHOR_ANSI,
            ),
            "Paper" => colors(
                [0x2c, 0x28, 0x20],
                [0xfa, 0xf6, 0xec],
                [0xa6, 0x5c, 0x00],
                PAPER_ANSI,
            ),
            _ => colors([218, 227, 237], [13, 19, 28], [83, 210, 185], DARK_ANSI),
        }
    }

    /// RGB for an indexed (SGR 38;5 / OSC 4) colour: the theme's 16 ANSI colours,
    /// then the standard 6x6x6 cube and 24-step grey ramp.
    pub fn indexed(&self, index: u8) -> [u8; 3] {
        if index < 16 {
            self.ansi[index as usize]
        } else if index >= 232 {
            [8 + (index - 232) * 10; 3]
        } else {
            let value = index - 16;
            let channel = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [
                channel(value / 36),
                channel(value / 6 % 6),
                channel(value % 6),
            ]
        }
    }

    /// True for themes with a light background (used for contrast decisions).
    pub fn is_light(&self) -> bool {
        let [r, g, b] = self.background;
        (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000 > 128
    }
}
/// WCAG relative luminance of an sRGB colour in 0..1.
fn relative_luminance([r, g, b]: [u8; 3]) -> f32 {
    let linear = |channel: u8| {
        let s = f32::from(channel) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// WCAG contrast ratio of two opaque colours (1..21).
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Readable-text guard from ssh4gui: when the remote (or a highlight such as
/// selection or search) puts text on a background of nearly the same
/// luminance, switch to near-black or near-white ink. Deliberately dim colours
/// that still have a modest contrast edge are left alone.
pub fn readable_foreground(foreground: [u8; 3], background: [u8; 3]) -> [u8; 3] {
    const COMFORTABLE: f32 = 3.0;
    const INVISIBLE: f32 = 2.0;
    const NEAR_LUMINANCE: f32 = 0.08;
    let ratio = contrast_ratio(foreground, background);
    if ratio >= COMFORTABLE {
        return foreground;
    }
    if ratio >= INVISIBLE
        && (relative_luminance(foreground) - relative_luminance(background)).abs() > NEAR_LUMINANCE
    {
        return foreground;
    }
    let (dark, light) = ([18, 18, 20], [242, 244, 248]);
    if contrast_ratio(dark, background) >= contrast_ratio(light, background) {
        dark
    } else {
        light
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self::for_theme("Midnight")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_legacy_integer_sizes_and_preserves_unknown_config_and_theme() {
        let mut config = Config::parse(
            "terminal_font_size=19\nui_zoom=9.0\ntheme='Custom'\nfuture_option='keep'\n",
        )
        .unwrap();
        let mut prefs = Preferences::load(&config);
        assert_eq!(prefs.font_size, 19.0);
        assert_eq!(prefs.zoom, 2.0);
        prefs.font_size = 17.0;
        prefs.apply(&mut config);
        assert_eq!(Preferences::load(&config), prefs);
        assert_eq!(config.get("theme").unwrap().as_str(), Some("Custom"));
        assert_eq!(config.get("future_option").unwrap().as_str(), Some("keep"));
    }

    #[test]
    fn scrollback_lines_come_from_profile_then_global_and_respect_the_switch() {
        let config = Config::parse(
            "scrollback_lines = 2000
[profiles.small]
host='a'
scrollback_lines = 500
[profiles.plain]
host='b'
",
        )
        .unwrap();
        let profiles = config.profiles().unwrap();
        let mut prefs = Preferences::load(&config);
        assert_eq!(prefs.history_lines(None), 2000);
        assert_eq!(prefs.history_lines(Some(&profiles["small"])), 500);
        assert_eq!(prefs.history_lines(Some(&profiles["plain"])), 2000);
        prefs.scrollback = false;
        assert_eq!(prefs.history_lines(Some(&profiles["small"])), 0);
        let defaults = Preferences::load(
            &Config::parse(
                "scrollback_lines = 9
",
            )
            .unwrap(),
        );
        assert_eq!(defaults.scrollback_lines, MIN_SCROLLBACK_LINES);
        assert_eq!(
            Preferences::load(&Config::default()).history_lines(None),
            DEFAULT_SCROLLBACK_LINES as usize
        );
    }

    #[test]
    fn every_theme_from_all_three_apps_resolves_with_distinct_colors() {
        let mut seen = std::collections::BTreeSet::new();
        for theme in THEMES {
            assert_eq!(known_theme(&theme.to_ascii_lowercase()), Some(theme));
            assert!(seen.insert(Colors::for_theme(theme).background), "{theme}");
        }
        assert_eq!(known_theme("Custom"), None);
        assert_eq!(Colors::for_theme("Custom"), Colors::for_theme("Midnight"));
        assert!(Colors::for_theme("Paper").is_light());
        assert!(!Colors::for_theme("Green Phosphor").is_light());
        assert_ne!(
            Colors::for_theme("Paper").ansi,
            Colors::for_theme("Slate").ansi
        );
    }

    #[test]
    fn readable_foreground_rescues_invisible_text_and_keeps_intentional_colors() {
        // White on white and black on black become readable ink.
        assert_eq!(
            readable_foreground([250, 250, 250], [255, 255, 255]),
            [18, 18, 20]
        );
        assert_eq!(readable_foreground([5, 5, 5], [0, 0, 0]), [242, 244, 248]);
        // Paper's dark ink on the dark selection highlight flips to light ink.
        let fixed = readable_foreground([0x2c, 0x28, 0x20], [39, 74, 91]);
        assert!(contrast_ratio(fixed, [39, 74, 91]) >= 3.0);
        // Normal theme text and soft greys with a visible edge are untouched.
        assert_eq!(
            readable_foreground([218, 227, 237], [13, 19, 28]),
            [218, 227, 237]
        );
        assert_eq!(
            readable_foreground([105, 121, 141], [13, 19, 28]),
            [105, 121, 141]
        );
    }
}
