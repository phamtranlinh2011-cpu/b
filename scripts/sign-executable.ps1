[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath
)

$ErrorActionPreference = 'Stop'
$certificateBase64 = $env:NOVA_SIGN_CERT_BASE64
$certificatePassword = $env:NOVA_SIGN_CERT_PASSWORD

if ([string]::IsNullOrWhiteSpace($certificateBase64) -and [string]::IsNullOrWhiteSpace($certificatePassword)) {
    Write-Warning 'Code-signing certificate is not configured. The executable will remain unsigned.'
    exit 0
}
if ([string]::IsNullOrWhiteSpace($certificateBase64) -or [string]::IsNullOrWhiteSpace($certificatePassword)) {
    throw 'Both NOVA_SIGN_CERT_BASE64 and NOVA_SIGN_CERT_PASSWORD must be configured.'
}

$resolvedFile = (Resolve-Path -LiteralPath $FilePath).Path
$windowsKits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$signtool = Get-Command 'signtool.exe' -ErrorAction SilentlyContinue
if ($signtool) {
    $signtoolPath = $signtool.Source
} else {
    $candidates = @(Get-ChildItem -Path (Join-Path $windowsKits '*\x64\signtool.exe') -ErrorAction SilentlyContinue |
        Sort-Object { [version]$_.Directory.Parent.Name } -Descending)
    if ($candidates.Count -eq 0) { throw 'signtool.exe was not found. Install the Windows SDK with SignTool.' }
    $signtoolPath = $candidates[0].FullName
}

$certificatePath = Join-Path $env:TEMP "nova-signing-$PID.pfx"
try {
    $certificateBytes = [Convert]::FromBase64String($certificateBase64)
    [IO.File]::WriteAllBytes($certificatePath, $certificateBytes)
    & $signtoolPath sign /fd SHA256 /f $certificatePath /p $certificatePassword /tr 'http://timestamp.digicert.com' /td SHA256 $resolvedFile
    if ($LASTEXITCODE -ne 0) { throw "signtool sign failed with exit code $LASTEXITCODE." }

    & $signtoolPath verify /pa /all $resolvedFile
    if ($LASTEXITCODE -ne 0) { throw "signtool verification failed with exit code $LASTEXITCODE." }
    Write-Host "[SUCCESS] Authenticode signature verified: $resolvedFile" -ForegroundColor Green
} finally {
    if (Test-Path -LiteralPath $certificatePath) {
        Remove-Item -LiteralPath $certificatePath -Force
    }
}
