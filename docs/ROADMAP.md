# NOVA Native roadmap

## 0.3.1 — Native foundation (previous source milestone)
- Rust native GUI, no WebView or Node runtime at execution time.
- Portable data folder and settings.
- Built-in Files, Editor, Calculator, Settings, Monitor.
- CMD terminal and Wi-Fi command flow.
- Windows ZIP packaging scripts and CI workflow.

## 0.3.2 — Windows build and safety fixes (previous)
- Fix Windows build errors reported by the MSVC runner.
- Harden Files/Editor path handling and portable Windows filename validation.
- Fail closed on corrupt Wi-Fi profile manifests and improve temporary XML cleanup.
- Improve CMD output/timeout diagnostics and calculator precision.
- Add unit tests for path validation, Wi-Fi parsing/profile helpers and calculator formatting.

## 0.3.3 — CI test corrections (previous)
- Fix trailing-space filename validation so validation sees the original input.
- Correct the WPA2 password test to allow a valid 64-character hexadecimal raw PSK.
- Remove unused AppPaths fields that generated a dead-code warning.

## 0.3.5 — Responsive terminal and process security (current)
- Run CMD and Windows WLAN operations on a background worker rather than the UI thread.
- Use a shared process runner with bounded output, timeouts and Windows Job Object cleanup.
- Create temporary WLAN XML with a protected DACL, remove it after import, and clean stale protected files at startup.
- Pin Rust 1.95.0 and persist a generated Cargo.lock before CI test/build; use `--locked` after first resolution.
- Validate on Windows CI and real adapters; nested Job Object restrictions and actual WLAN import remain platform integration checks.

## 0.4.0 — Desktop polish
- Independent app viewports / improved window manager.
- Search and file previews.
- Keyboard shortcuts, clipboard and notifications.
- Accessibility review and high-DPI testing.

## 0.5.0 — Performance and release validation
- Cold/warm launch benchmarks on HDD, SSD and USB flash drives.
- Idle CPU and RAM baselines on Windows 10/11.
- Test matrix across hardware and Wi-Fi adapters.
- Signed release process and update strategy.

## Future — NOVA Live USB
- Separate Linux Live image, persistence layer, UEFI and Secure Boot validation.
- Share visual design and selected apps; do not claim the Windows desktop executable is a bootable OS.
