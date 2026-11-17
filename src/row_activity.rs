//! Per-row "last changed" times for the Alt+hover tooltip (from ssh4).
//!
//! Rows are tracked by their position on the live screen, so output that
//! scrolls the whole screen re-stamps every row; in-place updates (dashboards,
//! progress lines, `top`, `watch`) re-stamp only the rows that changed.
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct RowActivity {
    rows: Vec<(u64, Instant)>,
}

fn row_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl RowActivity {
    /// Forget all rows (new connection or resize).
    pub fn reset(&mut self) {
        self.rows.clear();
    }

    /// Record `now` for every row whose text differs from the previous update.
    pub fn update<I, S>(&mut self, rows: I, now: Instant)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let hashes: Vec<u64> = rows.into_iter().map(|row| row_hash(row.as_ref())).collect();
        if hashes.len() != self.rows.len() {
            self.rows = hashes.into_iter().map(|hash| (hash, now)).collect();
            return;
        }
        for (slot, hash) in self.rows.iter_mut().zip(hashes) {
            if slot.0 != hash {
                *slot = (hash, now);
            }
        }
    }

    /// Update from the live (non-scrolled) screen of a vt100 parser.
    pub fn update_screen(&mut self, screen: &vt100::Screen, now: Instant) {
        let (_, columns) = screen.size();
        self.update(screen.rows(0, columns), now);
    }

    pub fn changed(&self, row: u16) -> Option<Instant> {
        self.rows.get(row as usize).map(|(_, at)| *at)
    }
}

/// Short human description such as "just now", "42 s ago" or "3 min ago".
pub fn describe_age(age: Duration) -> String {
    let seconds = age.as_secs();
    match seconds {
        0 => "just now".into(),
        1..=59 => format!("{seconds} s ago"),
        60..=3599 => format!("{} min ago", seconds / 60),
        _ => format!("{} h {} min ago", seconds / 3600, seconds % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changed_rows_are_restamped_and_resize_resets() {
        let start = Instant::now();
        let later = start + Duration::from_secs(5);
        let mut activity = RowActivity::default();
        activity.update(["a", "b", "c"], start);
        activity.update(["a", "B", "c"], later);
        assert_eq!(activity.changed(0), Some(start));
        assert_eq!(activity.changed(1), Some(later));
        assert_eq!(activity.changed(2), Some(start));
        assert_eq!(activity.changed(3), None);
        activity.update(["a", "B"], later + Duration::from_secs(1));
        assert_eq!(activity.changed(0), Some(later + Duration::from_secs(1)));
        activity.reset();
        assert_eq!(activity.changed(0), None);
    }

    #[test]
    fn tracks_vt100_rows_in_place() {
        let start = Instant::now();
        let mut parser = vt100::Parser::new(3, 10, 0);
        parser.process(b"top\r\nmid\r\nend");
        let mut activity = RowActivity::default();
        activity.update_screen(parser.screen(), start);
        parser.process(b"\x1b[2;1Hnew");
        let later = start + Duration::from_secs(2);
        activity.update_screen(parser.screen(), later);
        assert_eq!(activity.changed(0), Some(start));
        assert_eq!(activity.changed(1), Some(later));
        assert_eq!(activity.changed(2), Some(start));
    }

    #[test]
    fn ages_are_short_and_readable() {
        assert_eq!(describe_age(Duration::from_millis(400)), "just now");
        assert_eq!(describe_age(Duration::from_secs(42)), "42 s ago");
        assert_eq!(describe_age(Duration::from_secs(185)), "3 min ago");
        assert_eq!(describe_age(Duration::from_secs(3720)), "1 h 2 min ago");
    }
}
