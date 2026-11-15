# NOVA Native 0.3.4 — Reliability and hardening

- Fixed CMD output capture so readers continue draining stdout/stderr after the 1 MiB capture limit. This prevents the child process from blocking on a full pipe merely because NOVA stopped storing output.
- Bounded the in-memory terminal transcript by both line count (1,500) and total UTF-8 bytes (4 MiB).
- Resolve `cmd.exe`, `taskkill.exe` and `netsh.exe` through `%SystemRoot%\System32` rather than searching the current directory or PATH.
- Added a unit test for the terminal history memory bound.

## Still requires Windows validation

- `cargo test --target x86_64-pc-windows-msvc` and release build.
- Actual WLAN scan/import/connect/delete against physical adapters.
- CMD process-tree termination and descendant pipe inheritance.
- Long-running command responsiveness: command execution still occurs synchronously from the UI thread and can temporarily block the NOVA window until completion or timeout.
- Wi-Fi XML credentials are still briefly written in plaintext to TEMP without a custom DACL.
