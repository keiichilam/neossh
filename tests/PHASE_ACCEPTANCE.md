# Connection-phase implementation - v0.1.144

The SSH-008 audit found combined progress labels that did not prove individual
operation boundaries. The application now records configuration resolution and
uses the vendored SSH library's scoped observer for version negotiation, initial
key exchange, authentication, PTY request and shell request. Host-key verification
is recorded by the actual verifier, including known hosts and later rekeys.

Observer callbacks are thread-local, metadata-only and restored after nested
scopes or unwinding. They are installed around initial connect and final shell
startup, so upstream rekeys do not acquire a fabricated initial-connect label.
Peer labels identify the relevant hop. Existing routing failures retain their
route-step context, and cancellation-aware event delivery remains bounded.

PTY/shell requests currently set want-reply=false. The final label therefore says
"Shell open request sent (no acknowledgement requested)"; it does not claim the
server accepted the request. No credential, environment value or terminal input
is passed through the library phase enum.

session_probe asserts the ordered subsequence version/key exchange/host verify/
authentication/PTY/shell request/shell request sent on successful connections. It
also rejects terminal-startup labels after selected trust/auth/hop setup failures.
The native responsiveness fixture captures the expanded connection-history view
before resuming continuous output and busy-tab closure. Final evidence is recorded
in PROGRESS.md and RELEASE_ACCEPTANCE.md after the runs complete.

Verification completed:

- Full app suite `.qa/validation-a02a2223d8f844f4b1011038c2b29df1/results.json`:
  12 gates, 149 app tests, one backend test, 48 config comparisons and 101 SSH
  scenarios, with the new ordered-phase/failure assertions enabled.
- Library unit tests `.qa/phase-library-77ca53e6f4a8468a81c321273fc1fb30/results.json`:
  32 passed. Upstream host-dependent algorithm integration tests were not run by
  --lib; the owned app SSH suite provides live phase-path coverage.
- Optimized Linux link `.qa/linux-cross-c6659745a1014b1fbbd254bc1e20fdd3/results.json`.
- Native `.qa/linux-runtime-9aa7169b2899444ea890cc6904c089bc/results.json`: history
  opens and scrolls; inspected top capture shows configuration resolution and
  route identity. Inspected tail capture from the preceding passing run
  `.qa/linux-runtime-5b53feb6c04f4ece817fda1251f95396/results.json` shows host verify,
  authentication, PTY and shell labels. Middle entries are established by source
  rendering and ordered integration assertions, not claimed as visible in these
  top/tail captures. Both runs pass output drain, interrupt/recovery and cleanup.

The specified SSH-008 phase gap is resolved. This does not close unrelated
release-matrix, final audit or native macOS verification work.
