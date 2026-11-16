# NOVA Native 0.5.7 — Stability, Security and Release Pipeline

## Dependency and CI reliability
- Added a dedicated lockfile preparation/verification script. Developer builds can create a missing lockfile once; CI requires a committed lockfile and never commits or pushes dependency changes.
- CI now runs on pushes, pull requests and manual dispatch with read-only repository permissions. Signing credentials are passed only to a dedicated post-build signing step on a push to `main`; Cargo and dependency build scripts never receive them. Pull requests and manual runs are packaged unsigned.
- Pinned the checkout, Rust toolchain and upload-artifact actions to immutable commits.
- Added Clippy, locked metadata, Windows-target tests and release-build checks.
- Added Dependabot updates for Cargo dependencies and GitHub Actions.

## Windows integration and security
- Added a Windows junction/reparse-point integration test proving that redirected data folders are rejected and safe recursive deletion does not delete the junction target.
- Added a Windows Job Object test proving that timeout termination also terminates a spawned descendant process.
- Added a concise security policy describing the actual host/Sandbox boundaries and a private vulnerability-reporting path.

## Portable release and performance
- Build version is read from `Cargo.toml` rather than duplicated in the packaging script.
- Portable packages now include build provenance, executable SHA-256, Cargo.lock SHA-256, Authenticode status and a checksum manifest.
- Added optional post-build Authenticode signing using a PFX provided through environment secrets; no certificate or password is stored in source. Signing is isolated from Cargo so dependency build scripts never inherit the certificate secrets.
- Added `scripts/finalize-portable.ps1` to refresh build metadata, executable checksums, ZIP and sidecar after signing.
- Removed the unnecessary `Cargo.lock.build-reference` copy from the runtime USB package.
- Added `scripts/measure-performance.ps1` to sample NOVA CPU, working set, private memory, handles and thread count to CSV on Windows.

## Validation caveats
- The source archive and static configuration can be checked in this environment, but Rust compilation, Clippy, unit tests, Windows integration tests, EXE signing and runtime performance sampling require a Windows machine with Rust 1.95.0 and the MSVC build tools.
- `Cargo.lock` is intentionally not fabricated: the current editing environment has no Cargo/Rust toolchain or network access to resolve the full transitive dependency graph. Run `scripts/prepare-lockfile.ps1` on Windows and commit the generated file before expecting CI to pass.

## Follow-up source audit

- Confirmed the pinned `dtolnay/rust-toolchain` revision hard-codes Rust 1.95.0 in its action metadata and does not declare a `toolchain` input. The workflow intentionally supplies only its supported `targets` and `components` inputs.
- Rechecked Cargo/TOML and workflow YAML, source delimiters, declared test count and repository file references. These are static checks, not a substitute for `cargo check`, Clippy, or Windows runtime tests.

## Compile fixes from Windows CI feedback

- Replaced deprecated `Ui::id()` with `Ui::scope_id()` and deprecated `egui::Id::new` calls with `egui::Id::unique` while preserving the same stable salts.
- Explicitly typed the shortcut icon font size as `f32` so `to_bits()` has an unambiguous receiver type.
- Converted shortcut icon text from `&str` to the owned `String` required by egui 0.37's `Painter::layout_no_wrap`.
- Compared the quick-access app name directly as `&str`, avoiding the unstable `str::as_str()` API.
- Added `pinned_apps` to the storage test's `Settings` initializer and asserted that pinned apps survive save/load.


## Additional desktop-shortcut compile fix

- Corrected the iterator predicate for desktop shortcut lookup: `.iter()` passes `&&str` to `find`, so the comparison now dereferences the name to `&str` (`app.name == *name`).
- Promoted the desktop shortcut list to a shared catalogue constant and added a test ensuring every desktop shortcut maps to a unique app-catalog entry.
