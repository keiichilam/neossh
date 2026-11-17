//! Literal bracket matching over visible terminal cells, not language syntax.
pub type Position = (u16, u16);
pub type Pair = (Position, Position);

fn bracket(screen: &vt100::Screen, point: Position) -> Option<u8> {
    let value = screen.cell(point.0, point.1)?;
    if value.is_wide_continuation() {
        return None;
    }
    let text = value.contents();
    let bytes = text.as_bytes();
    (bytes.len() == 1 && b"()[]{}".contains(&bytes[0])).then(|| bytes[0])
}
fn partner(value: u8) -> u8 {
    match value {
        b'(' => b')',
        b')' => b'(',
        b'[' => b']',
        b']' => b'[',
        b'{' => b'}',
        b'}' => b'{',
        _ => unreachable!(),
    }
}
pub fn at(screen: &vt100::Screen, point: Position) -> Option<Pair> {
    let initial = bracket(screen, point)?;
    let forward = b"([{".contains(&initial);
    let (rows, columns) = screen.size();
    let columns = usize::from(columns);
    if columns == 0 {
        return None;
    }
    let mut index = usize::from(point.0) * columns + usize::from(point.1);
    let total = usize::from(rows) * columns;
    let mut expected = vec![partner(initial)];
    for _ in 0..8192 {
        index = if forward {
            index.checked_add(1)?
        } else {
            index.checked_sub(1)?
        };
        if index >= total {
            return None;
        }
        let next = ((index / columns) as u16, (index % columns) as u16);
        let Some(value) = bracket(screen, next) else {
            continue;
        };
        if b"([{".contains(&value) == forward {
            if expected.len() >= 1024 {
                return None;
            }
            expected.push(partner(value));
        } else if expected.pop()? != value {
            return None;
        } else if expected.is_empty() {
            return Some(if forward {
                (point, next)
            } else {
                (next, point)
            });
        }
    }
    None
}

/// Prefer the cursor cell, then the cell immediately before an insertion cursor.
pub fn near_cursor(screen: &vt100::Screen) -> Option<Pair> {
    let point = screen.cursor_position();
    if bracket(screen, point).is_some() {
        return at(screen, point);
    }
    if point.1 > 0 {
        return at(screen, (point.0, point.1 - 1));
    }
    if point.0 > 0 && screen.row_wrapped(point.0 - 1) {
        return at(screen, (point.0 - 1, screen.size().1.checked_sub(1)?));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_pairs_use_cell_coordinates_across_wide_text_and_rows() {
        let mut parser = vt100::Parser::new(5, 12, 0);
        parser.process("界({[ab]\r\ncd})".as_bytes());
        let screen = parser.screen();
        assert_eq!(at(screen, (0, 2)), Some(((0, 2), (1, 3))));
        assert_eq!(at(screen, (1, 3)), Some(((0, 2), (1, 3))));
        assert_eq!(at(screen, (0, 4)), Some(((0, 4), (0, 7))));
        assert_eq!(near_cursor(screen), Some(((0, 2), (1, 3))));
        assert!(at(screen, (0, 1)).is_none());
    }
    #[test]
    fn mismatches_unclosed_pairs_and_work_limits_do_not_produce_false_matches() {
        let mut parser = vt100::Parser::new(5, 20, 0);
        parser.process(b"([)]\r\n{unterminated");
        for point in [(0, 0), (0, 1), (0, 2), (0, 3), (1, 0)] {
            assert!(at(parser.screen(), point).is_none());
        }
        let mut parser = vt100::Parser::new(120, 80, 0);
        parser.process(format!("({})", "a".repeat(8500)).as_bytes());
        assert!(at(parser.screen(), (0, 0)).is_none());
    }
}
