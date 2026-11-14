# NOVA Native roadmap

## 0.3.1 — Native foundation (previous source milestone)
- Rust native GUI, no WebView or Node runtime at execution time.
- Portable data folder and settings.
- Built-in Files, Editor, Calculator, Settings, Monitor.
- CMD terminal and Wi-Fi command flow.
- Windows ZIP packaging scripts and CI workflow.

## 0.3.2 — Windows build and safety fixes (current)
- Fix Windows build errors reported by the MSVC runner.
- Harden Files/Editor path handling and portable Windows filename validation.
- Fail closed on corrupt Wi-Fi profile manifests and improve temporary XML cleanup.
- Improve CMD output/timeout diagnostics and calculator precision.
- Add unit tests for path validation, Wi-Fi parsing/profile helpers and calculator formatting.

## 0.3.3 — Windows integration hardening
- Replace temporary Wi-Fi XML file with direct WLAN API calls or a securely ACL-restricted temporary file.
- More robust SSID parsing for non-English Windows locales.
- Better CMD encoding handling for OEM code pages.
- Windows Job Object process-tree management and bounded pipe-drain behavior.
- Profile lifecycle, timeout and USB persistence integration tests on real Windows hardware.

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
