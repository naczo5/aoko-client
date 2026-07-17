[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^\d+(\.\d+){2,3}$')]
    [string]$Version,

    [switch]$Draft
)

$ErrorActionPreference = 'Stop'

$repositoryRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repositoryRoot

function Invoke-RequiredCommand {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$FailureMessage
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw $FailureMessage
    }
}

if ((& git branch --show-current).Trim() -ne 'main') {
    throw 'GitHub releases must be created from the main branch.'
}

if (& git status --porcelain) {
    throw 'The working tree is not clean. Commit, stash, or discard changes before creating a release.'
}

Invoke-RequiredCommand -FilePath 'gh' -Arguments @('auth', 'status', '--hostname', 'github.com') -FailureMessage 'GitHub CLI is not authenticated. Run "gh auth login" first.'
Invoke-RequiredCommand -FilePath 'git' -Arguments @('fetch', 'origin', 'main', '--tags') -FailureMessage 'Could not fetch origin/main and tags.'

$aheadBehind = ((& git rev-list --left-right --count 'HEAD...origin/main').Trim() -split '\s+')
if ($aheadBehind.Count -ne 2 -or $aheadBehind[0] -ne '0' -or $aheadBehind[1] -ne '0') {
    throw 'Local main does not exactly match origin/main. Pull or push main before creating a release.'
}

$repository = (& gh repo view --json nameWithOwner --jq '.nameWithOwner').Trim()
if ([string]::IsNullOrWhiteSpace($repository) -or $LASTEXITCODE -ne 0) {
    throw 'Could not resolve the GitHub repository from origin.'
}

$tag = "v$Version"
$existingReleaseTags = @(& gh release list --repo $repository --limit 100 --json tagName --jq '.[].tagName')
if ($LASTEXITCODE -ne 0) {
    throw 'Could not list existing GitHub releases.'
}

if ($existingReleaseTags -contains $tag) {
    throw "GitHub release '$tag' already exists."
}

$buildScript = Join-Path $repositoryRoot 'build_release.bat'
if (-not (Test-Path -LiteralPath $buildScript -PathType Leaf)) {
    throw "Release build script was not found: $buildScript"
}

if (-not $PSCmdlet.ShouldProcess("Aoko v$Version", 'Build release artifacts')) {
    return
}

& $buildScript $Version
if ($LASTEXITCODE -ne 0) {
    throw "Release build failed for v$Version."
}

$releaseDirectory = Join-Path $repositoryRoot 'Aoko_Release'
$zipPath = Join-Path $releaseDirectory 'Aoko.zip'
if (-not (Test-Path -LiteralPath (Join-Path $releaseDirectory 'Aoko.exe') -PathType Leaf)) {
    throw "Release output is missing Aoko.exe: $releaseDirectory"
}

Compress-Archive -Path (Join-Path $releaseDirectory '*') -DestinationPath $zipPath -Force
if (-not (Test-Path -LiteralPath $zipPath -PathType Leaf)) {
    throw "Could not create release archive: $zipPath"
}

$releaseArguments = @(
    'release', 'create', $tag, "$zipPath#Aoko.zip",
    '--repo', $repository,
    '--target', 'main',
    '--title', "aoko v$Version",
    '--generate-notes'
)
if ($Draft) {
    $releaseArguments += '--draft'
}

if (-not $PSCmdlet.ShouldProcess("$repository $tag", 'Create GitHub release')) {
    return
}

Invoke-RequiredCommand -FilePath 'gh' -Arguments $releaseArguments -FailureMessage "GitHub release creation failed for $tag."

$releaseKind = if ($Draft) { 'draft' } else { 'published' }
Write-Host "Created $releaseKind GitHub release $tag with Aoko.zip."
