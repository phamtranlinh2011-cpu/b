# NOVA Native roadmap

## 0.3.x — Native foundation and hardening (previous milestones)
- Rust native GUI, portable Data directory, Files/Editor/Calculator/Settings/Monitor, CMD and Wi-Fi integration.
- Fix MSVC build errors, harden file paths and settings writes, correct WPA2 raw PSK validation, background terminal tasks and Windows Job Object cleanup.
- Protected WLAN TEMP XML, stale-file cleanup and bounded output/history.

## 0.4.0 — Desktop UX, applications and portable Trash (completed previous milestone)
- Searchable Launcher, app tiles, navigation rail, floating in-shell windows and bottom dock/taskbar toggle.
- Visual design refresh: consistent spacing, rounded surfaces, theme/accent colors, window shadows, hover feedback and short fade transitions.
- Reduce visual effects setting for less motion.
- Files selection/double-click open, refresh, folder creation and Move to NOVA Trash.
- Portable NOVA Trash with restore, collision-safe names, permanent delete and confirmation before empty.
- Editor New/Save and word/character count; Calculator Clear/Swap; System Monitor updates only while visible.
- Google Chrome host launcher with install detection, new-window launch and official download-page fallback.
- Docs and CI artifact naming updated to v0.4.0.

## 0.4.x — Quality and native integration
- Execute Windows CI and physical PC test matrix (Windows 10/11, high DPI, Intel/AMD/NVIDIA GPU, USB flash drive, read-only and near-full drive).
- Test process termination, nested Job Objects, repeated CMD commands, slow/large output and application shutdown.
- Test actual WLAN profiles, locale-specific `netsh` output, adapters, WPA2/open networks and cleanup after forced process termination.
- Consider Windows Shell Recycle Bin integration as an optional mode distinct from NOVA Trash.
- Add keyboard shortcuts, accessibility review, focus order, AccessKit/screen-reader testing and high contrast.
- Consider separate native app viewports if window ownership/lifecycle and resource use are validated.

## 0.5.0 — Unicode, Vietnamese input and disposable isolation (completed foundation)
- UTF-8 and UTF-16 LE/BE text decoding; UTF-8 saves; eframe system-font fallback for missing glyphs.
- CMD requests UTF-8 code page before user commands; legacy OEM/ANSI utilities may still display incorrectly.
- UniKey 4.6 RC2 x64 companion download from the official site, strict Authenticode publisher check and USB-side placement; the helper script is embedded in NOVA at compile time.
- Offline Windows Sandbox profile for inspecting a staged read-only copy of one selected NOVA file; sample is not auto-executed.
- Isolated Chrome profile inside Windows Sandbox with a read-only app mapping, no writable host folder or clipboard redirection, and networking enabled for browsing.
- Security Center explains host/Sandbox boundaries and clears NOVA staging files/configs after Sandbox is closed.
- Fail-closed behavior: no fallback to host execution when Windows Sandbox is unavailable.
- Startup creates the Data tree component-by-component and fails closed on symlinks, junctions and reparse points in the core data folders; Trash/Security roots revalidate on use.
- UniKey helper also rejects reparse-point Apps/UniKey destinations and stages the verified executable before replacing the final file.
- Workflow uses pinned Rust 1.95.0 and locked Cargo builds; the first CI run generates/commits Cargo.lock if missing or stale.

## 0.5.x — Runtime verification and hardening
- Verify all unit tests and release build on Windows CI for the exact release commit.
- Test Sandbox Pro/Enterprise/Education edition detection, optional feature installation, one-instance limitation, file inspection and isolated Chrome on physical Windows 10/11 devices.
- Evaluate Defender scan integration and Windows Shell Recycle Bin integration as separate capabilities.
- Review process shutdown, Job Object behavior, USB read-only/near-full conditions and Unicode paths with non-ASCII usernames.
- Add signed NOVA releases, reproducible build provenance and an explicit update/rollback strategy that preserves portable user data.

## Future — NOVA Live USB
- Separate bootable Linux-based Live image, persistence layer, UEFI and Secure Boot validation. Do not describe the Windows desktop executable as a bootable OS.


## 0.5.1 — Security Center GUI prototype (current source release)

- Five launch workspaces: Zenmap, Greenbone GSA, OWASP ZAP, Wireshark and Burp Suite.
- Authorized-scope acknowledgement and third-party host-execution warnings.
- Third-party engines are optional and separately installed; Greenbone needs its own server/scanner.
- The scope acknowledgement is not technical enforcement. Local GUI tools run as Windows host processes and are not isolated by NOVA.
- This remains a portable Windows desktop prototype, not the Linux bootable Security OS.

## Future 0.6.0 — Linux Bootable Foundation

- Start the separate Debian Live-based Security OS branch.
- Port the NOVA shell and Security Center UI to the Linux environment.
- Design actual firewall-enforced network domains, disposable VM/lab environments, signed modules and encrypted persistence before integrating scanners.
