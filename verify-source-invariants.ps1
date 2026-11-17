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
    @{ Pattern = 'window\.open\(&mut files_open\)'; Description = 'File Manager window must borrow the local open flag' },
    @{ Pattern = 'self\.files_open\s*=\s*files_open\s*;'; Description = 'File Manager must copy the local open flag back to application state' },
    @{ Pattern = 'Some\(Err\(TryRecvError::Disconnected\)\)[\s\S]*?Some\(Err\(_\)\)'; Description = 'Hardware Center must handle all remaining telemetry receive states exhaustively' }
)
foreach ($Check in $RequiredPatterns) {
    if ($Source -notmatch $Check.Pattern) {
        throw "Source invariant failed: $($Check.Description). Update src/app.rs from the current NOVA source package."
    }
}

if ($Source -match '\.open\(&mut self\.[A-Za-z_][A-Za-z0-9_]*\)') {
    throw 'A NOVA window still borrows a self.* open flag directly. Use a local bool around Window::show to avoid overlapping closure borrows.'
}


# User-supplied paths must be validated against the same lexical root form
# before internal code switches to canonical paths. On Windows canonicalize()
# may add the \\?\ prefix; mixing that prefix into strip_prefix checks rejects
# valid temp/workspace paths as being outside NOVA.
$FileOpsPath = Join-Path $ProjectRoot 'src\file_ops.rs'
$TrashPath = Join-Path $ProjectRoot 'src\trash.rs'
if (-not (Test-Path -LiteralPath $FileOpsPath -PathType Leaf) -or -not (Test-Path -LiteralPath $TrashPath -PathType Leaf)) {
    throw 'File operation or Trash source was not found.'
}
$FileOps = Get-Content -LiteralPath $FileOpsPath -Raw
$Trash = Get-Content -LiteralPath $TrashPath -Raw
$PathPatterns = @(
    @{ Pattern = 'validate_existing_item\(root,\s*source\)'; Description = 'File operations validate the supplied source against its original root form' },
    @{ Pattern = 'validate_destination_dir\(root,\s*destination_dir\)'; Description = 'File operations validate the supplied destination against its original root form' },
    @{ Pattern = 'canonical_user_path\(root,\s*start\)'; Description = 'Search validates its supplied start path against the original root form' },
    @{ Pattern = 'canonical_user_path\(&paths\.user_dir,\s*source\)'; Description = 'Trash validates its supplied source against AppPaths user_dir before canonical comparisons' }
)
foreach ($Check in $PathPatterns) {
    $Content = if ($Check.Description -like 'Trash*') { $Trash } else { $FileOps }
    if ($Content -notmatch $Check.Pattern) {
        throw "Source path invariant failed: $($Check.Description)."
    }
}

Write-Host 'NOVA Rust ownership, borrow and Windows path invariants passed.' -ForegroundColor Green
