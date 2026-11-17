[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot
$Target = 'x86_64-pc-windows-msvc'
$RustVersion = '1.95.0'

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Build tool '$Name' was not found. Install Rust $RustVersion and MSVC Build Tools, or use GitHub Actions."
    }
}

if (-not $IsWindows -and $env:OS -ne 'Windows_NT') {
    throw 'This packager must run on Windows to produce the native Windows executable.'
}
Require-Command 'cargo'
Require-Command 'rustc'

$CargoToml = Get-Content (Join-Path $ProjectRoot 'Cargo.toml') -Raw
$VersionMatch = [regex]::Match($CargoToml, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $VersionMatch.Success) { throw 'Could not read the NOVA package version from Cargo.toml.' }
$Version = $VersionMatch.Groups[1].Value

$DetectedRust = (& rustc --version).Trim()
if ($LASTEXITCODE -ne 0 -or $DetectedRust -notmatch "^rustc $([regex]::Escape($RustVersion))(?:\s|$)") {
    throw "NOVA requires rustc $RustVersion. Detected: $DetectedRust"
}

Write-Host '[1/7] Checking pinned Rust toolchain...' -ForegroundColor Cyan
& rustc --version
& cargo --version

Write-Host '[2/7] Preparing the dependency lockfile...' -ForegroundColor Cyan
$LockPath = Join-Path $ProjectRoot 'Cargo.lock'
if (-not (Test-Path $LockPath)) {
    Write-Warning 'Cargo.lock is absent. Generating it once on this developer machine. Commit the new Cargo.lock before pushing.'
    & cargo generate-lockfile
    if ($LASTEXITCODE -ne 0) { throw 'cargo generate-lockfile failed.' }
}
$LockText = Get-Content $LockPath -Raw
$RootPackage = [regex]::Match($LockText, '(?ms)^\[\[package\]\]\s+name = "nova-native"\s+version = "([^"]+)"')
if (-not $RootPackage.Success -or $RootPackage.Groups[1].Value -ne $Version) {
    throw "Cargo.lock does not match nova-native $Version. Run scripts/prepare-lockfile.ps1 -Update, then commit Cargo.lock."
}
& cargo metadata --locked --format-version 1 --no-deps | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cargo.lock is invalid or stale. Run scripts/prepare-lockfile.ps1 -Update.' }

Write-Host '[3/7] Running Windows-target unit and integration tests...' -ForegroundColor Cyan
& cargo test --locked --target $Target
if ($LASTEXITCODE -ne 0) { throw 'cargo test failed.' }

Write-Host '[4/7] Building NOVA Native (x64 MSVC Release)...' -ForegroundColor Cyan
& cargo build --locked --release --target $Target
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }

$BuiltExe = Join-Path $ProjectRoot "target\$Target\release\NOVA.exe"
if (-not (Test-Path $BuiltExe)) { throw "Expected executable not found: $BuiltExe" }

Write-Host '[5/7] Preparing the portable USB folder...' -ForegroundColor Cyan
$ReleaseDirectory = Join-Path $ProjectRoot 'release'
$PackageRoot = Join-Path $ReleaseDirectory "NOVA-OS-Native-v$Version-USB"
if (Test-Path $PackageRoot) { Remove-Item $PackageRoot -Recurse -Force }
$Folders = @(
    'Data\Users\Default\Desktop',
    'Data\Users\Default\Documents',
    'Data\Users\Default\Downloads',
    'Data\Settings',
    'Data\Logs',
    'Data\Trash\Items',
    'Data\Trash\Meta',
    'Data\Security\Quarantine'
)
foreach ($Folder in $Folders) {
    New-Item -ItemType Directory -Path (Join-Path $PackageRoot $Folder) -Force | Out-Null
}
Copy-Item $BuiltExe (Join-Path $PackageRoot 'NOVA.exe')
Copy-Item (Join-Path $ProjectRoot 'portable.flag') $PackageRoot

@"
NOVA Native $Version - QUICK START

1. Keep NOVA.exe, portable.flag, QUICK_START.txt, BUILD_INFO.txt, Apps/UniKey and Data together on the USB drive.
2. Open NOVA.exe on Windows 10/11 x64.
3. Windows Sandbox requires a supported Windows edition with the feature enabled and hardware virtualization available.
4. NOVA itself runs on the Windows host; only explicitly opened Sandbox tasks are isolated.
5. Close NOVA and all Windows Sandbox windows before safely removing the USB drive.
6. Data, settings, Trash and security staging are stored below Data/ in portable mode.
7. UniKey 4.6 RC2 x64 is bundled in Apps/UniKey and starts when you open UniKey in NOVA.

Inspect the third-party notices and security boundaries in README.md and HUONG_DAN_VI.md in the source repository.
"@ | Set-Content (Join-Path $PackageRoot 'QUICK_START.txt') -Encoding UTF8

Write-Host '[6/7] Bundling official UniKey 4.6 RC2 x64...' -ForegroundColor Cyan
$PreviousNovaAppRoot = $env:NOVA_APP_ROOT
try {
    $env:NOVA_APP_ROOT = $PackageRoot
    & (Join-Path $PSScriptRoot 'Download-UniKey.ps1')
    if (-not (Test-Path -LiteralPath (Join-Path $PackageRoot 'Apps\UniKey\UniKeyNT.exe') -PathType Leaf)) {
        throw 'UniKey packaging completed without producing Apps/UniKey/UniKeyNT.exe.'
    }
}
finally {
    if ($null -eq $PreviousNovaAppRoot) { Remove-Item Env:NOVA_APP_ROOT -ErrorAction SilentlyContinue }
    else { $env:NOVA_APP_ROOT = $PreviousNovaAppRoot }
}

Copy-Item (Join-Path $ProjectRoot 'THIRD_PARTY_NOTICES.md') (Join-Path $PackageRoot 'THIRD_PARTY_NOTICES.txt')

Write-Host '[7/7] Finalizing build metadata, checksums and portable ZIP...' -ForegroundColor Cyan
& (Join-Path $PSScriptRoot 'finalize-portable.ps1') -PackageDirectory $PackageRoot
if (-not $?) { throw 'Portable package finalization failed.' }
Write-Host '[SUCCESS] Unsigned portable package created. Trusted release signing, when configured, runs in a separate post-build step.' -ForegroundColor Green
