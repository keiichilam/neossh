Based on egui-winit 0.28.1 from the Cargo registry.

Local patch: preserve Cut/Copy/Paste shortcut key-press events immediately
before the semantic clipboard event, including the modifiers at that press.
This lets the terminal distinguish Ctrl+Shift+C/V even when modifiers are
released before the next frame, and lets Ctrl+X (nano, emacs) and Shift+Delete
reach the terminal instead of being consumed as Cut. egui text fields ignore
these key presses (they act on the clipboard event), so other clipboard
behavior is unchanged.

Regression: clipboard_events_retain_shortcut_modifiers_after_release.
Upstream: https://github.com/emilk/egui/tree/0.28.1/crates/egui-winit
