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
    @{ Pattern = 'self\.files_open\s*=\s*files_open\s*;'; Description = 'File Manager must copy the local open flag back to application state' }
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


# Canonical path validation must accept the equivalent Windows DOS and
# extended-length (\\?\) spellings without weakening reparse-point checks.
$StoragePath = Join-Path $ProjectRoot 'src\storage.rs'
$FileOpsPath = Join-Path $ProjectRoot 'src\file_ops.rs'
$TrashPath = Join-Path $ProjectRoot 'src\trash.rs'
if (-not (Test-Path -LiteralPath $StoragePath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $FileOpsPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $TrashPath -PathType Leaf)) {
    throw 'Storage, file operation or Trash source was not found.'
}
$Storage = Get-Content -LiteralPath $StoragePath -Raw
$FileOps = Get-Content -LiteralPath $FileOpsPath -Raw
$Trash = Get-Content -LiteralPath $TrashPath -Raw
$PathPatterns = @(
    @{ Content = $Storage; Pattern = 'ensure_real_directory_chain\(candidate_parent\)'; Description = 'Canonical path validation checks the caller-visible parent chain before resolving aliases' },
    @{ Content = $Storage; Pattern = 'let canonical_candidate\s*=\s*candidate\.canonicalize\(\)\?;'; Description = 'Canonical path validation resolves the candidate independently of its lexical prefix' },
    @{ Content = $Storage; Pattern = 'canonical_candidate\.strip_prefix\(&canonical_root\)'; Description = 'Canonical path containment is checked using canonical forms of both paths' },
    @{ Content = $Storage; Pattern = 'canonical_user_path_accepts_mixed_canonical_and_lexical_spellings'; Description = 'A regression test covers mixed DOS/verbatim path spellings' },
    @{ Content = $FileOps; Pattern = 'validate_existing_item\(root,\s*source\)'; Description = 'File operations validate the selected source against the workspace root' },
    @{ Content = $FileOps; Pattern = 'validate_destination_dir\(root,\s*destination_dir\)'; Description = 'File operations validate the destination against the workspace root' },
    @{ Content = $FileOps; Pattern = 'canonical_user_path\(root,\s*start\)'; Description = 'Search validates its start path against the workspace root' },
    @{ Content = $Trash; Pattern = 'canonical_user_path\(&paths\.user_dir,\s*source\)'; Description = 'Trash validates the supplied source against AppPaths user_dir' }
)
foreach ($Check in $PathPatterns) {
    if ($Check.Content -notmatch $Check.Pattern) {
        throw "Source path invariant failed: $($Check.Description)."
    }
}

Write-Host 'NOVA Rust ownership, borrow and Windows path invariants passed.' -ForegroundColor Green
