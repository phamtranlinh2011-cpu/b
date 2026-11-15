# NOVA Native 0.5.1 — Security Center and GUI Tool Workspaces

## Fixes for Windows CI

- `UniKeyProcessKind` now derives `PartialEq` and `Eq` for process-state comparisons.
- Fixed the quarantine cleanup loop: `read_dir(...).collect::<io::Result<Vec<_>>>()` already unwraps entries, so the collected `DirEntry` is no longer incorrectly used with `?`.
- Deferred Trash refresh, isolated Chrome launch and UniKey process actions until after egui window closures to avoid overlapping mutable borrows of `NovaApp`.
- Replaced deprecated egui `Id::new` calls with `Id::unique` for stable locally-owned widget identities.
- Removed an accidental duplicate `.open()` call on the Calculator window.
- Added known default Zenmap/Nmap and OWASP ZAP install paths to GUI detection.

## Security Center and five GUI tool workspaces

NOVA now includes separate GUI workspaces for:

1. Network Discovery — Zenmap (Nmap GUI)
2. Vulnerability Management — Greenbone Security Assistant web console
3. Web Security — OWASP ZAP desktop
4. Packet Analyzer — Wireshark desktop
5. Web/API Inspector — Burp Suite desktop

The Security Center includes an explicit authorized-scope field and acknowledgement. This is a user workflow guard only; it does not technically constrain targets within third-party tools. Each third-party engine must be installed separately and runs on the Windows host. Greenbone requires a separately hosted scanner/server; NOVA opens its GSA web interface.

No third-party scanner binaries are silently downloaded, bundled or executed automatically. NOVA only detects known installation paths and launches the GUI after explicit user action. Users remain responsible for matching each tool's target list to the authorized scope.

## Validation status

Static source/configuration checks are included in the source package. The Rust compiler, Windows CI tests, actual third-party tool launch paths, and runtime UI require verification by the Windows GitHub Actions workflow and a Windows host.

## Storage, Trash and Sandbox hardening — audit Part 2

- Settings and Wi-Fi manifest writes now reject redirected parent directories. Every existing component in trusted NOVA data-root and parent-directory chains is checked again before use.
- Trash metadata is capped at 64 KiB when written and before restore reads it; unsafe metadata folders are rejected.
- Isolated Chrome accepts only HTTP(S) addresses, not file/script schemes or command-like inputs.
- Windows Sandbox sample staging copies from an opened source handle into an exclusive destination, enforces the 512 MiB limit during the copy, and uses reparse-safe cleanup on failures.
- Security cleanup removes only regular NOVA-generated `Browser-*.wsb` and `Sample-*.wsb` files; unrelated user `.wsb` files are retained. Directory enumeration errors are surfaced rather than silently ignored.
- Added seven regression tests; the source declares 68 tests across all platform configurations.


## Platform and packaging audit — Part 3

- UniKey and host Chrome Authenticode checks now run through NOVA's bounded Windows process runner with a 15-second timeout, process-tree cleanup and capped output. Paths are passed through environment variables instead of being interpolated into PowerShell source.
- UniKey starts with its USB executable directory as the working directory for relative companion resources.
- The Windows process runner fails closed if the initial thread does not have the expected suspended count before resume.
- Added regression tests for Terminal password masking, UTF-16 non-BMP Unicode and Vietnamese Wi-Fi profile XML. The source declares 71 tests across target configurations.
- Reviewed Terminal, Wi-Fi, UniKey, Unicode, Windows process handling, startup, CI and packaging. Rust compilation and Windows runtime checks remain pending because this packaging environment has no Rust toolchain.
