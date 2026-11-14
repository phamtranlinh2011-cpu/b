# NOVA Native 0.3.2 — build status

## Delivered in this source package
- Rust native GUI using eframe/egui and the Glow renderer.
- Portable data path using `portable.flag` and `Data/` beside NOVA.exe.
- Built-in Files, Editor, Calculator, Settings, System Monitor and Windows CMD terminal.
- Wi-Fi scan/profile/connect/delete commands using `netsh wlan`.
- Windows GitHub Actions workflow at `.github/workflows/windows-build.yml`.
- Static MSVC C runtime configuration restored in `.cargo/config.toml`.

## 0.3.2 fixes
- Fixed the three compile errors reported by the Windows runner: `sysinfo::System::new()` is handled as a `Result`, egui style mutation uses `Context::all_styles_mut`, and Terminal password masking is computed before mutably borrowing the input field.
- Replaced deprecated `Ui::id()` with `Ui::scope_id()`.
- System Monitor now degrades gracefully if `sysinfo` initialization fails instead of panicking during app startup.
- Files validates Windows-invalid/reserved names; Editor creates missing directories one component at a time and canonicalizes each component before writing.
- Corrupt/unreadable Wi-Fi manifest files now produce an error and fail closed. A failed manifest write after importing a WLAN profile triggers a best-effort profile rollback.
- Temporary WLAN profile files are created with exclusive `create_new` and collision retries; cleanup remains best-effort on normal exit only.
- CMD polling errors now trigger process termination attempts; output pipe draining is bounded to avoid an indefinite UI hang; reader-thread panics are surfaced; truncated command output is reported.
- Calculator formatting no longer rounds small finite values to zero.
- Added tests for path validation, nested directory creation, manifest corruption, WLAN temp cleanup, SSID/password validation and calculator formatting.

## Validation status
- ZIP integrity, TOML parsing, YAML structure and source-level checks can be run in this environment.
- Rust compiler/Cargo are not installed in this container, so `cargo test`, `cargo build`, rustfmt and clippy have not been run here.
- The supplied repository did not include `Cargo.lock`; Cargo will resolve compatible transitive dependency versions during each clean CI run. Commit the generated lockfile after the first successful Windows build if repeatable dependency resolution is required.
- Windows CMD process-tree behavior, actual `netsh wlan` operations, graphics compatibility and USB persistence still require real Windows testing.

## Important
This is source code, not a precompiled EXE. GitHub Actions must pass on a Windows runner before treating the package as build-verified. NOVA is a Windows desktop environment, not a separate OS kernel or bootable Live USB.
