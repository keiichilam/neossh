//! Literal search on an immutable capture of terminal cells and scrollback.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

type Position = (usize, u16);
const MAX_MATCHES: usize = 10_000;

pub struct SearchResults {
    pub screen: vt100::Screen,
    pub matches: Vec<(Position, Position)>,
    pub active: usize,
    pub truncated: bool,
    history: usize,
    pub origin: (u16, u16),
    viewport: (u16, u16),
}

impl SearchResults {
    pub fn navigate(&mut self, forward: bool) {
        if self.matches.is_empty() {
            return;
        }
        self.active = if forward {
            (self.active + 1) % self.matches.len()
        } else {
            (self.active + self.matches.len() - 1) % self.matches.len()
        };
        self.reveal();
    }

    fn reveal(&mut self) {
        if let Some((start, _)) = self.matches.get(self.active) {
            self.screen
                .set_scrollback(self.history.saturating_sub(start.0));
        }
        self.position_view();
    }

    pub fn viewport(&mut self, rows: u16, columns: u16) {
        if self.viewport != (rows, columns) {
            self.viewport = (rows, columns);
            self.position_view();
        }
    }

    fn position_view(&mut self) {
        let (rows, columns) = self.screen.size();
        self.origin = (0, 0);
        if let Some((start, _)) = self.matches.get(self.active) {
            let row = start
                .0
                .saturating_sub(self.history - self.screen.scrollback())
                as u16;
            self.origin.0 = row
                .saturating_sub(self.viewport.0 / 2)
                .min(rows.saturating_sub(self.viewport.0));
            self.origin.1 = start
                .1
                .saturating_sub(self.viewport.1 / 2)
                .min(columns.saturating_sub(self.viewport.1));
        }
    }

    /// None, another match, or the active match, in displayed cell coordinates.
    pub fn highlight(&self, row: u16, column: u16) -> Option<bool> {
        let point = (
            self.history - self.screen.scrollback() + usize::from(row),
            column,
        );
        let index = self.matches.partition_point(|(_, end)| *end < point);
        self.matches
            .get(index)
            .filter(|(start, end)| *start <= point && point <= *end)
            .map(|_| index == self.active)
    }
}

pub struct SearchJob {
    receiver: mpsc::Receiver<SearchResults>,
    cancel: Arc<AtomicBool>,
}

impl SearchJob {
    pub fn start(screen: vt100::Screen, query: String, case_sensitive: bool) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::spawn(move || {
            if let Some(results) = search(screen, &query, case_sensitive, &stop) {
                let _ = sender.send(results);
            }
        });
        Self { receiver, cancel }
    }

    pub fn poll(&self) -> Option<SearchResults> {
        self.receiver.try_recv().ok()
    }
}

impl Drop for SearchJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn search(
    mut screen: vt100::Screen,
    query: &str,
    case_sensitive: bool,
    cancel: &AtomicBool,
) -> Option<SearchResults> {
    let original_offset = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let (rows, columns) = screen.size();
    let query = if case_sensitive {
        query.to_owned()
    } else {
        query.to_lowercase()
    };
    let mut results = SearchResults {
        screen,
        matches: Vec::new(),
        active: 0,
        truncated: false,
        history,
        origin: (0, 0),
        viewport: (rows, columns),
    };
    let mut text = String::new();
    let mut positions = Vec::new();
    // Search logical lines, including soft wraps, without flattening history
    // into an unbounded second terminal buffer.
    for absolute_row in 0..history + usize::from(rows) {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        results
            .screen
            .set_scrollback(history.saturating_sub(absolute_row));
        let row = absolute_row.saturating_sub(history) as u16;
        for column in 0..columns {
            let cell = results.screen.cell(row, column).unwrap();
            if cell.is_wide_continuation() {
                continue;
            }
            let contents = cell.contents();
            let contents = if contents.is_empty() {
                " ".to_owned()
            } else if case_sensitive {
                contents
            } else {
                contents.to_lowercase()
            };
            let end = column + u16::from(cell.is_wide());
            positions.extend(std::iter::repeat_n(
                ((absolute_row, column), (absolute_row, end)),
                contents.len(),
            ));
            text.push_str(&contents);
        }
        if !results.screen.row_wrapped(row) || absolute_row + 1 == history + usize::from(rows) {
            if !query.is_empty() {
                for (start, matched) in text.trim_end().match_indices(&query) {
                    if results.matches.len() == MAX_MATCHES {
                        results.truncated = true;
                        break;
                    }
                    results
                        .matches
                        .push((positions[start].0, positions[start + matched.len() - 1].1));
                }
            }
            text.clear();
            positions.clear();
        }
        if results.truncated {
            break;
        }
    }
    results.screen.set_scrollback(original_offset);
    results.active = results.matches.len().saturating_sub(1);
    results.reveal();
    Some(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn searches_history_soft_wraps_unicode_and_keeps_original_screen_unchanged() {
        let mut parser = vt100::Parser::new(2, 6, 20);
        parser.process("Old界\r\nabcdefGH\r\nlatest".as_bytes());
        let screen = parser.screen().clone();
        let cancel = AtomicBool::new(false);
        let old = search(screen.clone(), "OLD界", false, &cancel).unwrap();
        assert_eq!(old.matches.len(), 1);
        assert_eq!(old.highlight(0, 4), Some(true));
        assert!(old.screen.scrollback() > 0);
        let wrapped = search(screen.clone(), "efGH", true, &cancel).unwrap();
        assert_eq!(wrapped.matches.len(), 1);
        assert!(wrapped.matches[0].0 .0 < wrapped.matches[0].1 .0);
        assert_eq!(screen.scrollback(), 0);
        assert!(search(screen.clone(), "OLD", true, &cancel)
            .unwrap()
            .matches
            .is_empty());
        cancel.store(true, Ordering::Relaxed);
        assert!(search(screen, "latest", true, &cancel).is_none());
    }

    #[test]
    fn navigation_wraps_and_empty_queries_have_no_matches() {
        let mut parser = vt100::Parser::new(2, 20, 10);
        parser.process(b"one one one");
        let mut results = search(
            parser.screen().clone(),
            "one",
            true,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(results.active, 2);
        results.navigate(true);
        assert_eq!(results.active, 0);
        results.navigate(false);
        assert_eq!(results.active, 2);
        assert!(
            search(parser.screen().clone(), "", true, &AtomicBool::new(false))
                .unwrap()
                .matches
                .is_empty()
        );
    }

    #[test]
    fn smaller_viewports_reveal_matches_and_match_counts_are_bounded() {
        let mut parser = vt100::Parser::new(4, 20, 10);
        parser.process(b"\x1b[4;18Hhit");
        let mut results = search(
            parser.screen().clone(),
            "hit",
            true,
            &AtomicBool::new(false),
        )
        .unwrap();
        results.viewport(2, 8);
        let start = results.matches[0].0;
        assert!(
            start.0 >= usize::from(results.origin.0) && start.0 < usize::from(results.origin.0 + 2)
        );
        assert!(start.1 >= results.origin.1 && start.1 < results.origin.1 + 8);
        assert_eq!(results.highlight(start.0 as u16, start.1), Some(true));
        let mut parser = vt100::Parser::new(2, 100, 150);
        parser.process("x".repeat(12_000).as_bytes());
        let results = search(parser.screen().clone(), "x", true, &AtomicBool::new(false)).unwrap();
        assert_eq!(results.matches.len(), MAX_MATCHES);
        assert!(results.truncated);
    }
}
