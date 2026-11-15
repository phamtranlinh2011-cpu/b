# NOVA Native 0.5.4 — Optimization notes

This release applies two conservative performance patterns:

1. **Lazy initialization:** host executable discovery, system telemetry, quarantine enumeration and security-tool discovery are deferred until the relevant workspace is opened. This avoids paying their startup cost for users who do not use those features.
2. **Session/UI caching:** the Files workspace caches a validated directory snapshot until navigation or explicit refresh; app shortcut glyphs cache egui text layout objects; security-tool discovery caches paths until the user explicitly refreshes detection.

The implementation keeps validation at use boundaries: cached Files entries are still revalidated by the existing canonical-path checks before opening, moving or inspecting files. Cached detection is informational; launch functions retain their own executable and publisher checks.

Relevant upstream documentation:

- egui Painter text layout: https://docs.rs/egui/0.37.0/egui/struct.Painter.html
- egui Galley: https://docs.rs/epaint/0.37.0/epaint/text/struct.Galley.html
- sysinfo 0.40.3: https://docs.rs/sysinfo/0.40.3/sysinfo/
