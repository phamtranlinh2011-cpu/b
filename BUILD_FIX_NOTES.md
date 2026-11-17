# Build Fix Notes — v0.5.7

This full source package is based on the newest cumulative NOVA v0.5.7 source in the Library (`NOVA-OS-Native-v0.5.7-Borrow-Guard-Fixes.zip`). It includes the four earlier source-improvement phases and the Rust ownership/borrow corrections below.

## Why the repeated CI errors appeared

The recent log's source locations (Wi-Fi around `src/app.rs:1338–1353` and File Manager around `src/app.rs:2923`) match the older Library archive `NOVA-OS-Native-v0.5.7-Phase-4-CI-Compile-Fixes.zip`, where both faulty expressions were still present. The current cumulative source contains the fixes, at different line numbers because it includes additional application code.

## E0382 — moved `ssid`

`start_network_connect` clones the selected SSID into `worker_ssid` before spawning the worker thread. The closure owns the clone, while the original `ssid` remains available for the UI status message.

```rust
let worker_ssid = ssid.clone();
// ...
.spawn(move || {
    let result = wifi::connect(&paths, &worker_ssid, password.as_deref())
        .and_then(|_| wifi::wait_for_connection(Some(&worker_ssid), Duration::from_secs(15)))
        .map_err(|error| error.to_string());
    let _ = sender.send(NetworkWorkerResult::Connect(result));
})
```

## E0500 — overlapping mutable borrow of `self`

File Manager uses a local `files_open` value for `Window::open`, allowing the egui closure to call methods on `self`. The value is copied back after `.show(...)` returns.

```rust
let mut files_open = self.files_open;
let shown = window.open(&mut files_open).show(ctx, |ui| {
    // File Manager UI may call methods on self here.
});
self.files_open = files_open;
```

## CI source verification

Before formatting, Clippy and build/test steps, CI checks the ownership/borrow invariants. It also prints the checked-out Git commit, workflow ref and `src/app.rs` SHA-256 so a run can be tied to the exact source revision. The invariant script checks both Wi-Fi ownership and File Manager local-state handling, including copying the open flag back, and rejects direct `Window::open(&mut self.<field>)` borrowing.

## Verification status

- Source assertions for the Wi-Fi clone and worker use: passed.
- Source assertions for local File Manager open flag and state copy-back: passed.
- CI YAML structure and TOML manifest parsing: checked.
- ZIP integrity: checked.
- Rust compilation, unit tests, Clippy, PowerShell AST execution and Windows runtime smoke test: not run in this environment. A Windows GitHub Actions run is still required to establish build success.
