# Build NOVA Native on GitHub (no Rust toolchain on your PC)

Use this route if you only want the finished Windows USB package and do not want to install Rust/Cargo or C++ Build Tools on your own computer. The build happens on a GitHub-hosted Windows runner.

## Steps

1. Sign in to GitHub and create a new repository. A private repository is fine.
2. Upload the **contents** of this NOVA Native source folder to the repository root. Make sure `.github/workflows/windows-build.yml` is included. Do not upload `target/` or `release/` if they exist.
3. Open the repository's **Actions** tab. If GitHub asks, enable Actions for the repository.
4. In the left sidebar select **Build NOVA Native for Windows**.
5. Choose **Run workflow**, select the default branch, and run it.
6. Wait for the workflow to finish. Open its successful run and find **Artifacts** at the bottom.
7. Download `NOVA-OS-Native-v0.3.2-USB`. It is the packaged portable folder, with `NOVA.exe`, `portable.flag`, `README_PORTABLE.txt`, `BUILD_INFO.txt` and `Data/`. GitHub supplies the artifact download as a ZIP archive.
8. Extract the downloaded artifact once. Copy the entire extracted contents to the USB drive. Keep `NOVA.exe`, `portable.flag`, `README_PORTABLE.txt`, and `Data/` together.

## On the target Windows PC

- No GitHub account, internet connection, Node.js, npm, Rust, Cargo, Visual Studio, WebView2, installer, or administrator permission is required to start NOVA.
- Open the USB in File Explorer and double-click `NOVA.exe`. Windows intentionally does not auto-run arbitrary programs when a USB is inserted.
- Windows may show SmartScreen or antivirus warnings for a newly built unsigned executable. Do not bypass a warning unless you trust the source and have checked the artifact. A public release should be code-signed.
- Close NOVA before ejecting the USB.

The initial build is x64 only. The workflow builds and runs unit tests, then packages the EXE and portable data folders. It does not test every real PC, GPU, wireless adapter, or router.
