# NOVA Native 0.5.0 — build status

## Source-side checks

- Package version is `0.5.0`; artifact name is `NOVA-OS-Native-v0.5.0-USB`.
- 57 distinct unit-test functions are present in source; the Windows x64 MSVC target includes 50 tests after platform-specific `cfg` selection.
- Windows workflow uses pinned Rust 1.95.0 and x64 MSVC target.
- Workflow refreshes and commits `Cargo.lock` only when absent or when the root crate version changes; tests and release build use `--locked`.
- Packaging copies the UniKey download helper and pre-creates Data/Trash/Security folders.
- Unicode, UniKey, Chrome Sandbox and file-inspection Sandbox behavior are described in the source/docs. Startup validates Data directory components against symlink/junction/reparse-point redirection; Trash and Security staging revalidate their roots at use time. Permanent deletion and quarantine cleanup recurse without following nested symlinks or Windows reparse points.

## Validation limits

The packaging environment does not contain `rustc`, `cargo` or `rustfmt`. It cannot claim Rust compilation, `cargo test`, clippy or a Release EXE build passed. A successful GitHub Actions run on the exact 0.5.0 commit is required to verify compilation and tests. Real Windows runtime tests are still needed for UniKey signature/typing, Windows Sandbox edition and launch behavior, Chrome read-only mapping, USB persistence, graphics drivers, Wi-Fi and process cleanup.
