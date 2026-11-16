## Current release: 0.5.7 — Stability, Security and Release Pipeline

### New desktop app improvements in this source update

- UniKey 4.6 RC2 x64 is now fetched from the official UniKey website during Windows packaging, Authenticode-verified, bundled at `Apps/UniKey/UniKeyNT.exe`, and automatically started when its NOVA shortcut is opened. There is no in-app download/install prompt.
- Calculator upgraded to a ClassWiz-inspired scientific workspace with expression parsing, DEG/RAD/GRA, scientific/trigonometric/hyperbolic functions, constants, factorial, combinations/permutations, normal/binomial/Poisson distributions, 1/2-variable statistics and regression, linear/quadratic equations, quadratic inequalities, numerical derivative/integration, numeric tables, Base-N, 2×2/3×3 matrices, vectors and ratios. This is a growing compatible feature set, not a claim of full fx-991CW feature parity.
- Google Chrome shortcut now launches the actual installed Windows Chrome executable as a native Chrome window, using the user's existing Chrome profile and settings instead of drawing a NOVA imitation window.

NOVA now has responsive desktop and Start grids, a searchable List/Grid File Manager with contextual actions and clearer empty states, Settings pages for Appearance/System/Apps/Network/Security/About, System theme following Windows, a Windows 11-inspired centered floating taskbar with a balanced system tray and local clock, and an animated Quick Settings panel. The taskbar shows all configured pinned apps plus unpinned workspaces while they are open; the app strip scrolls horizontally when it is full, rather than forcing every catalog item into the dock. Its side rails adapt symmetrically so Start and app shortcuts remain centered at narrower widths. Quick Settings controls and Start Menu grids now size themselves from the real content width, avoiding clipped buttons and overflow. Both Pinned and All apps content scroll vertically when the Start surface is short; Quick Access collapses to one, two or three columns based on the actual inner width. The File Manager List view allocates a bounded name column and size column, shortening long Unicode filenames without clipping the size or losing the full filename on disk. Start/Quick Settings surfaces use eased transitions, taskbar and desktop icons animate on hover, and app windows fade in/out with consistent rounded surfaces, borders and shadows. Snap panes no longer inherit oversized minimum widths after the transition; Settings reflows to one column, and Editor/Terminal/Trash/Monitor/Security action rows wrap when needed. The taskbar, Start menu and Quick Settings render in a foreground shell layer above app windows; Snap layouts reserve space above the dock and animate window position/size without covering it. Windows remain draggable/resizable with egui's native in-app window behavior. The taskbar is inside NOVA and does not replace the Windows host taskbar or control host volume/battery. See `RELEASE_NOTES_0.5.7.md` for the current release summary.

## 0.5.2 — Windows-style desktop refresh

- Replaces the old NOVA Launcher popup, left navigation rail and duplicate tile/dock navigation with a Windows-inspired desktop, centered taskbar and Start surface.
- Uses one application catalog for desktop shortcuts, pinned apps, search and categorized All apps.
- Adds pinned, recommended quick access, search, active taskbar states, hover transitions and a reduced-motion path.
- Updated in this source refresh: responsive centered floating taskbar, system tray clock, eased Start/Quick Settings transitions, hover feedback, pinned-plus-running app entries, safe snap-window movement, and shared modern widget/window styling.
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
- UniKey is bundled automatically into every Windows USB package at build time from the official UniKey 4.6 RC2 x64 archive. The packaging script validates archive paths and size, verifies Authenticode publisher `PHAM KIM LONG`, and places `UniKeyNT.exe` in `Apps/UniKey`. Opening UniKey in NOVA starts it automatically; no separate download/install step is shown. Select Unicode plus Telex or VNI in its tray menu.
- Windows Sandbox integration: inspect a single NOVA workspace file using a read-only mapped copy, with networking, vGPU and clipboard disabled.
- Portable data hardening: build the Data tree component by component and reject symlinks, junctions and reparse points in core folders; Trash and Security roots are checked again before use.
- Google Chrome opens the actual installed Windows Chrome application in a native Chrome window, using the user's normal Chrome profile, extensions, bookmarks and settings. It is not sandboxed by NOVA.
- Security Center explains the isolation boundary and removes NOVA's staged sample copies and generated `.wsb` launch configs after all Sandbox sessions are closed.

## Portable runtime

The intended USB package contains `NOVA.exe`, `portable.flag`, `QUICK_START.txt`, `BUILD_INFO.txt`, `THIRD_PARTY_NOTICES.txt`, `Apps/UniKey/UniKeyNT.exe`, `CHECKSUMS.sha256`, and a `Data/` folder. Keep them together. The Windows packaging script downloads UniKey from its official website and verifies its digital signature before bundling. If the build machine has no internet access, package creation stops rather than silently omitting UniKey.

On a compatible Windows 10/11 x64 machine, open the USB in File Explorer and double-click `NOVA.exe`. The target machine does not need Node.js, npm, Rust, Cargo, WebView2, a NOVA installer or administrator elevation. It still requires Windows system components and a compatible graphics driver for the Glow/OpenGL renderer. Windows intentionally does not auto-run arbitrary programs just because a USB drive is inserted.

## Built-in apps

| App | Behavior |
|---|---|
| Files | Browse `Data/Users/Default`; create folders; select, open as text, and move items into NOVA Trash; inspect a selected file in Windows Sandbox |
| Editor | Unicode text editor; UTF-8 and UTF-16 LE/BE input; UTF-8 output; 2 MiB per-file limit |
| Calculator | Scientific expressions, distributions, 1/2-variable statistics, equations/inequalities, numerical calculus, tables, Base-N, matrices, vectors and ratios |
| System Monitor | Host CPU usage, memory and process count; refresh while visible |
| Terminal | Real Windows CMD with signed-in user's permissions; UTF-8 code page request; NOVA Wi-Fi commands |
| Settings | Dark/light theme, accent color, reduced-motion option, shortcuts to UniKey and Security Center |
| NOVA Trash | Restore or permanently remove items deleted through NOVA Files |
| Google Chrome | Launch the installed native Windows Chrome window with its normal user profile |
| UniKey | Official signed UniKey 4.6 RC2 x64 bundled into the USB release; starts when its NOVA shortcut is opened |
| Security Center | Authorized-scope workflow reminder, Sandbox availability, isolation limits, staged-sample cleanup and five GUI security-tool workspaces |
| Network Discovery | Opens the separately installed Zenmap/Nmap GUI |
| Vulnerability Management | Opens a separately hosted Greenbone Security Assistant (GSA) console; scanner/server is not bundled |
| Web Security | Opens the separately installed OWASP ZAP desktop GUI |
| Packet Analyzer | Opens the separately installed Wireshark desktop GUI |
| Web / API Inspector | Opens the separately installed Burp Suite Community/Professional GUI |

## Important security boundaries

- **NOVA itself is not a VM or a security boundary.** It is an ordinary Windows process running as the current user. A vulnerability in NOVA, Windows, a driver, firmware or the hypervisor could still affect the host.
- **Inspect in Sandbox:** NOVA copies one selected file into `Data/Security/Quarantine/<session>/`, maps only that folder read-only, disables networking, vGPU and clipboard, and opens Explorer. The sample is not auto-executed; the user manually chooses what to open inside the Sandbox. The original workspace file is not modified.
- Close Windows Sandbox and confirm deletion to discard the VM's software, files and state. NOVA's staged sample copy and `.wsb` config remain on the USB until you close all Sandbox sessions and clear them in Security Center.
- Windows Sandbox is supported on Windows Pro, Enterprise and Education, not Windows Home. It also requires hardware virtualization, compatible Windows build and sufficient RAM/CPU/disk. NOVA fails closed when Sandbox is unavailable; it does not fall back to running a suspicious file on the host.
- **Terminal/CMD, normal host Chrome, UniKey, Wi-Fi and ordinary NOVA file operations are not sandboxed.** CMD commands can change the real host. UniKey is a separate host process and can receive keyboard input system-wide.
- UniKey is obtained from the official `unikey.org` release archive during packaging; NOVA checks a valid Authenticode signature and expected publisher before launch. A valid signature is a provenance check, not a guarantee that software is vulnerability-free. Chrome launch also rejects executables without a valid Google publisher signature.
- Wi-Fi setup imports a profile into the real Windows user account. Its XML password file exists briefly in host TEMP with a protected DACL for the owner, SYSTEM and local Administrators; a same-user malicious process is still outside NOVA's protection.
- NOVA does not automatically sandbox downloads made by Windows, host Chrome, CMD or another host application. Only the explicit file-inspection route uses Windows Sandbox.
- For unknown executable files, use **Inspect in Sandbox**, which disables networking.
- Keep Windows and Microsoft Defender up to date. No sandbox can promise protection against every future hypervisor or OS vulnerability.

## Build

Build computer requirements: Rust 1.95.0, Microsoft C++ Build Tools/MSVC and Windows SDK, plus internet access for dependencies on first build. On a fresh checkout, run `powershell -ExecutionPolicy Bypass -File scripts/prepare-lockfile.ps1` once and commit the generated `Cargo.lock`. When deliberately updating the dependency graph, run `scripts/prepare-lockfile.ps1 -Update` and review the lockfile diff. Then run `BUILD_NOVA.bat` or `scripts/build-portable.ps1` on Windows. The build script tests and compiles `NOVA.exe`, creates the portable folder and writes build metadata plus SHA-256 checksums. It deliberately builds unsigned; signing is a separate post-build step so PFX secrets never enter Cargo or dependency build scripts. `scripts/finalize-portable.ps1` refreshes metadata and the ZIP after a signature is applied. The target USB PC does not need developer tools.

GitHub Actions uses `.github/workflows/rust.yml` on `windows-latest` for pushes, pull requests and manual dispatch. It validates the committed lockfile, runs Clippy and Windows-target tests, builds Release and uploads the USB folder, ZIP and checksum. It has read-only repository permissions and does not modify the branch. Code-signing secrets are exposed only to a dedicated post-build signing step on a push to `main`; the Cargo build and dependency build scripts never receive them. Pull requests and manual runs build unsigned packages. For signed main-branch releases, configure `NOVA_SIGN_CERT_BASE64` (base64-encoded code-signing PFX) and `NOVA_SIGN_CERT_PASSWORD`; without both secrets, the EXE is intentionally unsigned.

## Performance sampling

Run NOVA on a Windows machine, then in PowerShell execute `powershell -ExecutionPolicy Bypass -File scripts/measure-performance.ps1 -DurationSeconds 120`. The script records whole-machine-normalized CPU usage, working set, private memory, handles and thread count once per second to `perf-results/*.csv`. For multiple NOVA instances, pass `-ProcessId`. Compare an idle sample, a sample with System Monitor open and a sample with several workspaces open; do not treat one machine's numbers as universal limits.

## Security policy

Read `SECURITY.md` for the host/Sandbox threat boundaries, release integrity checks and private vulnerability-reporting guidance.

## Source layout

```text
src/app.rs              Windows-style desktop, Start/taskbar and in-shell applications
src/apps.rs             Host Chrome detection, publisher validation and native launcher
src/security_tools.rs   Five GUI tool integrations, detection and scope reminder
src/isolation.rs        Windows Sandbox config, read-only staging and cleanup
src/storage.rs          Portable paths, settings, validation and atomic JSON writes
src/terminal.rs         CMD/Wi-Fi command parsing and background task model
src/trash.rs            Portable NOVA Trash and restore operations
src/unicode.rs          UTF-8 / UTF-16 text decoding
src/unikey.rs           Bundled UniKey discovery, signature validation and launcher
src/wifi.rs             Windows WLAN profile operations and protected TEMP XML
src/windows_process.rs  Windows Job Object, process timeout and bounded output
scripts/Download-UniKey.ps1  Build-time UniKey download and Authenticode verification
scripts/prepare-lockfile.ps1  Controlled Cargo.lock generation/verification
scripts/sign-executable.ps1   Optional post-build PFX-based Authenticode signing
scripts/finalize-portable.ps1  Build provenance, lock hash, checksums and ZIP finalizer
scripts/measure-performance.ps1  Windows CPU/RAM/handle/thread sampler
.github/workflows/      Windows CI, tests, dependency lock checks and packaging
.github/dependabot.yml  Weekly Cargo and Actions update PRs
.cargo/                 Windows static CRT target flags
```

## Documentation

- [Vietnamese user guide](HUONG_DAN_VI.md)
- [Current release notes](RELEASE_NOTES_0.5.7.md)
- [Security policy](SECURITY.md)

## Validation status

The Windows CI workflow is the authoritative compile/test gate for this project. It requires a committed `Cargo.lock`, runs Clippy and the Windows-target test suite (including junction/reparse-point and Job Object descendant-process tests), and only then packages the executable. The editing environment for this source update has no `rustc`, `cargo`, PowerShell or Windows runtime, and cannot reach the Rust distribution servers. Therefore this source archive has only received static TOML/YAML, path-reference, version and ZIP checks here; no compile, Clippy, Rust unit test, Windows integration test, EXE signing or runtime performance result is claimed. Run the lockfile preparation script on Windows and review the GitHub Actions result for the exact commit before treating the EXE as build-verified.
