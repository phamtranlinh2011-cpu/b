# NOVA Native v0.5.7 — Audit Phase 4 (CI, Performance & Release)

## Changes

1. CI now runs `cargo fmt --all -- --check` before Clippy/build.
2. CI parses every `scripts/**/*.ps1` with the built-in PowerShell AST parser; this is a syntax validation step, not a full PSScriptAnalyzer/style audit.
3. CI starts the packaged NOVA executable for a short startup smoke test and terminates it after the observation window. This does not validate interactive GUI workflows, hardware integrations, or long-running stability.
4. Trusted pushes to `main` now require both signing secrets. The signing step fails if they are absent, and finalization has `-RequireSignedExecutable` to reject unsigned/invalid NOVA binaries. PR/local packages may remain unsigned development artifacts.
5. Documentation explicitly states Windows x64 is the supported target, explains Cargo.lock expectations, and describes how to collect performance samples.
6. Performance sampling remains opt-in and records CPU, working set, private memory, handles, and threads. No fixed CI thresholds are imposed because runner hardware and GUI load vary.

## Deliberate limitations

- Windows ARM64 is not enabled: the toolchain, CI target, package naming, and release artifact are x64-specific. Adding ARM64 requires validating eframe/glow, sysinfo, all Windows FFI, UniKey availability, and portable packaging on native ARM64 runners.
- The source ZIP intentionally excludes Cargo.lock. The Git repository must retain and commit the lockfile; CI rejects missing/stale lockfiles.
- The startup smoke test is not a substitute for manual UI smoke testing on Windows 10/11 hardware.
- PowerShell AST parsing checks syntax only; it does not execute the scripts or guarantee runtime correctness.

## Checks performed in this environment

- `Cargo.toml` and `rust-toolchain.toml` parsed with Python `tomllib`.
- GitHub Actions and Dependabot YAML parsed with PyYAML.
- CI policy assertions confirmed formatting, PowerShell parser, Clippy, startup smoke test, and signed-finalization gates are present.
- Finalizer contains an explicit Authenticode `Valid` requirement when `-RequireSignedExecutable` is passed.
- Rust toolchain, PowerShell, Windows runtime, and GitHub Actions were unavailable here; `cargo fmt`, Clippy, tests, build, PowerShell AST parsing, and the startup smoke test were not executed.
