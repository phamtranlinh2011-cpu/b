# NOVA Native v0.5.7 — Borrow/Ownership Fix Update

## Changes

1. **E0382 — Wi-Fi SSID moved into worker**
   - Clone the selected SSID into `worker_ssid` before `thread::spawn(move || ...)`.
   - The worker uses `worker_ssid`; the original `ssid` remains available for the immediate UI status message.

2. **E0500 — File Manager window borrow overlaps `self` access**
   - Copy `self.files_open` into a local `files_open` variable.
   - Pass the local variable to `Window::open`, then copy the resulting open state back to `self.files_open` after `show` returns.

3. **Defensive window-state cleanup**
   - Applied the same local-open-state pattern to UniKey, Terminal, Settings, Trash, Calculator and Hardware Center.
   - These were precautionary borrow-safety cleanups; they were not reported as compiler errors in the supplied CI log.

4. **CI source invariant check**
   - Added `scripts/verify-source-invariants.ps1` and wired it into GitHub Actions before formatting/lint/build.
   - It checks the Wi-Fi and File Manager fixes and rejects direct `.open(&mut self.<field>)` patterns.

## Checks completed in this environment

- Static assertions for both reported ownership/borrow fixes: passed.
- Static assertion that no direct `.open(&mut self.<field>)` pattern remains in `src/app.rs`: passed.
- Local open-state copy-back checks for six additional windows: passed.
- `Cargo.toml` TOML parse: passed.
- GitHub Actions workflow YAML parse: passed.
- ZIP integrity check: performed after packaging.
- 154 Rust test attributes were found by source scan; tests were not executed.

## Not verified

This environment does not contain `cargo`, `rustc`, `rustfmt` or PowerShell. Therefore Rust formatting, compilation, Clippy, unit tests, PowerShell parsing, Windows build and executable startup were not run. The Windows CI result is still required before claiming a successful build.

## Cargo.lock

This source package does not include `Cargo.lock`. Preserve the existing repository lockfile when copying the source into GitHub. If the repository truly has no lockfile, generate it with the project's `scripts/prepare-lockfile.ps1` on Windows using the pinned Rust toolchain, then commit it before CI.
