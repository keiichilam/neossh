# Clipboard acceptance reconciliation

Scope: the copy/paste case in
[the release matrix](../../docs/redevelopment/04-validation-and-release.md).
Evidence paths below are relative to fresh/. Native GUI runs use an owned Linux
Xvfb display; Windows CLI evidence is separate. A passing byte comparison does
not by itself establish visual behavior.

| Matrix requirement | Evidence and status |
| --- | --- |
| Exact normalized drag selection, Ctrl+C, clear highlight | Native v0.1.124 `linux-runtime-62dbaa9eda754a11b9d42b21b74bdc9b`: Unicode/wide-cell and wrapped-line round trips; reviewed copy-cleared screenshot. |
| Frozen selection after remote output, scroll history and resize | Native v0.1.131 `linux-runtime-61f54dc2e5104d4093643d92cd9e0ffb`: select 140-character wrapped line, replace output with known scrollback, native wheel up, shrink/restore window, then copy/paste exact original text. Reviewed screenshot shows HISTORY 14/15 and Back to live output. |
| Ctrl+C without selection sends remote interrupt | Raw-PTY fixture checks exactly 03; this proves the input byte. Native v0.1.135 `linux-runtime-c26b0ef13d5c44859016db929b599340` interrupts foreground sleep 60, verifies its child process stops and submits a subsequent shell command successfully. |
| Ctrl+Shift+C selection/no-selection, Escape cancels locally | Native Vim v0.1.119 `linux-runtime-5e1ae95d482046389db82218a360a8f0` verifies exact selected text with Ctrl+Shift+C. Raw-PTY fixture checks no-selection copy-only, Escape with no remote bytes, then Ctrl+C proving selection cleared. |
| Right-click single line; LF, CRLF, CR-only and trailing-newline review | Raw-PTY fixture checks all newline variants wait and cancellation sends nothing. Single-line Unicode/whitespace paste is exact; empty paste sends nothing. |
| Confirmed text exactly once; Ctrl+V same review policy; forced paste | Raw-PTY fixture confirms each newline variant via Ctrl+V and compares unchanged bytes; forced mixed-newline paste is exact. Native v0.1.124 additionally verifies ordinary multiline confirmation and right-click review. |
| Selection Copy / Paste / Search online | v0.1.124 verifies exact Copy and highlight clearing. v0.1.127 `linux-runtime-730dab74f0224bce9217633be3f07203` verifies exact Search online query, distinct clipboard Paste and immediate Enter. Unit menu regressions replace live output before the action to verify frozen text. |
| Real editor normal/forced bracketed paste | Native Vim/tmux v0.1.117 `linux-runtime-d29c51f9893a4c30bb729a91aaf14908`: reviewed/forced paste, exact saved Unicode/whitespace, resize and exit. v0.1.124 also compares exact bracket delimiters with a raw PTY and checks mode reset. |
| Windows CLI newline/Unicode/empty clipboard | `cli-clipboard-a8d06c80a02b4f6eb4bdff68e8ae53d6`: owned native console and clipboard, exact payloads, review cancellation/confirmation, output-only logs and console restoration. |

All evidence directories above are under `.qa/`. The raw-PTY fixture is
`linux_smoke.py --gui-text-clipboard`; its v0.1.130 passing result is
`.qa/linux-runtime-46dd9bd28c344a84a5597e4ab68f0543/results.json`. This reconciliation does
not claim macOS runtime or an exhaustive platform/feature cross-product.
