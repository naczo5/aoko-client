---
name: write-docs
description: >-
  Keep README, the marketing website, the published docs site, AGENTS.md, and
  agent skills aligned with current client modules and behavior. Use when adding
  or changing a module or setting, editing website/ or README, writing Starlight
  pages, or updating AGENTS.md and .agents/skills/.
---

# Writing Aoko Documentation

Docs describe the **current** product. Source of truth is the running client:
`ModuleCatalog`, `Clicker` properties, WPF labels, and bridge capabilities — not
older markdown.

Then follow **add-client-module** if the code is also changing.

---

## 1. Surfaces (update the ones the change belongs to)

| Surface | Path / URL | Audience | What it must match |
| :--- | :--- | :--- | :--- |
| Agent operating guide | `AGENTS.md` | Agents | Commands, safety, catalog-first sync. Index skills; do not duplicate module manuals. |
| Skills | `.agents/skills/*/SKILL.md` | Agents | Procedures that still match the code. Link related skills; keep checklists at command/outcome level. |
| README | `README.md` | Users / GitHub | Feature list, versions, quick start, install. No JNI internals unless a user must set an env var. |
| Marketing site | `website/public/`  | Users | Landing page that also contains module list with details, as well as a mock preview of the client's external gui |
| Docs site (source) | `website/src/content/docs/` | Users | Per-module pages; categories follow the in-GUI layout. |
| Docs site (published) | https://naczo5.github.io/aoko-client/ | Users | Built from `website/` by `.github/workflows/docs.yml` on push of `website/**` to `main` or `dev`. |

Local preview: `cd website` → `npm run dev` (base path `/aoko-client/`).

User-facing pages do not explain agent skills. Agent files do not replace module
guides. If a setting exists in the GUI, it belongs on the module page (and in
README only if it is a headline feature).

---

## 2. Match the client

Before writing, read the implementation, not the previous sentence in the docs.

* **Modules:** `Aoko/Core/ModuleCatalog.cs` is the registry. Public docs cover
  user-visible modules. Skip `DevOnly` entries unless they are shown in the GUI.
* **Settings:** names, ranges, and defaults from `Clicker` / Profile setters and
  the XAML card — not from memory. Version gates from capabilities, not from an
  old “1.8.9 only” line.
* **Behavior:** document what the module does now. If getting-started and the GUI disagree,
  fix the page.
* **Cross-links:** README feature bullets, getting-started, and the module page
  should not contradict each other.

A new user-facing module is unfinished until the Starlight page, sidebar, and
README feature line exist (see add-client-module).

---

## 3. Writing rules

### Removals and replacements stay silent

When a feature, setting, bind, version, or command is removed or replaced:

* Delete the old mentions. Update remaining text so it is true **now**.
* Do **not** document the removal. No “removed X”, “formerly Y”, “no longer
  supports Z”, changelogs inside module pages, or “we used to…”.
* Do not leave stubs, strikethrough, or empty TODO sections for gone features.

The history belongs in git, not in the docs.

### No trivia that does not help the reader

Write the step or the behavior. Do not name internal tests, helper types, or
one-off implementation details unless the reader must use that name.

Bad: `` `dotnet test` passes (includes `ModuleRegistrationTests`) ``  
Good: `dotnet test` passes.

Bad: listing every JSON key in a user guide  
Good: the setting label the GUI shows, plus what it does.

Agent skills may cite files agents must edit (`ModuleCatalog.cs`, `ParseConfig`).
They still should not sprinkle unrelated test class names into checklists.

### Tone and shape

* User docs: present tense, the in-GUI name, settings tables like existing
  module pages (`title`, `description`, version support, settings, usage).
* Agent docs: imperative, short. Prefer “run `dotnet test`” over narrating the
  suite.
* Do not invent product names or modules that are not in the catalog.

---

## 4. Checklist

- [ ] Behavior matches current GUI and code
- [ ] Gone features were deleted from docs, not described
- [ ] Right surfaces updated (README / website / AGENTS / skills)
- [ ] Sidebar slugs match files under `website/src/content/docs/`
- [ ] No leftover “Keybinds tab” / old defaults / wrong version lists
