# NOVA Native 0.3.2 — build and reliability fixes

## Fixed
- Windows compile errors with `sysinfo` 0.40.3 and egui 0.37.0.
- Borrow-checker conflict in masked Terminal input.
- Deprecated egui `Ui::id()` call.
- System Monitor startup panic risk when system information initialization fails.
- File path creation checks and Windows-invalid/reserved filename handling.
- Corrupt Wi-Fi manifest being silently treated as empty.
- Temporary Wi-Fi file name collision handling and rollback when manifest registration fails.
- CMD reader errors, output truncation reporting, process-kill fallback and bounded pipe-drain waiting to prevent an indefinite UI hang.
- Calculator rounding of very small results.

## Validation still required
- `cargo test` and `cargo build --release --target x86_64-pc-windows-msvc` on GitHub Actions.
- Runtime tests of CMD timeout/child processes, Wi-Fi profiles, USB read/write and graphics on actual Windows 10/11 devices.
- Wi-Fi credential XML still briefly contains plaintext credentials in the current user's TEMP directory; custom DACLs and abrupt-crash cleanup are not implemented.
