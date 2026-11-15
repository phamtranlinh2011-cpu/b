# NOVA 0.5.1 — GUI Security Tool References

This note records the upstream documentation used for the NOVA Security Center launch workspaces. It does not claim that NOVA implements the external scanner engines.

| Workspace | Official reference | What NOVA uses it for |
|---|---|---|
| Network Discovery | https://nmap.org/zenmap/ | Zenmap is the official GUI for Nmap, with saved profiles and searchable scan results. |
| Network Discovery on Windows | https://nmap.org/book/inst-windows.html | Windows installation paths, the default Nmap folder and the optional Zenmap component. |
| Vulnerability Management | https://greenbone.github.io/docs/latest/architecture.html | GSA is the web UI; gvmd, scanner, database and services run separately. |
| Greenbone GSA frontend | https://github.com/greenbone/gsa | GSA is a web frontend that connects to the Greenbone service stack. |
| Web Security | https://www.zaproxy.org/docs/desktop/ | ZAP desktop workspace and beginner-to-advanced web testing workflows. |
| ZAP UI | https://www.zaproxy.org/docs/desktop/ui/ | Workspace, request/response, history and alerts panels. |
| ZAP Windows installation path | https://www.zaproxy.org/faq/where-is-zap-installed/ | Common Program Files installation folders used by NOVA detection. |
| Packet Analyzer | https://www.wireshark.org/docs/wsug_html_chunked/ | Wireshark User's Guide for GUI protocol analysis. |
| Wireshark GUI | https://www.wireshark.org/docs/man-pages/wireshark.html | Capture/display filters and interactive GUI behavior. |
| Web/API Inspector | https://portswigger.net/burp/documentation/desktop/getting-started | Burp's proxy, target scope, repeater and report workflows. |
| Burp installation | https://portswigger.net/burp/documentation/desktop/getting-started/download-and-install | Official installation and Community/Professional options. |
| egui 0.37 identity API | https://docs.rs/egui/latest/egui/id/struct.Id.html | `Id::unique` is the current alternative for stable IDs where `Id::new` is deprecated. |
| egui windows | https://docs.rs/egui/latest/egui/containers/struct.Window.html | `Window::open` borrows the open-state boolean; NOVA defers calls that need `&mut self` until after the UI closure. |

## Security boundaries

- NOVA detects and opens the external GUI programs; it does not bundle their binaries or silently install them.
- The target-scope acknowledgement is a workflow reminder. It does not technically enforce the target allowlist inside Nmap, ZAP, Wireshark, Burp or Greenbone.
- The four desktop applications are launched as Windows host processes. Greenbone is opened in the registered host browser. These launchers are not Windows Sandbox isolation.
- Use the tools only against assets that you own or have explicit authorization to assess. Configure the same allowlist inside each external tool before any active test.
