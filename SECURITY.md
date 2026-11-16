# Security Policy

## Security boundaries

NOVA is a regular Windows desktop application running with the current user's permissions. It is not an operating system, hypervisor, antivirus engine, or security boundary.

- Only the explicit **Inspect in Windows Sandbox** path stages a copy of one NOVA workspace file into a read-only mapped folder with networking and clipboard redirection disabled.
- Terminal/CMD, Google Chrome, UniKey, Wi-Fi management, normal filesystem actions and third-party security GUIs execute on the Windows host.
- The scope acknowledgement in Security Center is a workflow reminder; it does not technically restrict external scanners.
- Data folders reject symlinks and Windows reparse points, and recursive deletion is designed not to traverse them. This does not eliminate every filesystem race or protect against a compromised Windows account, kernel, driver or hypervisor.

## Reporting a vulnerability

Do not publish exploit details, credentials, personal data or weaponized samples in a public issue. Use the repository's **Security → Advisories → Report a vulnerability** private reporting flow when enabled. Otherwise contact the repository maintainers through a private channel and include the affected version, Windows edition/build, reproduction steps and expected versus actual behavior.

Please allow maintainers time to investigate and prepare a fix before public disclosure. Do not test NOVA or its optional external tools against systems without explicit authorization.

## Release integrity

- Use the SHA-256 sidecar generated with each portable ZIP to verify the downloaded archive.
- Release EXE signing is optional and requires a trusted code-signing PFX certificate supplied through `NOVA_SIGN_CERT_BASE64` and `NOVA_SIGN_CERT_PASSWORD`.
- An Authenticode-valid signature indicates publisher provenance and file integrity since signing; it does not guarantee that the software is vulnerability-free.
- The committed `Cargo.lock` and pinned Rust toolchain are required for locked builds. CI has read-only repository permissions and never writes generated dependency files back to the branch.
