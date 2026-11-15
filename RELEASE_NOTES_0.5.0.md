# NOVA Native 0.5.0 — Unicode, UniKey and Isolation

## Added

- Explicit eframe system-font fallback for missing glyphs, including CJK and other scripts when a suitable Windows font is installed.
- Editor text decoding for UTF-8 (BOM optional) and UTF-16 LE/BE with BOM; saved files are UTF-8.
- UTF-8 code-page request (`chcp 65001`) before CMD command execution.
- UniKey Launcher/Settings integration. The download helper is embedded in `NOVA.exe`, fetches the official UniKey 4.6 RC2 x64 ZIP, enforces archive-size limits and requires a valid Authenticode signature with exact publisher `PHAM KIM LONG` before NOVA starts it.
- Security Center and Windows Sandbox workflows for selected file inspection and isolated Chrome.
- A read-only, offline, clipboard-disabled and vGPU-disabled Sandbox profile for suspicious workspace files.
- A fresh Sandbox Chrome profile with read-only mapping of the host Chrome application directory. Browser profile data and downloads stay inside the disposable VM.
- Explicit confirmation before launching normal host Chrome.

## Security behavior

- NOVA does not claim to be a VM. CMD, UniKey, Wi-Fi, normal Chrome and ordinary file operations still execute on or modify the Windows host.
- Suspicious files are copied into the NOVA quarantine staging area and exposed read-only to Windows Sandbox. Files are not auto-executed.
- Chrome Sandbox browsing enables networking to load websites; networking can expose the Sandbox to local-network services. Use offline Inspect in Sandbox for unknown executables.
- Closing Windows Sandbox discards its guest state. The USB staging copies and generated `.wsb` configs remain until the user closes every Sandbox session and clears them in Security Center.
- Windows Sandbox is not available on Windows Home and requires hardware virtualization and supported Windows editions.
- If Windows Sandbox is unavailable, NOVA does not fall back to executing suspicious files on the host.

## Build and validation

- GitHub Actions package name is `NOVA-OS-Native-v0.5.0-USB`.
- Workflow refreshes `Cargo.lock` only when absent or when the root crate version changes, then uses `--locked` for tests/build.
- The source packaging environment has no Rust/Cargo compiler. A Windows GitHub Actions run is required to verify `cargo test` and Release build.

### Additional hardening review

- Create the portable/local Data directory tree one component at a time; reject symlink, junction and reparse-point redirection before startup creates child folders.
- Apply the same real-directory checks to NOVA Trash and Sandbox Security/Quarantine.
- UniKey helper refuses reparse-point `Apps`/`UniKey` destinations, stages its verified executable under a unique temporary name on the USB, then verifies the final copy.
- Documentation clarifies that ordinary host downloads and host apps are not automatically sandboxed, and same-user concurrent filesystem races remain outside this protection.

- Permanent Trash deletion and Security Center cleanup now recurse without following nested symlinks or Windows reparse-point directories.
- UniKey download helper rejects ZIP path traversal and archive symlink entries, verifies the staged executable with its `.exe` suffix, and replaces an existing verified copy only after staging checks pass.
- Chrome host and Sandbox launch now require a valid Authenticode signature from the expected Google publisher name; unsigned or lookalike `chrome.exe` files are rejected.
- Security notes now explicitly describe host-side Wi-Fi profile import and its short-lived protected TEMP XML, plus the fact that NOVA does not automatically sandbox every host download.
