# NOVA Native v0.5.7 — Audit Phase 2: Stability

## Scope

This phase continues from the Phase 1 Security & Data Integrity source. It changes only the stability-related areas below and does not add Cargo dependencies.

## Changes

### 1. Wi-Fi discovery independent of Windows display language

- Replaced parsing of localized `netsh wlan show networks` text with the Windows WLAN API (`WlanOpenHandle`, `WlanEnumInterfaces`, `WlanScan`, `WlanGetAvailableNetworkList`).
- Added RAII cleanup for WLAN client handles and API-allocated memory.
- Added compile-time struct size/offset assertions for the hand-written WLAN FFI layouts.
- Decode SSIDs as UTF-8 only when the byte length is 1–32 and contains no control characters. Invalid/non-UTF-8 byte strings are skipped instead of being lossily converted into a different network name.
- Deduplicate and sort names before returning them to Settings. Scanning stays on the existing `nova-wifi-scan` worker; the UI thread is not blocked by the asynchronous scan wait.
- Map `ERROR_ACCESS_DENIED` from `WlanScan` / `WlanGetAvailableNetworkList` to a location-consent explanation and expose a shortcut to `ms-settings:privacy-location`.

Microsoft documents that those WLAN APIs can return `ERROR_ACCESS_DENIED` without precise-location consent, and that `WlanScan` returns asynchronously with a four-second driver expectation. See:
- https://learn.microsoft.com/en-us/windows/win32/nativewifi/wi-fi-access-location-changes
- https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlanscan
- https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/ns-wlanapi-wlan_available_network
- https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/ns-wlanapi-wlan_available_network_list

### 2. Hardware provider initialization and sampling

- Moved initialization of disk, network, GPU and sensor providers into the `nova-hardware-telemetry` worker.
- The worker owns `sysinfo` provider objects; only cloned plain-data `HardwareSnapshot` values cross to the UI thread through a one-slot bounded channel.
- Provider sampling pauses while the Hardware Center is closed and is requested again when it reopens.
- Snapshots are cloned only when a provider refresh occurs, not on every 500 ms worker loop.
- Worker startup failure or unexpected disconnect is exposed in the UI instead of silently displaying partial telemetry.
- GPU status now differentiates an unavailable provider, an available provider reporting no adapters, and a provider reporting adapter data.

CPU/RAM and process sampling retain their separate existing refresh cadence; this phase moves the disk/network/GPU/sensor providers off the egui frame path.

### 3. File Manager progress, cancellation and lifecycle

- Added cooperative cancellation checks for recursive search at each directory and entry.
- Added bounded-block file copy (256 KiB) with cancellation checks between reads, byte/item progress counters, and cleanup of the private staging tree when a copy is cancelled or fails.
- Preserved the existing `copy_item` API and added a cancellable worker-facing API.
- Move requests check cancellation between source items. A single filesystem move/rename is atomic and cannot be interrupted halfway through.
- The UI locks list actions while a worker is active, reports the current operation and progress, and offers Cancel.
- Closing the Files window or NOVA signals the active worker to stop cooperatively. Clearing a recursive search cancels it and prevents late/stale results from replacing the normal directory view.
- Search cancellation/failure restores the current folder rather than leaving results from a previous search visible.

### 4. Documentation

- Updated `README.md` and `HUONG_DAN_VI.md` to document worker behavior, progress/cancellation, WLAN API discovery and Windows location consent, and GPU provider states.

## Static validation performed

- `Cargo.toml` parsed successfully using Python `tomllib`.
- `.github/workflows/rust.yml` parsed successfully using PyYAML.
- Rust lexer delimiter-balance check passed for the four modified Rust files: `app.rs`, `file_ops.rs`, `hardware.rs`, and `wifi.rs`.
- Counted 143 declared Rust test functions across `src/` (including Windows-only conditional tests). This is a source inventory, not evidence that tests passed.
- Confirmed `Cargo.toml` is unchanged from the Phase 1 baseline; no dependency changes were introduced.
- Independently checked the expected Windows WLAN structure sizes and offsets with a Python `ctypes` layout model.
- Source diff is limited to `README.md`, `HUONG_DAN_VI.md`, `src/app.rs`, `src/file_ops.rs`, `src/hardware.rs`, `src/wifi.rs`, and this audit report.

## Not verified in this environment

The environment does not have `cargo`, `rustc`, `rustfmt`, or PowerShell available. Therefore this phase has **not** been compiled, formatted with rustfmt, tested by `cargo test`, checked by Clippy, or exercised on Windows. In particular, actual WLAN driver behavior, Windows location-consent UX, hardware provider availability, and worker shutdown under real filesystem latency still require Windows validation.

The source archive does not include `Cargo.lock`, matching the prior source-artifact convention. Preserve the repository's committed `Cargo.lock`; the existing CI workflow uses `--locked`. No new dependencies were added in this phase.

## Operational caveats

- WLAN discovery can require Windows precise-location consent. If the WLAN APIs return `ERROR_ACCESS_DENIED`, enable the relevant Location settings and retry. The scan worker waits four seconds (the documented driver-completion expectation) before reading available networks.
- Non-UTF-8 SSID byte sequences cannot currently be represented by NOVA's UTF-8 SSID input and are omitted from scan results.
- Search and copy cancellation are cooperative. A blocked filesystem call cannot be forcibly interrupted; a single Move operation completes or fails before the worker checks cancellation for the next item.
- The hardware worker is signalled to stop through channel disconnection when NOVA closes. A provider call already in progress must return before the worker thread can exit.
