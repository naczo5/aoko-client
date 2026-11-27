[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^\d+(\.\d+){1,3}$')]
    [string]$Version,

    # Build from dev and tag v<Version>-beta, flagged as a pre-release once published.
    # Stable releases still require main and a numeric tag.
    [switch]$Beta,

    # Optional markdown file with the release body. When omitted, GitHub
    # generates notes. Keep the body a short user-facing bullet list.
    [string]$NotesFile
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

$releaseBranch = if ($Beta) { 'dev' } else { 'main' }
$tag = if ($Beta) { "v$Version-beta" } else { "v$Version" }
$title = if ($Beta) { "aoko v$Version-beta" } else { "aoko v$Version" }

$currentBranch = ((& git branch --show-current) -join '').Trim()
$detachedHead = [string]::IsNullOrWhiteSpace($currentBranch)
if (-not $detachedHead -and $currentBranch -ne $releaseBranch) {
    throw "GitHub releases must be created from the $releaseBranch branch."
}
if ($env:GITHUB_REF -like 'refs/tags/*' -and $env:GITHUB_REF -ne "refs/tags/$tag") {
    throw "Workflow tag '$($env:GITHUB_REF)' does not match release tag '$tag'."
}

if (& git status --porcelain) {
    throw 'The working tree is not clean. Commit, stash, or discard changes before creating a release.'
}

Invoke-RequiredCommand -FilePath 'gh' -Arguments @('auth', 'status', '--hostname', 'github.com') -FailureMessage 'GitHub CLI is not authenticated. Run "gh auth login" first.'
Invoke-RequiredCommand -FilePath 'git' -Arguments @('fetch', 'origin', $releaseBranch, '--tags') -FailureMessage "Could not fetch origin/$releaseBranch and tags."

if ($detachedHead) {
    # CI tag checkout: the tag may target a dev commit that is not the branch tip,
    # but it must still be reachable from the release branch.
    & git merge-base --is-ancestor HEAD "origin/$releaseBranch"
    if ($LASTEXITCODE -ne 0) {
        throw "Release commit is not on origin/$releaseBranch."
    }
} else {
    $aheadBehind = ((& git rev-list --left-right --count "HEAD...origin/$releaseBranch").Trim() -split '\s+')
    if ($aheadBehind.Count -ne 2 -or $aheadBehind[0] -ne '0' -or $aheadBehind[1] -ne '0') {
        throw "Local $releaseBranch does not exactly match origin/$releaseBranch. Pull or push $releaseBranch before creating a release."
    }
}

$repository = ((& gh repo view --json nameWithOwner --jq '.nameWithOwner') -join '').Trim()
if ([string]::IsNullOrWhiteSpace($repository) -or $LASTEXITCODE -ne 0) {
    throw 'Could not resolve the GitHub repository from origin.'
}
$existingReleaseTags = @(& gh release list --repo $repository --limit 100 --json tagName --jq '.[].tagName')
if ($LASTEXITCODE -ne 0) {
    throw 'Could not list existing GitHub releases.'
}

if ($existingReleaseTags -contains $tag) {
    throw "GitHub release '$tag' already exists."
}

$notesFilePath = $null
if (-not [string]::IsNullOrWhiteSpace($NotesFile)) {
    if (-not (Test-Path -LiteralPath $NotesFile -PathType Leaf)) {
        throw "Release notes file was not found: $NotesFile"
    }
    $notesFilePath = (Resolve-Path -LiteralPath $NotesFile).Path
}

$tagExists = $false
& git rev-parse --verify --quiet "refs/tags/$tag" | Out-Null
if ($LASTEXITCODE -eq 0) { $tagExists = $true }
if ($tagExists) {
    $tagCommit = ((& git rev-list -n 1 $tag) -join '').Trim()
    $headCommit = ((& git rev-parse HEAD) -join '').Trim()
    if ($tagCommit -ne $headCommit) {
        throw "Tag $tag points to $tagCommit, not HEAD $headCommit. Check out the tagged commit or use a new version."
    }
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

# Releases are always created as drafts. This script cannot publish: that is a manual step in
# the GitHub UI, so an unintended run can never produce a public release or pre-release.
$releaseArguments = @(
    'release', 'create', $tag, "$zipPath#Aoko.zip",
    '--repo', $repository,
    '--title', $title,
    '--draft'
)
if ($tagExists) {
    $releaseArguments += '--verify-tag'
} else {
    $releaseArguments += @('--target', $releaseBranch)
}
if ($notesFilePath) {
    $releaseArguments += @('--notes-file', $notesFilePath)
} else {
    $releaseArguments += '--generate-notes'
}
if ($Beta) {
    $releaseArguments += '--prerelease'
    $releaseArguments += '--latest=false'
}

if (-not $PSCmdlet.ShouldProcess("$repository $tag", 'Create GitHub release')) {
    return
}

Invoke-RequiredCommand -FilePath 'gh' -Arguments $releaseArguments -FailureMessage "GitHub release creation failed for $tag."

# Defence in depth: confirm GitHub really holds a draft.
# (gh release view cannot look up a draft by tag, so read it from the list.)
$releaseList = ((& gh release list --repo $repository --limit 100 --json tagName,isDraft) -join "") | ConvertFrom-Json
$created = @($releaseList | Where-Object { $_.tagName -eq $tag }) | Select-Object -First 1
if ($LASTEXITCODE -ne 0 -or -not $created -or $created.isDraft -ne $true) {
    throw "Release $tag is not a draft. Check it in the GitHub UI immediately."
}
Write-Host "Created draft GitHub release $tag with Aoko.zip. Publish it manually from the GitHub releases page."
