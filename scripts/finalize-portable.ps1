[CmdletBinding()]
param(
    [string]$PackageDirectory = ''
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

$CargoToml = Get-Content (Join-Path $ProjectRoot 'Cargo.toml') -Raw
$VersionMatch = [regex]::Match($CargoToml, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $VersionMatch.Success) { throw 'Could not read the NOVA package version from Cargo.toml.' }
$Version = $VersionMatch.Groups[1].Value
$Target = 'x86_64-pc-windows-msvc'

if ([string]::IsNullOrWhiteSpace($PackageDirectory)) {
    $PackageDirectory = Join-Path $ProjectRoot "release\NOVA-OS-Native-v$Version-USB"
}
$PackageRoot = (Resolve-Path -LiteralPath $PackageDirectory).Path
$ExePath = Join-Path $PackageRoot 'NOVA.exe'
if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
    throw "NOVA.exe was not found in package directory: $PackageRoot"
}

$Commit = 'unknown (source is not a Git checkout)'
if (Get-Command 'git' -ErrorAction SilentlyContinue) {
    $GitCommit = & git rev-parse HEAD 2>$null
    if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($GitCommit)) { $Commit = $GitCommit.Trim() }
}
$DetectedRust = if (Get-Command 'rustc' -ErrorAction SilentlyContinue) { (& rustc --version).Trim() } else { 'unknown (rustc unavailable during finalization)' }
$Signature = Get-AuthenticodeSignature -FilePath $ExePath
$Signer = if ($Signature.SignerCertificate) { $Signature.SignerCertificate.GetNameInfo('SimpleName', $false) } else { 'Unsigned' }
$BuildTime = [DateTime]::UtcNow.ToString('o')
$ExeHash = (Get-FileHash -LiteralPath $ExePath -Algorithm SHA256).Hash.ToLowerInvariant()
$UniKeyPath = Join-Path $PackageRoot 'Apps\UniKey\UniKeyNT.exe'
if (-not (Test-Path -LiteralPath $UniKeyPath -PathType Leaf)) { throw 'Bundled UniKeyNT.exe is missing from Apps/UniKey.' }
$UniKeySignature = Get-AuthenticodeSignature -FilePath $UniKeyPath
if ($UniKeySignature.Status -ne 'Valid' -or $UniKeySignature.SignerCertificate.GetNameInfo('SimpleName', $false) -cne 'PHAM KIM LONG') {
    throw 'Bundled UniKey failed Authenticode publisher verification during finalization.'
}
$UniKeyHash = (Get-FileHash -LiteralPath $UniKeyPath -Algorithm SHA256).Hash.ToLowerInvariant()
$LockPath = Join-Path $ProjectRoot 'Cargo.lock'
if (-not (Test-Path -LiteralPath $LockPath -PathType Leaf)) { throw 'Cargo.lock is required to finalize a reproducible release.' }
$LockHash = (Get-FileHash -LiteralPath $LockPath -Algorithm SHA256).Hash.ToLowerInvariant()

@"
Product: NOVA Native
Version: $Version
Packaged UTC: $BuildTime
Commit: $Commit
Target: $Target
Rust: $DetectedRust
Renderer: eframe/egui Glow
Executable SHA-256: $ExeHash
Bundled companion: UniKey 4.6 RC2 x64 (official publisher signature verified)
UniKey Authenticode status: $($UniKeySignature.Status)
UniKey publisher: $($UniKeySignature.SignerCertificate.GetNameInfo('SimpleName', $false))
UniKey SHA-256: $UniKeyHash
Cargo.lock SHA-256: $LockHash
Authenticode status: $($Signature.Status)
Authenticode signer: $Signer
Portable mode: enabled by portable.flag; data stays under Data/
Windows Sandbox: optional Windows feature; only explicitly opened routes use it
External security tools: optional, separately installed; not bundled or sandboxed by NOVA
"@ | Set-Content (Join-Path $PackageRoot 'BUILD_INFO.txt') -Encoding UTF8
@(
    "$ExeHash  NOVA.exe",
    "$UniKeyHash  Apps/UniKey/UniKeyNT.exe"
) | Set-Content (Join-Path $PackageRoot 'CHECKSUMS.sha256') -Encoding ASCII

$ReleaseDirectory = Split-Path -Parent $PackageRoot
New-Item -ItemType Directory -Path $ReleaseDirectory -Force | Out-Null
$ZipPath = Join-Path $ReleaseDirectory "NOVA-OS-Native-v$Version-USB.zip"
if (Test-Path -LiteralPath $ZipPath) { Remove-Item -LiteralPath $ZipPath -Force }
Compress-Archive -Path (Join-Path $PackageRoot '*') -DestinationPath $ZipPath -CompressionLevel Optimal
$ZipHash = (Get-FileHash -LiteralPath $ZipPath -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -Path ($ZipPath + '.sha256') -Value "$ZipHash  $(Split-Path -Leaf $ZipPath)" -Encoding ASCII

Write-Host ''
Write-Host '[SUCCESS] Portable package metadata and archive refreshed.' -ForegroundColor Green
Write-Host "EXE: $ExePath" -ForegroundColor Green
Write-Host "Authenticode: $($Signature.Status) ($Signer)" -ForegroundColor Green
Write-Host "USB folder: $PackageRoot" -ForegroundColor Green
Write-Host "ZIP: $ZipPath" -ForegroundColor Green
Write-Host "SHA256: $($ZipPath).sha256" -ForegroundColor Green
if ($Signature.Status -ne 'Valid') {
    Write-Warning 'This executable is not Authenticode-verified. Configure NOVA signing secrets for trusted main-branch releases.'
}
