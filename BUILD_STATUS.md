# NOVA Native 0.3.5 — build status

## Delivered in this source package
- Rust native GUI using eframe/egui and the Glow renderer.
- Portable data path using `portable.flag` and `Data/` beside `NOVA.exe`.
- Built-in Files, Editor, Calculator, Settings, System Monitor and Windows CMD terminal.
- Wi-Fi scan/profile/connect/delete commands using `netsh wlan`.
- Asynchronous terminal worker for CMD and WLAN operations; the UI remains responsive while those operations run.
- Shared bounded Windows process runner with 30-second CMD / 20-second WLAN timeouts, continuously drained capped output, and Windows Job Object cleanup.
- Temporary WLAN XML created with a protected DACL, removed after import, with startup cleanup for matching abandoned files older than 10 minutes.
- Pinned Rust 1.95.0 toolchain and a first-run CI step that generates/commits `Cargo.lock`; later CI/local builds use `--locked`.

## 0.3.5 fixes
- CMD and Wi-Fi operations no longer execute on the egui UI thread.
- `cmd.exe` and `netsh.exe` are resolved with `GetSystemDirectoryW`, not the working directory or PATH.
- A Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` owns each spawned process; timeout and cleanup terminate processes remaining in that job.
- Stdout/stderr readers keep draining after the 1 MiB capture cap to avoid pipe backpressure.
- WLAN import XML access is restricted to the file owner, SYSTEM and local Administrators. NOVA closes the writer before `netsh` opens it, deletes the file after import, and removes matching abandoned files older than 10 minutes on later startup.
- Added 27 unit tests covering path validation, calculator formatting, Wi-Fi parsing/profile handling, terminal task scheduling, process capture/timeout, output draining and temporary-file cleanup.

## Validation evidence in this package
- ZIP integrity, SHA256, TOML parsing, workflow YAML syntax, version-reference consistency, source-level delimiter/import checks and key source invariants are validated during packaging.
- This environment has no Rust/Cargo toolchain and cannot run `cargo test`, `cargo build`, rustfmt or clippy. A fresh Windows Actions run is required to compile the FFI declarations, execute all unit tests, verify Job Object behavior and build the EXE.

## Remaining Windows validation
- Confirm `netsh.exe` can read the protected WLAN XML after NOVA closes the writer, then confirm normal deletion and stale-file cleanup on supported Windows versions.
- Test actual Wi-Fi scan/import/connect/delete on multiple wireless adapters and routers.
- Test behavior under nested Job Object restrictions and app shutdown during a command.
- Test USB persistence, read-only media, graphics compatibility and Windows 10/11 hardware coverage.

## Important
This is source code, not a precompiled EXE. GitHub Actions must pass on a Windows runner before treating the package as build-verified. NOVA is a Windows desktop environment, not a separate OS kernel or bootable Live USB.
