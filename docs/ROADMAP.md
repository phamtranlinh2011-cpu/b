# NOVA Native roadmap

## 0.3.1 — Native foundation (current source milestone)
- Rust native GUI, no WebView or Node runtime at execution time.
- Portable data folder and settings.
- Built-in Files, Editor, Calculator, Settings, Monitor.
- CMD terminal and Wi-Fi command flow.
- Windows ZIP packaging scripts and CI workflow.

## 0.3.1 — Windows integration hardening
- Replace temporary Wi-Fi XML file with direct WLAN API calls.
- More robust SSID parsing for non-English Windows locales.
- Better CMD encoding handling for OEM code pages.
- Explicit read-only USB behavior and safe-eject warning.
- Add tests for path traversal, profile lifecycle, and process timeout.

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
