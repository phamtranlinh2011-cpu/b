# NOVA Security Center — GUI Tool Integration Notes

## Product boundary

This release provides a GUI-first Security Center and launch workspaces. It is not a complete reimplementation of the external scanning engines and does not bundle them. The five integrations are:

| NOVA workspace | External application/service | Role |
|---|---|---|
| Network Discovery | Zenmap / Nmap | Visual network discovery and saved scan profiles |
| Vulnerability Management | Greenbone Security Assistant (GSA) | Web front end to a separately installed Greenbone Vulnerability Management service |
| Web Security | OWASP ZAP | Desktop workspace for proxy history, passive alerts and authorized web testing |
| Packet Analyzer | Wireshark | GUI protocol analysis of capture files or permitted interfaces |
| Web/API Inspector | Burp Suite Community or Professional | Manual web/API inspection and proxy tools |

## Scope guard

The Security Center asks the operator to enter a scope and explicitly confirm ownership or written authorization. NOVA does not inject or enforce this scope inside the third-party applications. The operator must also confirm the target list in each external tool; a click in NOVA is not proof of ownership or authorization. Before running any tool, the operator must configure the same target allowlist in that tool. Active tests must be limited to systems the operator is authorized to assess.

The scope guard is a safety reminder, not an authorization-verification service or a network firewall. Third-party tools are launched on the Windows host and are not placed inside Windows Sandbox by this release.

## Installation and detection

NOVA checks known default Windows install folders (including the standard Nmap/Zenmap and OWASP ZAP locations) and optional `Apps/` folders beside `NOVA.exe`. If an application was installed to a custom directory, install it in a documented default location or place a portable copy under the matching `Apps/` directory. It does not download or install Nmap/Zenmap, Greenbone, ZAP, Wireshark or Burp. Install them from their official project sources and use their own update mechanisms. Greenbone is a server-based scanner plus GSA web interface; the server, scanner, database and vulnerability feed are external to NOVA.

## Official references

- Zenmap (official Nmap GUI): https://nmap.org/zenmap/
- Nmap Windows installation and Zenmap availability: https://nmap.org/book/inst-windows.html
- Greenbone architecture and GSA: https://greenbone.github.io/docs/latest/architecture.html
- Greenbone Security Assistant frontend: https://github.com/greenbone/gsa
- OWASP ZAP desktop guide: https://www.zaproxy.org/docs/desktop/
- OWASP ZAP desktop UI: https://www.zaproxy.org/docs/desktop/ui/
- Wireshark User's Guide: https://www.wireshark.org/docs/wsug_html_chunked/
- Wireshark GUI manual: https://www.wireshark.org/docs/man-pages/wireshark.html
- Burp Suite Community/Professional getting started: https://portswigger.net/burp/documentation/desktop/getting-started
- Burp Suite installation: https://portswigger.net/burp/documentation/desktop/getting-started/download-and-install
