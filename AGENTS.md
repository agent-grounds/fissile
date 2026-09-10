<!-- BEGIN GRUND MANAGED BLOCK -->
## Grounding with grund (v8)

This project uses [`grund`](https://github.com/vjovanov/grund): every spec, goal, decision, and end-to-end test has a stable ID `<KIND>-<NNN>-<slug>[.<section>]` (`KIND ∈ {GND, GOAL, FS, AR, DF, DA, RM}`), cited with the marker `§` — e.g. `<§>FS-042-user-login.3.1` (the `FS-042-user-login` here is a shape illustration, not a real ID in this repo, hence the `<§>` escape). Type `$$` in a grund-aware editor and it becomes `§`. Bare ID-shaped tokens are also recognized as citations because `[reference] strict = false` is set in `grund.toml`; remove that compatibility override or set strict back to `true` to require the `§` marker (run `grund fmt --marker` first to upgrade existing bare citations).

### Grounding from a citation

A `§<ID>` is a pointer to a fact, not a file path. Resolve it with `grund` and climb only as far as needed:

- `grund <ID>` — the lead (heading-less, cut at the first child section). The cheap first read for a bare `§<ID>` citation.
- `grund <ID> --toc` — the lead plus the nested section map. Use to choose which subsection to fetch next.
- `grund <ID> --full` — the entire body. Escalate to this when narrower reads aren't enough.
- `grund <ID> --brief` — heading + first paragraph only.
- `grund refs <ID>` — every site that cites the ID; add `--summary` for one line per file. Run before renaming or moving a declaration.
- `grund list` / `grund list --kind FS,AR` — discover IDs if you get lost

### Project map

- [GND](docs/grund.md): Reason for existence
- [GOAL](docs/goals.md): Outcomes the project is trying to achieve
- [FS](docs/functional-spec): User-visible behavior and requirements
- [AR](docs/architecture): System design and technical structure
- [DF](docs/decisions/functional): Product behavior decisions and tradeoffs
- [DA](docs/decisions/architectural): Architecture decisions and tradeoffs
- [tests/e2e/](tests/e2e): User scenarios: black-box proof of the spec
- [tests/integration/](tests/integration): Integration tests: proof that the parts fit as designed
- [RM](docs/roadmap.md): Planned milestones and sequencing

### Project namespaces

A namespace is a project boundary, not a docs folder. The current project is the local namespace: cite its IDs as `§<ID>`.

Create or use a separate namespace when work introduces an independently checked app, package, service, or subproject. Give that project its own `grund.toml`, add it to the workspace root's `[workspace] members`, run `grund init` there, and set a stable `project_name`.

Do not create a namespace for a regular module or component that still belongs to this project. Cite across namespaces as `§alias/<ID>` and run `grund check` from the workspace root.

### Declarations and citations

Declarations are heading lines `# FS-042-user-login: …` in markdown. In a code doc-comment (Rustdoc, Javadoc, JSDoc, Python docstring, Go `//`, …) drop the `#` — write `/// FS-042-user-login: …` directly. Numbered headings inside a declaration are citable sections: use depth-matching headings (`## 1. …`, `### 1.1 …`, etc.) so `§<ID>.1` / `§<ID>.1.1` resolve; mismatched heading depth is a `grund check` error. Plain headings or bold labels are fine for non-citable local structure. One doc-comment may declare multiple IDs (e.g. an `AR-` and an `FS-` on the same class) — each gets its own body. An inline source declaration is reachable from the configured kind home via a one-line stub: `# <ID>: [<path>](<path>)`.

### Rules

- **Spec first.** For behavior or design changes, write or update the most-specific spec point before code.
- **Cite as you write.** Place `§<ID>` at the point a claim or behavior is made — on the doc-comment for a whole behavior, inline beside the clause it enforces.
- **Marker = live citation.** A `§`-prefixed token resolves and is checked wherever it appears — including inside Markdown backticks. To mention an ID without citing it, write `<§><ID>`, omit the marker, or use a fenced code block.
- **Inline citation style.** Inline notes: ≤ 1 line preferred, hard cap 3 lines; ≤ 100 columns. A note is one comment block: a blank line splits it, an empty comment line does not. Doc-comments (`///`, `//!`, `/** */`, a docstring, a comment right above a definition) are documentation, not notes: they are never measured, so cite in-sentence there.
- **Always cite the most-specific point.**

### Citation directions

`must`/`never` are `grund check` errors; `should`/`avoid` are suggestions (`grund check --suggestions`).

- Each file in **tests/e2e/** must cite FS; avoid citing AR.
- Each file in **tests/integration/** should cite AR.
Anything not listed above is allowed.

### Clickable citations

On repository web surfaces, link `§<ID>` to the PR branch in PR bodies, the reviewed commit in reviews, an exact commit for permalinks, and the default branch otherwise; fall back to plain when unsure.
<!-- END GRUND MANAGED BLOCK -->

<!-- BEGIN FISSILE MANAGED BLOCK -->
## Keeping Files Small With fissile (v3)

This repository caps file size with [`fissile`](https://github.com/agent-grounds/fissile)
so that agents spend fewer tokens reading. Run `fissile check --staged` before
claiming work is done; its findings say what to split and how. Where the
pre-commit hook is installed it runs that same check — never get past it with
`--no-verify`.
<!-- END FISSILE MANAGED BLOCK -->
