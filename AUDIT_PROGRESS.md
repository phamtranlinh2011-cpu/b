# NOVA Native 0.5.1 — Three-Part Source Audit

This document tracks one continuous audit of the NOVA source tree. The three parts are review passes over the same project, not separate source packages or separate programs.

## Part 1 — Application shell and GUI security integration

Reviewed and fixed in the current source tree:

- `src/app.rs` — launcher, Security Center and tool windows, including the reported egui borrow issues.
- `src/apps.rs` — Chrome discovery, URL validation and launch integration.
- `src/security_tools.rs` — five external GUI tool integrations, Greenbone URL handling and authorization acknowledgement.

Changes include stricter Greenbone URL validation, regression tests for malformed URLs and explicit wording that the authorized-scope acknowledgement is a workflow reminder, not a firewall or enforcement boundary.

## Part 2 — Storage, Trash and Sandbox staging

Reviewed and updated in this pass:

- `src/storage.rs`
- `src/trash.rs`
- `src/isolation.rs`

Changes in Part 2:

1. Settings/manifest atomic writes now refuse to write through a redirected or non-directory parent rather than creating parent directories with `create_dir_all`.
2. NOVA data-folder creation and user-directory operations now re-check that their trusted root/parent is a real directory and not a symlink or Windows reparse point.
3. Trash metadata writes enforce the same 64 KiB limit already used when listing/deleting Trash items; restore now checks the limit before reading metadata.
4. Trash metadata writes verify the metadata directory is a real directory before opening a temporary file.
5. Isolated Chrome accepts only HTTP(S) addresses and rejects user-info, whitespace, control characters, quotes and backslashes.
6. File staging for Windows Sandbox uses an exclusive destination file, copies from an opened source handle, enforces the 512 MiB limit during copying and removes staging folders with NOVA's reparse-safe deletion helper.
7. Security cleanup now counts and deletes only regular `.wsb` files whose names match NOVA's `Browser-...` or `Sample-...` convention. Unrelated `.wsb` files are preserved, and directory-read errors are no longer silently discarded.
8. Added seven regression tests. The source tree now declares 68 Rust `#[test]` functions in total, including platform-specific tests.

## Part 3 — Remaining platform modules and packaging

Reviewed and updated in this pass:

- `src/terminal.rs`, `src/wifi.rs`, `src/unikey.rs`, `src/unicode.rs`
- `src/windows_process.rs`, `src/main.rs`
- `src/apps.rs` publisher verification path, build scripts, GitHub Actions workflow and user-facing documentation

Changes in Part 3:

1. UniKey and Chrome Authenticode checks now use NOVA's bounded Windows process runner, with a 15-second deadline, process-tree cleanup and capped output rather than unbounded `Command::output()` calls.
2. Executable paths are passed to PowerShell via a process environment variable instead of interpolating paths into script source. This preserves Unicode paths and avoids PowerShell-source injection through unusual file names.
3. UniKey launches with its own executable folder as the working directory so relative companion configuration/resource files stay associated with the USB copy.
4. The suspended-child runner now fails closed if the initial thread's suspend count is not exactly the one expected from `CREATE_SUSPENDED`.
5. Added regression coverage for direct and selected Wi-Fi password masking, non-BMP Unicode surrogate-pair decoding and Vietnamese WLAN profile XML escaping.
6. Re-reviewed CMD parsing and bounded history, Wi-Fi input/secret handling and profile lifecycle, UTF-8/UTF-16 decoding, startup error handling, Rust toolchain pinning, Windows CI and portable packaging scripts.

The source tree now declares 71 Rust `#[test]` functions across all target configurations.

## Validation limits

The current packaging container does not provide `rustc`, `cargo` or `rustfmt`. The changes have been reviewed statically, but `cargo test`, clippy, formatting and a Windows Release build have not been executed here. A Windows GitHub Actions run on this exact source revision and real Windows runtime checks are still required.
