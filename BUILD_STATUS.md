# NOVA Native 0.5.1 — build status

## Source-side checks

- Package version is `0.5.1`; artifact name is `NOVA-OS-Native-v0.5.1-USB`.
- 71 distinct `#[test]` functions are declared across the Rust source; target-specific `cfg` attributes determine which tests are compiled for Windows.
- Windows workflow uses pinned Rust 1.95.0 and x64 MSVC target.
- Workflow refreshes and commits `Cargo.lock` only when absent or when the root crate version changes; tests and release build use `--locked`.
- Packaging copies the UniKey download helper, records the five optional GUI tool integrations and pre-creates Data/Trash/Security folders.
- Unicode, UniKey, Chrome Sandbox and file-inspection Sandbox behavior are described in the source/docs. Startup validates Data directory components against symlink/junction/reparse-point redirection; storage, Trash and Security operations re-check each existing path component and revalidate their roots at use time. Settings writes reject redirected parent folders; Trash metadata is size-limited on write and restore; Sandbox staging uses an exclusive destination and bounded copy; quarantine cleanup preserves unrelated `.wsb` files. Permanent deletion and quarantine cleanup recurse without following nested symlinks or Windows reparse points.
- Authenticode checks for UniKey and host Chrome use the bounded process runner (15-second deadline, process-tree cleanup and capped output). Executable paths are passed as environment values, not interpolated PowerShell source. UniKey is launched from its own directory for portable relative-file resolution.
- CMD/WLAN process work uses the Job Object runner with bounded execution and retained output. Terminal password inputs are masked and regression-tested; UTF-8/UTF-16 decoding includes non-BMP Unicode; WLAN profile XML escaping includes Vietnamese SSIDs.

## Latest Windows CI feedback and source fix

The first user-provided Windows CI log compiled the test target successfully and ran 61 tests: 60 passed, 1 failed. The failing UniKey test compared a canonical Windows path (which can include `\\?\` and expanded path casing/short-name components) against the original path string. The regression assertion now compares canonical paths. The `scope_ready` unused-assignment warning was also removed.

The fixed source has not yet been rerun on Windows CI. A fresh `cargo test --locked --target x86_64-pc-windows-msvc` is required to confirm the path assertion now passes; then run the Release build job.

## Validation limits

The current packaging environment does not contain `rustc`, `cargo` or `rustfmt`. Static source review and regression-test additions are complete for all three audit passes, but compilation, `cargo test`, clippy, formatting and a Release EXE build have not been run here. A successful GitHub Actions run on the exact source revision is required to verify compilation and tests. Real Windows runtime tests are still needed for UniKey signature/typing, Windows Sandbox edition and launch behavior, Chrome read-only mapping, USB persistence, graphics drivers, Wi-Fi and process cleanup.
