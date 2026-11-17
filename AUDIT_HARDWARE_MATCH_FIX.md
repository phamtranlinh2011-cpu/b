# NOVA Native v0.5.7 — Hardware match exhaustiveness fix

## Issue

The Windows CI compile reported `E0004` in `src/app.rs` at the `match hardware_update` expression in `monitor_window`. The expression has type:

`Option<Result<Option<HardwareSnapshot>, TryRecvError>>`

The match handled `Some(Ok(Some(_)))`, `Some(Ok(None))`, `Some(Err(TryRecvError::Disconnected))`, and `None`, but omitted `Some(Err(TryRecvError::Empty))`.

## Change

The `Some(Ok(None))` and `Some(Err(TryRecvError::Empty))` cases now share one arm. Both mean there is no new telemetry snapshot available; the current snapshot is retained, and a short repaint is requested only while the first hardware sample is still initializing. `Disconnected` continues to be treated as an unexpected worker stop.

The CI source-invariant script now verifies that the hardware match includes this case. Existing Wi-Fi SSID ownership and File Manager local-open-flag fixes from the Library-CI-Guarded source are retained.

## Checks performed

- Confirmed the hardware match now covers all variants of the outer `Option`, inner `Result`, and nested `Option`.
- Confirmed the existing Wi-Fi `worker_ssid` clone remains present.
- Confirmed the File Manager continues to use a local `files_open` flag.
- Confirmed the CI invariant script includes the new hardware-match check.
- Validated `Cargo.toml` parsing and ZIP integrity.

## Not performed

This environment does not have the Rust toolchain or Windows runtime available. `cargo fmt`, `cargo check`, `cargo test`, Clippy, and the Windows GitHub Actions workflow have not been run. This package therefore contains a source-level fix, not a verified successful Windows build.

## Version note

The Cargo package version remains `0.5.7`, matching the source project. No version bump was made as part of this targeted compile fix.
