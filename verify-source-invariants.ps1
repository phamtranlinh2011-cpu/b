$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$SourcePath = Join-Path $ProjectRoot 'src\app.rs'
if (-not (Test-Path -LiteralPath $SourcePath -PathType Leaf)) {
    throw "Rust application source was not found: $SourcePath"
}
$Source = Get-Content -LiteralPath $SourcePath -Raw

$RequiredPatterns = @(
    @{ Pattern = 'let worker_ssid\s*=\s*ssid\.clone\(\)\s*;'; Description = 'Wi-Fi worker must own a cloned SSID' },
    @{ Pattern = 'wifi::connect\(&paths,\s*&worker_ssid,'; Description = 'Wi-Fi worker must connect using its owned SSID' },
    @{ Pattern = 'let mut files_open\s*=\s*self\.files_open\s*;'; Description = 'File Manager must use a local open flag' },
    @{ Pattern = 'window\.open\(&mut files_open\)'; Description = 'File Manager window must borrow the local open flag' }
    @{ Pattern = 'self\.files_open\s*=\s*files_open\s*;'; Description = 'File Manager must copy the local open flag back to application state' }
)
foreach ($Check in $RequiredPatterns) {
    if ($Source -notmatch $Check.Pattern) {
        throw "Source invariant failed: $($Check.Description). Update src/app.rs from the current NOVA source package."
    }
}

if ($Source -match '\.open\(&mut self\.[A-Za-z_][A-Za-z0-9_]*\)') {
    throw 'A NOVA window still borrows a self.* open flag directly. Use a local bool around Window::show to avoid overlapping closure borrows.'
}

Write-Host 'NOVA Rust ownership/borrow source invariants passed.' -ForegroundColor Green
