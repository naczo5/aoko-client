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
`Aoko_Release` as `Aoko.zip`, and creates the GitHub release with `gh`.

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

### CI release workflow

`.github/workflows/release.yml` builds on `windows-latest`, runs the native and
C# tests, then calls `New-GitHubRelease.ps1`. Prefer it for releases so the
artifacts are built and tested on Windows.

- Manual: **Actions → Build and publish release**, choose `beta` (builds `dev`)
  or `stable` (builds `main`), version, draft, and optional notes.
- Tag: create an **annotated** tag whose message is the release body, then push
  it. The workflow uses the tag message as the notes; a lightweight tag falls
  back to generated notes. Tag `v<version>-beta` on `dev` or `v<version>` on
  `main`. The tag must contain the workflow file; the script accepts detached
  CI tag checkouts when the commit is reachable from the release branch.

  ```powershell
  git tag -a v0.x.y-beta -F notes.md
  git push origin v0.x.y-beta
  ```
- `workflow_dispatch` only appears after the workflow exists on the default
  branch. Tag pushes work from `dev` immediately.
- Releases created with the default `GITHUB_TOKEN` do not emit a `release`
  event, so the workflow explicitly dispatches `docs.yml` and `scoop.yml` after
  publishing. Scoop skips prereleases.

### Release notes style

Match the existing releases: a short, user-facing bullet list, not a changelog
dump.

- 4–6 bullets, one line each. No headings, tables, paragraphs, or emoji.
- Describe what changed for the user, not commit titles or internal symbols.
- Merge related fixes into one bullet instead of listing every commit.
- Beta builds: one bullet can say the modules are not thoroughly tested.
- No marketing language. Plain, factual wording.
- End with a single `**Full Changelog**: https://github.com/naczo5/aoko-client/compare/<previous>...<tag>` line.

Example:

```markdown
- Beta tab with Jump Reset, Hit Select, Trajectories, Murder Mystery, Break Progress, and Kill Aura
- Fast Place, No Hurt Cam, Sprint, TNT Timer, and WTap native support
- Fixed a 1.21/26.x crash on server or dimension changes
- Jump Reset and No Hurt Cam reliability fixes

**Full Changelog**: https://github.com/naczo5/aoko-client/compare/v11.6...v11.7-beta
```

Pass notes to the script with `-NotesFile <path>`; otherwise GitHub generates
them.

Do not call `build_release.bat` yourself when using this script; the script
already invokes it.

The published `Aoko.zip` is enough. The release workflow dispatches the docs
and Scoop workflows after publishing (bot-created releases do not emit release
events). Do not run bucket/manifest scripts as part of the release.

---

## 3. Checklist

- [ ] Native tests exit 0
- [ ] `dotnet test` passes (includes `ModuleRegistrationTests`)
- [ ] Everyday PRs: `build_dll.bat` only
- [ ] Shipping: `release.yml` CI workflow, or `New-GitHubRelease.ps1` on clean `main` matching origin
- [ ] Beta shipping: `release.yml` beta/tag, or `New-GitHubRelease.ps1 -Beta` on clean `dev` matching origin (tag `v<version>-beta`)
- [ ] Release body: short bullet list in the existing style, with Full Changelog last
- [ ] No locked `Aoko.exe` / Minecraft holding the DLLs
