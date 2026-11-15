# NOVA Native 0.5.6 — build status

- Source package version: `0.5.6`; expected Windows artifact: `NOVA-OS-Native-v0.5.6-USB`.
- UI audit follow-up: taskbar now exposes all supported workspace windows and scrolls horizontally when space is limited; icon galley cache keys include display scale to avoid stale sizing after DPI changes.
- Search hint now accurately describes the implemented app/settings search; the right-side panel is labeled Quick Settings / recent activity rather than implying Windows notification integration.
- Static source and manifest checks are possible in this environment, but Rust/Cargo and a Windows GUI runtime are unavailable here. Windows `cargo test`, Release build, high-DPI, resize, theme and real-device checks must be confirmed by GitHub Actions/on-device testing.
