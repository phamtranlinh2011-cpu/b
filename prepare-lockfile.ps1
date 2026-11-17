[CmdletBinding()]
param(
    [switch]$Update
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

foreach ($tool in @('cargo', 'rustc')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Required tool '$tool' was not found. Install Rust 1.95.0 using rustup, then retry."
    }
}

$rustVersion = (& rustc --version).Trim()
if ($LASTEXITCODE -ne 0 -or $rustVersion -notmatch '^rustc 1\.95\.0(?:\s|$)') {
    throw "NOVA requires rustc 1.95.0. Detected: $rustVersion"
}

$lockPath = Join-Path $ProjectRoot 'Cargo.lock'
if (-not (Test-Path $lockPath) -or $Update) {
    Write-Host '[NOVA] Resolving and writing Cargo.lock...' -ForegroundColor Cyan
    & cargo generate-lockfile
    if ($LASTEXITCODE -ne 0) { throw 'cargo generate-lockfile failed.' }
}

Write-Host '[NOVA] Verifying that Cargo.lock matches Cargo.toml...' -ForegroundColor Cyan
& cargo metadata --locked --format-version 1 --no-deps | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cargo.lock is missing or stale. Run scripts/prepare-lockfile.ps1 -Update.' }

$toml = Get-Content (Join-Path $ProjectRoot 'Cargo.toml') -Raw
$versionMatch = [regex]::Match($toml, '(?m)^version\s*=\s*"([^"]+)"')
$lockText = Get-Content $lockPath -Raw
$rootMatch = [regex]::Match($lockText, '(?ms)^\[\[package\]\]\s+name = "nova-native"\s+version = "([^"]+)"')
if (-not $versionMatch.Success -or -not $rootMatch.Success -or $versionMatch.Groups[1].Value -ne $rootMatch.Groups[1].Value) {
    throw 'The nova-native entry in Cargo.lock does not match Cargo.toml. Run scripts/prepare-lockfile.ps1 -Update.'
}

Write-Host ''
Write-Host "[SUCCESS] Cargo.lock is valid for NOVA $($versionMatch.Groups[1].Value)." -ForegroundColor Green
Write-Host 'Commit Cargo.lock to Git so CI and other developer machines use the same dependency graph.' -ForegroundColor Yellow
