$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot
$Version = '0.5.0'
$Target = 'x86_64-pc-windows-msvc'

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Build tool '$Name' was not found. Use the GitHub Actions workflow if you do not want a local Rust toolchain."
    }
}

if (-not $IsWindows -and $env:OS -ne 'Windows_NT') {
    throw 'This packager must run on Windows to produce the native Windows executable.'
}
Require-Command 'cargo'
Require-Command 'rustc'

Write-Host '[1/6] Checking pinned Rust toolchain...' -ForegroundColor Cyan
rustc --version
cargo --version

Write-Host '[2/6] Preparing reproducible dependency lockfile...' -ForegroundColor Cyan
$LockPath = Join-Path $ProjectRoot 'Cargo.lock'
$ExpectedVersion = (Select-String -Path (Join-Path $ProjectRoot 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
$RefreshLock = -not (Test-Path $LockPath)
if (-not $RefreshLock) {
    $LockText = Get-Content $LockPath -Raw
    $RootPackage = [regex]::Match($LockText, '(?ms)^\[\[package\]\]\s+name = "nova-native"\s+version = "([^"]+)"')
    if (-not $RootPackage.Success -or $RootPackage.Groups[1].Value -ne $ExpectedVersion) { $RefreshLock = $true }
}
if ($RefreshLock) {
    cargo generate-lockfile
    if ($LASTEXITCODE -ne 0) { throw 'cargo generate-lockfile failed.' }
}

Write-Host '[3/6] Running NOVA unit tests...' -ForegroundColor Cyan
cargo test --locked --target $Target
if ($LASTEXITCODE -ne 0) { throw 'cargo test failed.' }

Write-Host '[4/6] Building NOVA Native (x64 MSVC)...' -ForegroundColor Cyan
cargo build --locked --release --target $Target
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }

$BuiltExe = Join-Path $ProjectRoot "target\$Target\release\NOVA.exe"
if (-not (Test-Path $BuiltExe)) { throw "Expected executable not found: $BuiltExe" }
if (-not (Test-Path (Join-Path $ProjectRoot 'Cargo.lock'))) { throw 'Cargo.lock is missing.' }

Write-Host '[5/6] Preparing the portable USB folder...' -ForegroundColor Cyan
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
Copy-Item (Join-Path $ProjectRoot 'README_PORTABLE.txt') $PackageRoot
New-Item -ItemType Directory -Path (Join-Path $PackageRoot 'scripts') -Force | Out-Null
Copy-Item (Join-Path $ProjectRoot 'scripts\Download-UniKey.ps1') (Join-Path $PackageRoot 'scripts\Download-UniKey.ps1')
Copy-Item (Join-Path $ProjectRoot 'Cargo.lock') (Join-Path $PackageRoot 'Cargo.lock.build-reference')
"NOVA Native $Version`nTarget: $Target`nRust: 1.95.0`nUnicode: UTF-8 editor + system-font fallback + CMD UTF-8 code page`nChrome: host app files read-only in isolated Windows Sandbox`nIsolation: Windows Sandbox required; Windows Home is not supported`nUniKey: optional official signed companion downloaded to USB" | Set-Content (Join-Path $PackageRoot 'BUILD_INFO.txt')

Write-Host '[6/6] Creating ZIP and SHA256...' -ForegroundColor Cyan
New-Item -ItemType Directory -Path $ReleaseDirectory -Force | Out-Null
$ZipPath = Join-Path $ReleaseDirectory "NOVA-OS-Native-v$Version-USB.zip"
if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
Compress-Archive -Path (Join-Path $PackageRoot '*') -DestinationPath $ZipPath -CompressionLevel Optimal
$Checksum = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -Path ($ZipPath + '.sha256') -Value "$Checksum  $(Split-Path -Leaf $ZipPath)" -Encoding ASCII

Write-Host ''
Write-Host '[SUCCESS] NOVA Native package created.' -ForegroundColor Green
Write-Host "EXE: $PackageRoot\NOVA.exe" -ForegroundColor Green
Write-Host "USB folder: $PackageRoot" -ForegroundColor Green
Write-Host "ZIP: $ZipPath" -ForegroundColor Green
Write-Host "SHA256: $($ZipPath).sha256" -ForegroundColor Green
Write-Host 'Copy the complete package folder to a USB drive.' -ForegroundColor Green
