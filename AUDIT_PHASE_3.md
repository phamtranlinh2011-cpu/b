# NOVA Native v0.5.7 — Audit Phase 3: App Quality

## Scope

This source update continues from **NOVA-OS-Native-v0.5.7 Phase 2 — Stability**. It focuses on daily app workflows and user-visible correctness. No Cargo dependency or `Cargo.toml` changes were made.

## Implemented changes

### 1. Windows file clipboard interoperability

- Added `src/clipboard.rs` with native Windows `CF_HDROP` path-list interoperability and the `Preferred DropEffect` hint used by Explorer-style file operations.
- `Ctrl+C`, `Ctrl+X`, and `Ctrl+V` in NOVA Files now exchange file paths with Windows File Explorer and applications that support `CF_HDROP`.
- The clipboard contains file paths, not file contents. External paths pasted into NOVA are always **copied**; NOVA does not move or delete external source files even when the source clipboard advertises a Move operation.
- Imports reject top-level and nested symlinks/reparse points and special filesystem objects. Copy uses a staging directory, no-replace publishing, cancellation checks, and bounds of 64 directory levels and 50,000 visited entries per imported tree. The clipboard path list is bounded to 2,000 items and 2,000,000 UTF-16 path characters.
- If Windows clipboard access is unavailable, NOVA falls back to its in-app file clipboard. Failed/incomplete imports retain only the remaining external source paths in a retry queue; pasting the same clipboard selection again retries only those remaining items.

### 2. One-step Undo for Trash

- Added **Undo last delete** and `Ctrl+Z` in NOVA Files to restore the latest batch moved into NOVA Trash.
- Failed restores remain in Trash and are kept in the retry list. Normal per-item Trash Restore remains available.
- Undo is deliberately one-level and session-only. It is not a general Undo system for Rename, Copy, Move, or folder creation. Exiting NOVA clears the in-memory Undo stack; Trash entries remain restorable from the Trash app.

### 3. Editor external-change protection

- The Editor stores a bounded content fingerprint when opening a file and compares the current file contents before overwriting that same path.
- The check detects same-length edits, length changes, and a removed file. If a conflict is found, the Editor preserves the in-memory edits and blocks the save; the user can reopen the document or choose a different path with Save As semantics.
- The fingerprint uses FNV-1a plus content length as a conflict detector, not as a cryptographic integrity primitive. The check is a pre-save guard, not a transactional lock against every concurrent writer between comparison and replacement.

### 4. Actual Wi-Fi association status

- Added `WlanQueryInterface` querying `wlan_intf_opcode_current_connection` and reading the current `WLAN_CONNECTION_ATTRIBUTES` association and signal-quality fields.
- Connect/disconnect UI status now polls the current WLAN association after Windows accepts the command. It does not treat command submission alone as proof of a completed connection.
- The WLAN API-allocated result is released with `WlanFreeMemory`, and compile-time size/offset assertions pin the hand-written FFI layouts. Expected SSID matching is exact and case-sensitive.
- Microsoft documents that `wlan_intf_opcode_current_connection` returns `WLAN_CONNECTION_ATTRIBUTES`, that disconnected/disconnecting interfaces return `ERROR_INVALID_STATE`, and that callers free `ppData` using `WlanFreeMemory`:
  - https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlanqueryinterface
  - https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/ne-wlanapi-wlan_intf_opcode
  - https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/ns-wlanapi-wlan_connection_attributes

### 5. Calculator and Security Center regression coverage

- Added Calculator regression cases for invalid/non-finite distribution parameters, CDF bounds and monotonicity at boundaries, factorial/combinatorics overflow limits, and invalid Base-N digits/bases. No calculator algorithm behavior was changed in this audit phase.
- Refactored Security Center Authenticode result interpretation into a pure helper and added tests distinguishing unsigned OWASP ZAP (explicit one-session acknowledgement only), other unsigned tools, invalid/untrusted signatures, and unexpected publishers. The Windows PowerShell/Authenticode runtime path still requires Windows verification.

### 6. Documentation

- Updated README, Vietnamese guide, and release notes for CF_HDROP, copy-only external import, retry behavior, one-level Undo, Editor conflicts, and confirmed WLAN status.

## Explicitly deferred

- SMART/NVMe health status and physical-disk read/write throughput are not implemented. The Storage view continues to show the capacity/filesystem fields exposed by its existing provider; NOVA does not infer or fabricate a health percentage.
- Undo is only for the latest Trash batch in the current app session, not all filesystem operations.
- The Windows clipboard integration handles file-path clipboard format (`CF_HDROP`), not arbitrary text, images, or virtual-file clipboard formats.

## Static validation performed

- `Cargo.toml` parsed successfully and matches the Phase 2 baseline; no dependency changes.
- GitHub Actions workflow YAML parsed successfully.
- Hand-written WLAN FFI layouts were independently checked for expected sizes/offsets: association attributes 68 bytes, security attributes 16 bytes, connection attributes 604 bytes (association offset 520, security offset 588), and `DROPFILES` 20 bytes.
- Rust source lexer pass checked 16 `.rs` files for balanced delimiters and terminated strings/comments.
- No trailing whitespace was found in the edited Rust modules or user-facing Markdown files.
- The source declares 154 `#[test]` cases after this update. **These tests were counted, not executed.**
- ZIP integrity and SHA-256 are checked for the delivered archive.

## Build and runtime validation limitation

The editing environment does not contain `cargo`, `rustc`, `rustfmt`, or PowerShell, and has no Windows runtime. Therefore no Rust compile, Clippy, unit-test execution, Windows FFI execution, GUI smoke test, or real Wi-Fi/clipboard integration test is claimed. The authoritative validation remains the Windows GitHub Actions workflow and a real Windows run.

The source ZIP does not include `Cargo.lock`. Keep the committed repository `Cargo.lock` alongside this source and retain the workflow's `--locked` checks.
