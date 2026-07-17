[CmdletBinding()]
param(
    [string]$ManifestPath = (Join-Path $PSScriptRoot "..\bucket\aoko.json"),
    [string]$ExpectedVersion,
    [string]$ExpectedUrl,
    [string]$ExpectedHash,
    [string]$ArchivePath
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
    throw "Scoop manifest was not found: $ManifestPath"
}

try {
    $manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
}
catch {
    throw "Scoop manifest is not valid JSON: $($_.Exception.Message)"
}

foreach ($property in 'version', 'description', 'homepage', 'license') {
    if ([string]::IsNullOrWhiteSpace([string]$manifest.$property)) {
        throw "Scoop manifest is missing '$property'."
    }
}

$package = $manifest.architecture.'64bit'
if ($null -eq $package) {
    throw "Scoop manifest is missing the 64bit package definition."
}

if ([string]::IsNullOrWhiteSpace([string]$package.url) -or $package.url -notmatch '^https://') {
    throw "Scoop manifest has an invalid 64bit download URL."
}

if ([string]$package.hash -notmatch '^[a-fA-F0-9]{64}$') {
    throw "Scoop manifest must contain a SHA-256 hash for the 64bit package."
}

if ($ExpectedVersion -and $manifest.version -ne $ExpectedVersion) {
    throw "Expected manifest version '$ExpectedVersion', found '$($manifest.version)'."
}

if ($ExpectedUrl -and $package.url -ne $ExpectedUrl) {
    throw "Expected manifest URL '$ExpectedUrl', found '$($package.url)'."
}

if ($ExpectedHash -and $package.hash -ne $ExpectedHash) {
    throw "Expected manifest SHA-256 '$ExpectedHash', found '$($package.hash)'."
}

$shortcut = @($manifest.shortcuts | Where-Object {
    $_.Count -ge 2 -and $_[0] -eq 'Aoko.exe' -and $_[1] -eq 'Aoko'
})
if ($shortcut.Count -ne 1) {
    throw "Scoop manifest must create the Aoko Start Menu shortcut."
}

if ($manifest.checkver.github -ne 'https://github.com/naczo5/aoko-client') {
    throw "Scoop manifest must check GitHub releases for updates."
}

if ($manifest.autoupdate.architecture.'64bit'.url -ne 'https://github.com/naczo5/aoko-client/releases/download/v$version/Aoko.zip') {
    throw "Scoop manifest has an unexpected 64bit auto-update URL."
}

if ($ArchivePath) {
    if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) {
        throw "Release archive was not found: $ArchivePath"
    }

    $archiveHash = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($package.hash -ne $archiveHash) {
        throw "Release archive SHA-256 '$archiveHash' does not match the manifest."
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $ArchivePath))
    try {
        $entries = @($archive.Entries | ForEach-Object FullName)
        foreach ($requiredEntry in 'Aoko.exe', 'bridge.dll', 'bridge_261.dll', 'Data/gtb_wordlist.js', 'Data/minecraftia.ttf') {
            if ($entries -notcontains $requiredEntry) {
                throw "Release archive is missing required file '$requiredEntry'."
            }
        }
    }
    finally {
        $archive.Dispose()
    }
}

Write-Host "Scoop manifest validation passed for aoko $($manifest.version)."
