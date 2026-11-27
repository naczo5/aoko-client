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

`scripts\New-GitHubRelease.ps1` is the shared build/publish path: it
checks the release branch is clean and matches origin, runs
`build_release.bat` (which already builds both bridges then publishes), zips
`Aoko_Release` as `Aoko.zip`, and creates a **draft** GitHub release with `gh`.

**Nothing automated publishes a release.** The script and the workflow always create
drafts and cannot do otherwise (there is no `-Draft` switch to forget, and no tag
trigger). The release owner publishes by hand from the GitHub releases page; that
manual publish is also what refreshes the docs and, for stable, the Scoop manifest.

```powershell
.\McInjector\run_tests.bat
dotnet test .\Aoko.Tests\Aoko.Tests.csproj
.\scripts\New-GitHubRelease.ps1 -Version 0.x.y
```

Requires `gh` authenticated, and a stable **must run on `main`**. Do not create stable releases from `dev`. Do not
merge or push `main` unless the user explicitly asked.

### Beta releases from `dev`

Use `-Beta` to draft a pre-release from the current `dev` branch:

```powershell
.\scripts\New-GitHubRelease.ps1 -Version 0.x.y -Beta
```

The script requires `dev` to be clean and match `origin/dev`, tags the release
`v0.x.y-beta`, sets the GitHub prerelease flag on the draft, and targets `dev`. The Scoop
workflow skips prereleases, so the stable channel is unaffected. Bump `-Version`
for each new beta; an existing tag fails the release.

### CI release workflow

`.github/workflows/release.yml` builds on `windows-latest`, runs the native and
C# tests, then calls `New-GitHubRelease.ps1`. Prefer it for releases so the
artifacts are built and tested on Windows.

- **Actions → Build and publish release** (`workflow_dispatch` only): choose `beta`
  (builds `dev`) or `stable` (builds `main`), version, and optional notes. The result is
  always a draft. Publish it manually in the GitHub UI after downloading and checking
  `Aoko.zip`.
- Pushing a tag does nothing. The workflow has no tag trigger on purpose, because a tag push
  once published a release unintentionally.
- `workflow_dispatch` only appears after the workflow exists on the default
  branch, but `gh workflow run release.yml --ref dev` runs the `dev` copy.
- Docs and Scoop refresh when you publish the draft (their `release: published`
  triggers). The release workflow does not dispatch them. Scoop skips prereleases.

### Release notes style

Write the diff from the previous release, not a changelog of every commit.

- 4–6 bullets, one line each. No headings, tables, paragraphs, or emoji.
- New modules go in one explicit `New modules:` bullet. Do not bury them under a tab name or describe them as "native support".
- Do not list fixes for features that first ship in this same release. If the previous release never had the feature, its bug fixes are not user-facing.
- Mention internal rendering/UI work when users will notice it: HUD refresh, shared renderer, font, layout, driver risk, safe mode.
- Merge related items into one bullet instead of listing every commit.
- Describe what changed for the user, not commit titles or internal symbols.
- Beta builds: note that the new modules are not thoroughly tested.
- No marketing language. Plain, factual wording.
- End with a single `**Full Changelog**: https://github.com/naczo5/aoko-client/compare/<previous>...<tag>` line.

Example:

```markdown
- New modules: Fast Place, No Hurt Cam, Sprint, TNT Timer, WTap, Jump Reset, Hit Select, Trajectories, Murder Mystery, Break Progress
- New Beta tab for untested modules; use at your own risk. Kill Aura moved there (still requires Dev Mode)
- HUD rendering refresh: shared ImGui layer, live latency/brand/FPS watermark, glass module rows, restyled player/GTB/Pixel Party/Block ESP/HUD editor panels, bundled Manrope font
- HUD layout: module rows sort by full width including tags; right-anchored lists stay put when resizing
- Overlay stability: lower driver risk and a no-overlay safe mode

**Full Changelog**: https://github.com/naczo5/aoko-client/compare/v11.6...v11.7-beta
```

Pass notes to the script with `-NotesFile <path>`; otherwise GitHub generates
them.

Do not call `build_release.bat` yourself when using this script; the script
already invokes it.

The published `Aoko.zip` is enough. Publishing the draft in the GitHub UI emits the
`release` event that refreshes the docs and Scoop workflows. Do not run
bucket/manifest scripts as part of the release.

---

## 3. Checklist

- [ ] Native tests exit 0
- [ ] `dotnet test` passes (includes `ModuleRegistrationTests`)
- [ ] Everyday PRs: `build_dll.bat` only
- [ ] Shipping: `release.yml` CI workflow, or `New-GitHubRelease.ps1` on clean `main` matching origin
- [ ] Beta shipping: `release.yml` beta/tag, or `New-GitHubRelease.ps1 -Beta` on clean `dev` matching origin (tag `v<version>-beta`)
- [ ] Release body: short bullet list in the existing style, with Full Changelog last
- [ ] No locked `Aoko.exe` / Minecraft holding the DLLs
