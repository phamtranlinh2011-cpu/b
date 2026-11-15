## UI update (0.5.6)

NOVA now has responsive desktop and Start grids, a searchable List/Grid File Manager with contextual actions and clearer empty states, Settings pages for Appearance/System/Apps/Network/Security/About, System theme following Windows, a Quick Settings / recent-activity panel, keyboard shortcuts, persistent Start pin/unpin controls, a horizontally scrollable taskbar for all supported workspaces, taskbar app context menus and left/right snap layouts for supported workspaces. Windows remain draggable/resizable with egui's native in-app window behavior. See `RELEASE_NOTES_0.5.6.md`.

## 0.5.2 — Windows-style desktop refresh

- Replaces the old NOVA Launcher popup, left navigation rail and duplicate tile/dock navigation with a Windows-inspired desktop, centered taskbar and Start surface.
- Uses one application catalog for desktop shortcuts, pinned apps, search and categorized All apps.
- Adds pinned, recommended quick access, search, active taskbar states, hover transitions and a reduced-motion path.
- Existing app windows and safety confirmation routes are preserved.

## 0.5.1 — Security Center GUI workspaces

- Adds NOVA Security Center with an authorized-asset scope acknowledgement.
- Adds five GUI workspaces: Zenmap/Nmap, Greenbone GSA, OWASP ZAP, Wireshark and Burp Suite.
- Third-party engines are optional and must be installed separately; Greenbone uses a separately hosted server/web console.
- The scope acknowledgement is a user workflow guard, not a technical scan boundary. Third-party tools run on the Windows host and are not sandboxed by this release.
- Fixes the UniKey enum comparison, quarantine cleanup iterator, Chrome/Trash egui borrow issues, deprecated egui ID calls and a duplicate Calculator `.open()` call.


## What changed in 0.5.1

- Unicode UI fallback: eframe's `system_fonts` feature and `NativeOptions::system_font_fallback` load missing glyphs from fonts installed on the Windows host (CJK, Arabic and other scripts, where the host has a suitable font).
- Unicode Editor: opens UTF-8 (with or without BOM) and UTF-16 LE/BE files with BOM; saves UTF-8. Filesystem paths and text fields use Unicode.
- Terminal: CMD starts with code page 65001 for UTF-8 console-mode tools. Some legacy programs may still emit OEM/ANSI output and display imperfectly.
- UniKey app: its download helper is embedded in `NOVA.exe`, fetches the official UniKey 4.6 RC2 x64 ZIP, enforces archive/executable size limits, verifies Authenticode with the exact publisher `PHAM KIM LONG`, and places it on USB without installing a service or startup task. Launch-time publisher checks use a bounded process and preserve Unicode paths; UniKey starts from its USB folder. Select Unicode plus Telex or VNI in its tray menu.
- Windows Sandbox integration: inspect a single NOVA workspace file using a read-only mapped copy, with networking, vGPU and clipboard disabled; or browse with Chrome in a fresh disposable Sandbox profile.
- Portable data hardening: build the Data tree component by component and reject symlinks, junctions and reparse points in core folders; Trash and Security roots are checked again before use.
- Chrome's isolated action is the primary choice; launching normal Chrome on the Windows host requires an explicit confirmation.
- Security Center explains the isolation boundary and removes NOVA's staged sample copies and generated `.wsb` launch configs after all Sandbox sessions are closed.

## Portable runtime

The intended USB package contains `NOVA.exe`, `portable.flag`, `README_PORTABLE.txt`, `BUILD_INFO.txt`, `scripts/Download-UniKey.ps1` (source of the helper embedded into NOVA.exe), and a `Data/` folder. Keep them together. The app checks for `portable.flag` beside `NOVA.exe` and stores NOVA files/settings/Trash/security staging below `Data/`.

On a compatible Windows 10/11 x64 machine, open the USB in File Explorer and double-click `NOVA.exe`. The target machine does not need Node.js, npm, Rust, Cargo, WebView2, a NOVA installer or administrator elevation. It still requires Windows system components and a compatible graphics driver for the Glow/OpenGL renderer. Windows intentionally does not auto-run arbitrary programs just because a USB drive is inserted.

## Built-in apps

| App | Behavior |
|---|---|
| Files | Browse `Data/Users/Default`; create folders; select, open as text, and move items into NOVA Trash; inspect a selected file in Windows Sandbox |
| Editor | Unicode text editor; UTF-8 and UTF-16 LE/BE input; UTF-8 output; 2 MiB per-file limit |
| Calculator | Basic addition, subtraction, multiplication and division |
| System Monitor | Host CPU usage, memory and process count; refresh while visible |
| Terminal | Real Windows CMD with signed-in user's permissions; UTF-8 code page request; NOVA Wi-Fi commands |
| Settings | Dark/light theme, accent color, reduced-motion option, shortcuts to UniKey and Security Center |
| NOVA Trash | Restore or permanently remove items deleted through NOVA Files |
| Google Chrome | Open an existing Chrome install in Windows Sandbox (recommended) or, after confirmation, on the host |
| UniKey | Optional official signed Vietnamese input companion, downloaded to USB on demand |
| Security Center | Authorized-scope workflow reminder, Sandbox availability, isolation limits, staged-sample cleanup and five GUI security-tool workspaces |
| Network Discovery | Opens the separately installed Zenmap/Nmap GUI |
| Vulnerability Management | Opens a separately hosted Greenbone Security Assistant (GSA) console; scanner/server is not bundled |
| Web Security | Opens the separately installed OWASP ZAP desktop GUI |
| Packet Analyzer | Opens the separately installed Wireshark desktop GUI |
| Web / API Inspector | Opens the separately installed Burp Suite Community/Professional GUI |

## Important security boundaries

- **NOVA itself is not a VM or a security boundary.** It is an ordinary Windows process running as the current user. A vulnerability in NOVA, Windows, a driver, firmware or the hypervisor could still affect the host.
- **Inspect in Sandbox:** NOVA copies one selected file into `Data/Security/Quarantine/<session>/`, maps only that folder read-only, disables networking, vGPU and clipboard, and opens Explorer. The sample is not auto-executed; the user manually chooses what to open inside the Sandbox. The original workspace file is not modified.
- **Isolated Chrome:** the existing Chrome application directory is mapped read-only. A new profile is created inside Windows Sandbox; cookies, cache, extensions and downloads stay in the disposable VM. Networking is enabled for browsing and can expose the Sandbox to local-network services. Clipboard and writable host mappings are disabled.
- Close Windows Sandbox and confirm deletion to discard the VM's software, files and state. NOVA's staged sample copy and `.wsb` config remain on the USB until you close all Sandbox sessions and clear them in Security Center.
- Windows Sandbox is supported on Windows Pro, Enterprise and Education, not Windows Home. It also requires hardware virtualization, compatible Windows build and sufficient RAM/CPU/disk. NOVA fails closed when Sandbox is unavailable; it does not fall back to running a suspicious file on the host.
- **Terminal/CMD, normal host Chrome, UniKey, Wi-Fi and ordinary NOVA file operations are not sandboxed.** CMD commands can change the real host. UniKey is a separate host process and can receive keyboard input system-wide.
- UniKey is downloaded from `unikey.org`; NOVA checks a valid Authenticode signature and the expected publisher before launch. A valid signature is a provenance check, not a guarantee that software is vulnerability-free. Chrome host and Sandbox launch also reject executables without a valid Google publisher signature.
- Wi-Fi setup imports a profile into the real Windows user account. Its XML password file exists briefly in host TEMP with a protected DACL for the owner, SYSTEM and local Administrators; a same-user malicious process is still outside NOVA's protection.
- NOVA does not automatically sandbox every download made by Windows, host Chrome, CMD or another host application. Only the explicit isolated browser and file-inspection routes use Windows Sandbox.
- Isolated Chrome has networking enabled; do not treat it as an offline malware-analysis environment. For unknown executable files, use **Inspect in Sandbox**, which disables networking.
- Keep Windows and Microsoft Defender up to date. No sandbox can promise protection against every future hypervisor or OS vulnerability.

## Build

Build computer requirements: Rust 1.95.0, Microsoft C++ Build Tools/MSVC and Windows SDK, plus internet access for dependencies on first build. Run `BUILD_NOVA.bat` or `scripts/build-portable.ps1` on Windows. They run tests, build `NOVA.exe`, create a portable folder, ZIP and SHA256. The target USB PC does not need developer tools.

GitHub Actions uses `.github/workflows/rust.yml` on `windows-latest`, runs `cargo test --locked`, builds Release and uploads `NOVA-OS-Native-v0.5.6-USB`. The workflow generates or refreshes `Cargo.lock` when the root package version changes; the GitHub Actions token must be allowed to write `contents` for that lockfile commit.

## Source layout

```text
src/app.rs              Windows-style desktop, Start/taskbar and in-shell applications
src/apps.rs             Host Chrome detection, URL validation and launcher
src/security_tools.rs   Five GUI tool integrations, detection and scope reminder
src/isolation.rs        Windows Sandbox config, read-only staging and cleanup
src/storage.rs          Portable paths, settings, validation and atomic JSON writes
src/terminal.rs         CMD/Wi-Fi command parsing and background task model
src/trash.rs            Portable NOVA Trash and restore operations
src/unicode.rs          UTF-8 / UTF-16 text decoding
src/unikey.rs           Official UniKey discovery, signature validation and launcher
src/wifi.rs             Windows WLAN profile operations and protected TEMP XML
src/windows_process.rs  Windows Job Object, process timeout and bounded output
scripts/Download-UniKey.ps1  Official UniKey download and Authenticode verification
.github/workflows/      Windows CI build and packaging
.cargo/                 Windows static CRT target flags
docs/                   Architecture, security, research and roadmap
```

## Documentation

- [Vietnamese guide](HUONG_DAN_VI.md)
- [Portable USB guide](README_PORTABLE.txt)
- [Architecture](docs/ARCHITECTURE.md)
- [Security boundary and threat model](docs/SECURITY_0.5.0.md)
- [Research references](docs/RESEARCH_REFERENCES_0.5.0.md)
- [Release notes](RELEASE_NOTES_0.5.0.md)
- [0.5.1 Security Center and GUI tool references](docs/RESEARCH_REFERENCES_0.5.1.md)
- [0.5.1 GUI tool integration and security boundary](docs/SECURITY_GUI_TOOLS_0.5.1.md)
- [0.5.1 release notes](RELEASE_NOTES_0.5.1.md)
- [0.5.2 Windows-style desktop refresh](RELEASE_NOTES_0.5.2.md)
- [0.5.2 UI research references](docs/RESEARCH_REFERENCES_0.5.2.md)
- [0.5.5 UI Polish](RELEASE_NOTES_0.5.5.md)
- [0.5.6 audit fixes](RELEASE_NOTES_0.5.6.md)

## Validation status

This package can be structurally checked (TOML/YAML parsing, source invariants, Windows workflow contents, version/manifest checks and ZIP integrity). The packaging environment used here does not have `rustc` or `cargo`, so it cannot claim that Rust compilation or `cargo test` passed. A successful GitHub Actions run on the exact 0.5.6 commit is required before treating the EXE as build-verified. Real Windows tests are still needed for UniKey signing/typing, Sandbox edition and launch behavior, Chrome mapped-folder operation, USB persistence and Windows security boundaries.
