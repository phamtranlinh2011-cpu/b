$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$AppRoot = $env:NOVA_APP_ROOT
if ([string]::IsNullOrWhiteSpace($AppRoot) -or -not (Test-Path -LiteralPath $AppRoot -PathType Container)) {
    throw 'NOVA_APP_ROOT is missing or is not a valid application folder.'
}
$AppRoot = (Resolve-Path -LiteralPath $AppRoot).Path
$DownloadUri = 'https://www.unikey.org/assets/release/unikey46RC2-230919-win64.zip'
$Stage = Join-Path $env:TEMP ('NOVA-UniKey-' + [Guid]::NewGuid().ToString('N'))
$Zip = Join-Path $Stage 'unikey.zip'
$Extract = Join-Path $Stage 'extract'
$Destination = $null
$TempTarget = $null
$BackupTarget = $null

function Ensure-NovaDirectDirectory([string]$Parent, [string]$Name) {
    $ParentFull = [System.IO.Path]::GetFullPath($Parent)
    $Candidate = Join-Path $ParentFull $Name
    if (Test-Path -LiteralPath $Candidate) {
        $Item = Get-Item -LiteralPath $Candidate -Force
        if (-not $Item.PSIsContainer -or (($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
            throw "NOVA refuses to write through a link, reparse point or non-directory: $Candidate"
        }
    }
    else {
        New-Item -ItemType Directory -Path $Candidate -ErrorAction Stop | Out-Null
        $Item = Get-Item -LiteralPath $Candidate -Force
        if (-not $Item.PSIsContainer -or (($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
            throw "New NOVA directory is not a real directory: $Candidate"
        }
    }
    $Resolved = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $Candidate).Path)
    if (-not $Resolved.Equals($Candidate, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "NOVA directory resolved outside its expected parent: $Candidate"
    }
    return $Resolved
}

try {
    New-Item -ItemType Directory -Force -Path $Stage, $Extract | Out-Null
    Write-Host 'Downloading UniKey 4.6 RC2 x64 from the official UniKey website...'
    Invoke-WebRequest -Uri $DownloadUri -OutFile $Zip -UseBasicParsing -TimeoutSec 60
    $Archive = Get-Item -LiteralPath $Zip
    if ($Archive.Length -le 0 -or $Archive.Length -gt 20MB) {
        throw "UniKey archive size is outside the accepted range: $($Archive.Length) bytes."
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $ZipArchive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        if ($ZipArchive.Entries.Count -gt 200) {
            throw "UniKey archive contains too many entries: $($ZipArchive.Entries.Count)."
        }
        $ExpandedBytes = [int64]0
        foreach ($Entry in $ZipArchive.Entries) {
            $ExpandedBytes += $Entry.Length
            $EntryName = $Entry.FullName.Replace('\', '/')
            $Segments = $EntryName.Split('/')
            if ([string]::IsNullOrWhiteSpace($EntryName) -or $EntryName.StartsWith('/') -or $EntryName.Contains(':') -or $EntryName.Contains([char]0) -or ($Segments -contains '..') -or ($Segments -contains '.')) {
                throw "UniKey archive contains an unsafe entry path: $($Entry.FullName)"
            }
            $Attributes = [uint32]([int64]$Entry.ExternalAttributes -band 0xFFFFFFFFL)
            $UnixType = ($Attributes -shr 16) -band 0xF000
            $DosAttributes = $Attributes -band 0x0000FFFF
            if ($UnixType -eq 0xA000 -or ($DosAttributes -band 0x0400) -ne 0) {
                throw "UniKey archive contains a symbolic link or reparse-point entry: $($Entry.FullName)"
            }
        }
        if ($ExpandedBytes -gt 64MB) {
            throw "UniKey archive expands beyond the 64 MiB safety limit: $ExpandedBytes bytes."
        }
    }
    finally {
        $ZipArchive.Dispose()
    }
    Expand-Archive -LiteralPath $Zip -DestinationPath $Extract -Force

    $Executable = Get-ChildItem -LiteralPath $Extract -Filter 'UniKeyNT.exe' -File -Recurse | Select-Object -First 1
    if (-not $Executable) {
        throw 'The official archive did not contain UniKeyNT.exe.'
    }

    $Signature = Get-AuthenticodeSignature -LiteralPath $Executable.FullName
    if ($Signature.Status -ne 'Valid' -or $Signature.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -cne 'PHAM KIM LONG') {
        throw "UniKey signature verification failed. Status=$($Signature.Status); Subject=$($Signature.SignerCertificate.Subject)"
    }

    if ($Executable.Length -le 0 -or $Executable.Length -gt 20MB) {
        throw "UniKey executable size is outside the accepted range: $($Executable.Length) bytes."
    }

    $AppsDirectory = Ensure-NovaDirectDirectory -Parent $AppRoot -Name 'Apps'
    $Destination = Ensure-NovaDirectDirectory -Parent $AppsDirectory -Name 'UniKey'
    $TargetExecutable = Join-Path $Destination 'UniKeyNT.exe'
    if (Test-Path -LiteralPath $TargetExecutable) {
        $ExistingTarget = Get-Item -LiteralPath $TargetExecutable -Force
        if ($ExistingTarget.PSIsContainer -or (($ExistingTarget.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
            throw 'NOVA refuses to replace a UniKey executable that is a directory, link or reparse point.'
        }
    }

    $TempTarget = Join-Path $Destination ('.UniKeyNT-' + [Guid]::NewGuid().ToString('N') + '.tmp.exe')
    Copy-Item -LiteralPath $Executable.FullName -Destination $TempTarget
    $TempSignature = Get-AuthenticodeSignature -LiteralPath $TempTarget
    if ($TempSignature.Status -ne 'Valid' -or $TempSignature.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -cne 'PHAM KIM LONG') {
        throw 'Signature verification failed on the staged UniKey copy.'
    }
    if (Test-Path -LiteralPath $TargetExecutable) {
        # Prefer an atomic replace where the USB filesystem supports it. Some
        # removable filesystems do not; retain a rollback copy for that case.
        try {
            [System.IO.File]::Replace($TempTarget, $TargetExecutable, $null)
            $TempTarget = $null
        }
        catch {
            $BackupTarget = Join-Path $Destination ('.UniKeyNT-backup-' + [Guid]::NewGuid().ToString('N') + '.exe')
            [System.IO.File]::Move($TargetExecutable, $BackupTarget)
            try {
                [System.IO.File]::Move($TempTarget, $TargetExecutable)
                $TempTarget = $null
            }
            catch {
                if ((Test-Path -LiteralPath $BackupTarget) -and -not (Test-Path -LiteralPath $TargetExecutable)) {
                    [System.IO.File]::Move($BackupTarget, $TargetExecutable)
                }
                throw
            }
        }
    }
    else {
        [System.IO.File]::Move($TempTarget, $TargetExecutable)
        $TempTarget = $null
    }

    $CopiedSignature = Get-AuthenticodeSignature -LiteralPath $TargetExecutable
    if ($CopiedSignature.Status -ne 'Valid' -or $CopiedSignature.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -cne 'PHAM KIM LONG') {
        Remove-Item -LiteralPath $TargetExecutable -Force -ErrorAction SilentlyContinue
        if ($BackupTarget -and (Test-Path -LiteralPath $BackupTarget)) {
            [System.IO.File]::Move($BackupTarget, $TargetExecutable)
        }
        throw 'Signature verification failed after copying UniKey to the USB drive.'
    }
    if ($BackupTarget -and (Test-Path -LiteralPath $BackupTarget)) {
        Remove-Item -LiteralPath $BackupTarget -Force -ErrorAction SilentlyContinue
    }

    Write-Host 'UniKey was copied to NOVA USB and its Authenticode signature is valid.' -ForegroundColor Green
    Write-Host "Path: $(Join-Path $Destination 'UniKeyNT.exe')"
    Write-Host 'Open NOVA and select UniKey; NOVA starts the bundled input method automatically.'
}
finally {
    if ($TempTarget -and (Test-Path -LiteralPath $TempTarget)) {
        Remove-Item -LiteralPath $TempTarget -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $Stage) {
        Remove-Item -LiteralPath $Stage -Recurse -Force -ErrorAction SilentlyContinue
    }
}
