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


# Windows path containment uses stable file identity for equivalent path spellings
# and continues to reject reparse points before following or operating on paths.
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
    @{ Content = $Storage; Pattern = 'windows_directory_identity'; Description = 'Windows path containment is verified using volume/file identity rather than lexical path spelling' },
    @{ Content = $Storage; Pattern = 'windows_spelling_relative_path'; Description = 'Windows DOS and verbatim path prefixes are normalized and compared ordinally before the file-ID fallback' },
    @{ Content = $Storage; Pattern = 'windows_spelling_relative_path_accepts_dos_and_verbatim_prefixes_but_rejects_siblings'; Description = 'A regression test covers DOS/verbatim prefix equivalence and sibling-path rejection' },
    @{ Content = $Storage; Pattern = 'relative_path_from_canonical_root'; Description = 'Canonical paths are converted to a verified root-relative path' },
    @{ Content = $Storage; Pattern = 'GetFileInformationByHandle'; Description = 'Windows containment fallback verifies real volume/file identities' },
    @{ Content = $Storage; Pattern = 'FILE_ATTRIBUTE_REPARSE_POINT'; Description = 'The identity check refuses reparse-point directories' },
    @{ Content = $Storage; Pattern = 'canonical_user_path_with_relative'; Description = 'Callers can reuse the verified relative path without lexical strip_prefix comparisons' },
    @{ Content = $Storage; Pattern = 'canonical_user_path_accepts_mixed_canonical_and_lexical_spellings'; Description = 'A regression test covers mixed DOS/verbatim path spellings' },
    @{ Content = $Storage; Pattern = 'canonical_user_path_accepts_case_varied_windows_spellings'; Description = 'A Windows regression test covers case-varied path spellings' },
    @{ Content = $Storage; Pattern = 'missing_components'; Description = 'Directory creation resolves an existing ancestor before appending missing components' },
    @{ Content = $FileOps; Pattern = 'validate_existing_item\(root,\s*source\)'; Description = 'File operations validate the selected source against the workspace root' },
    @{ Content = $FileOps; Pattern = 'canonical_directory_contains\(&canonical_source,\s*&destination\)'; Description = 'Copy, import and move reject destination directories inside the source tree using identity-based containment' },
    @{ Content = $FileOps; Pattern = 'validate_destination_dir\(root,\s*destination_dir\)'; Description = 'File operations validate the destination against the workspace root' },
    @{ Content = $FileOps; Pattern = 'canonical_user_path\(root,\s*start\)'; Description = 'Search validates its start path against the workspace root' },
    @{ Content = $FileOps; Pattern = 'canonical_user_path_with_relative\(root,\s*parent\)'; Description = 'Rename derives the target parent from the verified root-relative path' },
    @{ Content = $Trash; Pattern = 'canonical_user_path_with_relative\(&paths\.user_dir,\s*source\)'; Description = 'Trash validates the source and obtains a verified relative path by directory identity' }
)
foreach ($Check in $PathPatterns) {
    if ($Check.Content -notmatch $Check.Pattern) {
        throw "Source path invariant failed: $($Check.Description)."
    }
}
if ($Storage -match 'let\s+relative\s*=\s*candidate\.strip_prefix\(root\)' -or
    $FileOps -match 'destination\.starts_with\(&canonical_source\)' -or
    $Trash -match 'canonical_source\.strip_prefix\(&canonical_root\)') {
    throw 'A path containment check still relies on a lexical prefix comparison.'
}

$SearchFunction = [regex]::Match($FileOps, '(?s)pub fn search_user_files_with_control\(.*?\)\s*->\s*io::Result<SearchResult>\s*\{(.*?)\n\}')
if (-not $SearchFunction.Success -or
    $SearchFunction.Groups[1].Value -notmatch 'if cancel\.load\(Ordering::Relaxed\)\s*\{\s*return Err\(io::Error::new\(io::ErrorKind::Interrupted' -or
    $SearchFunction.Groups[1].Value.IndexOf('canonical_user_path') -lt 0 -or
    $SearchFunction.Groups[1].Value.IndexOf('canonical_user_path') -lt $SearchFunction.Groups[1].Value.IndexOf('if cancel.load')) {
    throw 'Cancelled file search must return Interrupted before canonical path validation.'
}

Write-Host 'NOVA Rust ownership, borrow, cancellation and Windows file-identity path invariants passed.' -ForegroundColor Green
