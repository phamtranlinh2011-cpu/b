$ErrorActionPreference = 'Stop'

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$RevisionPath = Join-Path $ProjectRoot 'SOURCE_REVISION.txt'
$ExpectedRevision = 'NOVA-SOURCE-AUDIT-2026-11-18-CI-WIFI-IDENTITY-R4'
if (-not (Test-Path -LiteralPath $RevisionPath -PathType Leaf)) {
    throw 'Source revision marker is missing. Replace the repository files with the complete current NOVA source package.'
}
$ActualRevision = (Get-Content -LiteralPath $RevisionPath -Raw).Trim()
if ($ActualRevision -ne $ExpectedRevision) {
    throw "Wrong NOVA source revision. Expected '$ExpectedRevision' but found '$ActualRevision'. Replace the repository files with the complete current NOVA source package."
}
Write-Host "NOVA source audit revision: $ActualRevision" -ForegroundColor Cyan

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
$UniKeyPath = Join-Path $ProjectRoot 'src\unikey.rs'
if (-not (Test-Path -LiteralPath $StoragePath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $FileOpsPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $TrashPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $UniKeyPath -PathType Leaf)) {
    throw 'Storage, file operation, Trash or UniKey source was not found.'
}
$Storage = Get-Content -LiteralPath $StoragePath -Raw
$FileOps = Get-Content -LiteralPath $FileOpsPath -Raw
$Trash = Get-Content -LiteralPath $TrashPath -Raw
$UniKey = Get-Content -LiteralPath $UniKeyPath -Raw
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
    @{ Content = $Storage; Pattern = 'resolved root:.*resolved candidate:'; Description = 'Path denial diagnostics include the canonical root and candidate to identify path mismatches in CI logs' },
    @{ Content = $Storage; Pattern = 'canonical_user_path_accepts_mixed_canonical_and_lexical_spellings'; Description = 'A regression test covers mixed DOS/verbatim path spellings' },
    @{ Content = $Storage; Pattern = 'audit_revision_20261117_temp_workspace_relative_path_round_trips'; Description = 'The checked-out source contains the current temporary-workspace path regression test' },
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

$CopyFunction = [regex]::Match($FileOps, '(?s)fn copy_item_cancellable_with_hook<.*?\n\}')
if (-not $CopyFunction.Success -or
    $CopyFunction.Groups[0].Value.IndexOf('if cancel.load(Ordering::Relaxed)') -lt 0 -or
    $CopyFunction.Groups[0].Value.IndexOf('if cancel.load(Ordering::Relaxed)') -gt $CopyFunction.Groups[0].Value.IndexOf('canonical_root = root.canonicalize()')) {
    throw 'Cancelled file copy must check cancellation before filesystem validation or staging work.'
}

$SearchFunction = [regex]::Match($FileOps, '(?s)pub fn search_user_files_with_control\(.*?\)\s*->\s*io::Result<SearchResult>\s*\{(.*?)\n\}')
if (-not $SearchFunction.Success -or
    $SearchFunction.Groups[1].Value -notmatch 'if cancel\.load\(Ordering::Relaxed\)\s*\{\s*return Err\(io::Error::new\(io::ErrorKind::Interrupted' -or
    $SearchFunction.Groups[1].Value.IndexOf('canonical_user_path') -lt 0 -or
    $SearchFunction.Groups[1].Value.IndexOf('canonical_user_path') -lt $SearchFunction.Groups[1].Value.IndexOf('if cancel.load')) {
    throw 'Cancelled file search must return Interrupted before canonical path validation.'
}

$WifiPath = Join-Path $ProjectRoot 'src\wifi.rs'
$TerminalPath = Join-Path $ProjectRoot 'src\terminal.rs'
$HardwarePath = Join-Path $ProjectRoot 'src\hardware.rs'
$WorkflowPath = Join-Path $ProjectRoot '.github\workflows\rust.yml'
$Wifi = Get-Content -LiteralPath $WifiPath -Raw
$Terminal = Get-Content -LiteralPath $TerminalPath -Raw
$Hardware = Get-Content -LiteralPath $HardwarePath -Raw
$Workflow = Get-Content -LiteralPath $WorkflowPath -Raw
if ($Wifi -notmatch 'matches_expected_ssid\(&connection\.ssid,\s*expected\)' -or
    $Wifi -match 'connection\.ssid\.eq_ignore_ascii_case\(expected\)') {
    throw 'Wi-Fi connection verification must compare exact SSID bytes.'
}
if ($Wifi -notmatch 'matches_profile_name\(&name,\s*profile_name\)' -or
    $Wifi -match 'name\.eq_ignore_ascii_case\(profile_name\)') {
    throw 'Windows WLAN profile names must be compared case-sensitively.'
}
if ($Wifi -notmatch 'checked_dot11_ssid_bytes' -or $Wifi -match 'ssid\.bytes\[\.\.length\.min\(32\)\]') {
    throw 'WLAN association SSID lengths must be validated instead of silently truncated.'
}
if ($Wifi -notmatch 'Err\(connection_timeout_error\(' -or $Wifi -notmatch 'unconfirmed_wifi_requests_return_a_timeout_error') {
    throw 'Unconfirmed WLAN association/disconnection must be reported as a timeout failure.'
}
if ($Terminal -notmatch 'wifi::wait_for_connection\(Some\(&ssid\),\s*Duration::from_secs\(15\)\)') {
    throw 'Terminal Wi-Fi connect must confirm the actual WLAN association before reporting success.'
}
if ($Hardware -notmatch 'pub vendor: Option<String>' -or
    $Source -notmatch 'gpu\.vendor\.as_deref\(') {
    throw 'Hardware GPU vendor metadata must be consumed by the UI, not left as an unused field.'
}
if ($Workflow -notmatch 'components:\s*clippy,\s*rustfmt' -or
    $Workflow -match 'Write-Host\s+"NOVA workflow ref: \$\{\{\s*github\.ref') {
    throw 'CI must install rustfmt and must not interpolate github.ref directly into inline PowerShell.'
}
if ($Workflow -notmatch 'NOVA_WORKFLOW_REF:\s*\$\{\{\s*github\.ref\s*\}\}') {
    throw 'CI must pass the workflow ref to PowerShell through an environment variable.'
}
if ($Wifi -notmatch 'PROFILE_PREFIX.*timestamp:x.*sequence:x' -or
    $Storage -notmatch 'is_generated_nova_wifi_profile_name' -or
    $Storage -notmatch 'nova_wifi_profile_name_rejects_unmanaged_manifest_names' -or
    $Storage -notmatch 'nova_wifi_profile_name_validator_accepts_only_generated_suffixes') {
    throw 'Wi-Fi manifest profile names must be validated before being trusted for profile deletion.'
}
if ($Wifi -notmatch 'fn file_mut\(&mut self\)\s*->\s*io::Result<&mut File>' -or
    $Wifi -match 'file_mut\(\)\.expect\(') {
    throw 'Temporary Wi-Fi profile file access must return a recoverable I/O error rather than panic.'
}
if ($UniKey -notmatch 'canonical_user_path\(&root,\s*&path\)' -or
    $UniKey -match 'canonical\.starts_with\(&root\)') {
    throw 'UniKey discovery must use the Windows-aware canonical user-path verifier.'
}

Write-Host 'NOVA ownership, borrow, cancellation, filesystem identity, Wi-Fi identity and CI invariants passed.' -ForegroundColor Green
