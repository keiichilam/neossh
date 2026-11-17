#[test]
fn shrinking_keeps_cursor_output_and_moves_removed_top_rows_to_history() {
    let mut parser = vt100::Parser::new(6, 12, 10);
    parser.process(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nprompt$\x1b7");
    parser.set_size(3, 12);
    assert_eq!(parser.screen().contents(), "four\nfive\nprompt$");
    assert_eq!(parser.screen().cursor_position(), (2, 7));
    parser.process(b"\x1b[H\x1b8!");
    assert_eq!(parser.screen().contents(), "four\nfive\nprompt$!");
    parser.set_scrollback(3);
    assert_eq!(parser.screen().contents(), "one\ntwo\nthree");
}

#[test]
fn shrinking_removes_bottom_rows_first_when_cursor_already_fits() {
    let mut parser = vt100::Parser::new(6, 12, 10);
    parser.process(b"one\r\nprompt$");
    parser.set_size(3, 12);
    assert_eq!(parser.screen().contents(), "one\nprompt$");
    assert_eq!(parser.screen().cursor_position(), (1, 7));
    parser.set_scrollback(10);
    assert_eq!(parser.screen().scrollback(), 0);
}

#[test]
fn alternate_screen_and_disabled_history_keep_the_live_prompt() {
    for alternate in [false, true] {
        let mut parser = vt100::Parser::new(4, 12, 0);
        if alternate {
            parser.process(b"\x1b[?1049h");
        }
        parser.process(b"one\r\ntwo\r\nthree\r\nprompt$");
        parser.set_size(2, 12);
        assert_eq!(parser.screen().contents(), "three\nprompt$");
        assert_eq!(parser.screen().cursor_position(), (1, 7));
        assert_eq!(parser.screen().alternate_screen(), alternate);
    }
}

#[test]
fn saved_cursor_is_clamped_even_when_live_cursor_needs_no_scroll() {
    let mut parser = vt100::Parser::new(6, 12, 10);
    parser.process(b"\x1b[6;12H\x1b7\x1b[H");
    parser.set_size(3, 4);
    parser.process(b"\x1b8X");
    assert_eq!(parser.screen().cell(2, 3).unwrap().contents(), "X");
}
