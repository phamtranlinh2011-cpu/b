# NOVA v0.5.7 – Hardware receive match exhaustiveness follow-up

## Reported CI failure

Windows CI reported E0004 at `match hardware_update` in `src/app.rs`: the compiler still considered `Some(Err(std::sync::mpmc::TryRecvError::Empty))` uncovered, despite the prior explicit `TryRecvError::Empty` arm.

## Corrective change

The match now keeps the explicit `Some(Err(TryRecvError::Disconnected))` arm for a stopped worker, then adds `Some(Err(_))` as a conservative fallback for every other receive error. The fallback preserves the last known hardware snapshot and requests another repaint only during first-sample initialization. It does not terminate the worker or falsely report a hardware failure.

This catch-all is intentionally used instead of naming `Empty` again. It makes the outer `Option<Result<...>>` match exhaustive even if the standard-library error enum is non-exhaustive or its implementation changes. `Some(Ok(None))` continues to represent the normal no-new-snapshot state.

## Retained changes

- Wi-Fi worker owns `worker_ssid = ssid.clone()` while the original `ssid` remains available for UI status.
- File Manager opens the egui window with a local `files_open` flag and copies the resulting value back to `self.files_open` after `.show(...)`.
- CI still reports the checked-out commit and the SHA-256 of `src/app.rs` before compilation.

## Static checks performed

- Verified the Hardware Center match has an explicit `Disconnected` arm and a following `Some(Err(_))` fallback.
- Verified the Wi-Fi cloned-SSID invariant remains present.
- Verified the File Manager local-open-flag invariant remains present.
- Verified the PowerShell invariant script requires the catch-all receive-error arm.
- Parsed the ZIP central directory and tested all compressed entries for integrity.

## Not performed

The current environment does not provide the Rust toolchain or Windows runtime. `cargo fmt`, `cargo check`, `cargo test`, Clippy, Windows linking, PowerShell parser validation, and the packaged startup smoke test have not been executed. This package is not being represented as a confirmed successful Windows build.
