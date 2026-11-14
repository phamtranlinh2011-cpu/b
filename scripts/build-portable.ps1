$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

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

Write-Host '[1/5] Checking Rust toolchain...' -ForegroundColor Cyan
rustc --version
cargo --version

Write-Host '[2/5] Running NOVA unit tests...' -ForegroundColor Cyan
cargo test --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'cargo test failed.' }

Write-Host '[3/5] Building NOVA Native (x86_64-pc-windows-msvc)...' -ForegroundColor Cyan
cargo build --release --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }

$BuiltExe = Join-Path $ProjectRoot 'target\x86_64-pc-windows-msvc\release\NOVA.exe'
if (-not (Test-Path $BuiltExe)) { throw "Expected executable not found: $BuiltExe" }

Write-Host '[4/5] Preparing the portable USB folder...' -ForegroundColor Cyan
$ReleaseDirectory = Join-Path $ProjectRoot 'release'
$PackageRoot = Join-Path $ReleaseDirectory 'NOVA-Portable'
if (Test-Path $PackageRoot) { Remove-Item $PackageRoot -Recurse -Force }
New-Item -ItemType Directory -Path $PackageRoot -Force | Out-Null
foreach ($Folder in @('Data\Users\Default\Desktop','Data\Users\Default\Documents','Data\Users\Default\Downloads','Data\Settings','Data\Logs','Backup')) {
    New-Item -ItemType Directory -Path (Join-Path $PackageRoot $Folder) -Force | Out-Null
}
Copy-Item $BuiltExe (Join-Path $PackageRoot 'NOVA.exe')
Copy-Item (Join-Path $ProjectRoot 'portable.flag') $PackageRoot
Copy-Item (Join-Path $ProjectRoot 'README_PORTABLE.txt') $PackageRoot

Write-Host '[5/5] Creating ZIP and SHA256...' -ForegroundColor Cyan
$ZipPath = Join-Path $ReleaseDirectory 'NOVA-OS-Native-v0.3.2-USB.zip'
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
Write-Host 'Copy the complete NOVA-Portable folder to a USB drive.' -ForegroundColor Green
