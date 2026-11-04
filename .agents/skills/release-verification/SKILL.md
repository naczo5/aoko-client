---
name: release-verification
description: >-
  Execution runbook for validating builds and publishing an Aoko GitHub release.
  Use when preparing a release, validating pull requests, or verifying pipeline builds.
---

# Release & Build Verification

Run from the repository root. Close Minecraft and `Aoko.exe` first so DLLs are
not locked.

---

## 1. Default gate (PRs and everyday verification)

```
[1. Native tests] ──> [2. C# tests] ──> [3. Native compile]
```

```powershell
.\McInjector\run_tests.bat
dotnet test .\Aoko.Tests\Aoko.Tests.csproj
.\build_dll.bat
```

`build_dll.bat` builds `bridge.dll` and `bridge_261.dll` and copies them into
`Aoko\bin\Debug\`, `Release\`, and related output folders. Confirm the copies
are newer than the source change.

Do **not** run `build_release.bat` or the GitHub release script for ordinary PRs.

---

## 2. Publish a GitHub release

Use `scripts\New-GitHubRelease.ps1`. It is the only supported publish path: it
checks `main` is clean and matches `origin/main`, runs `build_release.bat`
(which already builds both bridges then publishes), zips `Aoko_Release` as
`Aoko.zip`, and creates the GitHub release with `gh`.

```powershell
.\McInjector\run_tests.bat
dotnet test .\Aoko.Tests\Aoko.Tests.csproj
.\scripts\New-GitHubRelease.ps1 -Version 0.x.y
```

Optional: `-Draft` for a draft release. Requires `gh` authenticated, and a
stable **must run on `main`**. Do not create stable releases from `dev`. Do not
merge or push `main` unless the user explicitly asked.

### Beta releases from `dev`

Use `-Beta` to publish the current `dev` branch as an opt-in pre-release:

```powershell
.\scripts\New-GitHubRelease.ps1 -Version 0.x.y -Beta
```

The script requires `dev` to be clean and match `origin/dev`, tags the release
`v0.x.y-beta`, sets the GitHub prerelease flag, and targets `dev`. The Scoop
workflow skips prereleases, so the stable channel is unaffected. Bump `-Version`
for each new beta; an existing tag fails the release.

Do not call `build_release.bat` yourself when using this script; the script
already invokes it.

The published `Aoko.zip` is enough. A GitHub Action updates the install
manifest on `release: published`. Do not run bucket/manifest scripts as part of
the release.

---

## 3. Checklist

- [ ] Native tests exit 0
- [ ] `dotnet test` passes (includes `ModuleRegistrationTests`)
- [ ] Everyday PRs: `build_dll.bat` only
- [ ] Shipping: `New-GitHubRelease.ps1` on clean `main` matching origin
- [ ] Beta shipping: `New-GitHubRelease.ps1 -Beta` on clean `dev` matching origin (tag `v<version>-beta`)
- [ ] No locked `Aoko.exe` / Minecraft holding the DLLs
