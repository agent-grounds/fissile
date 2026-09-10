# FS-001-config: fissile reads a versioned TOML config file

`fissile` is configured by a single TOML document. Its home is
`.agent-grounds/fissile.toml`, and the former home `.agents/fissile.toml` is
still read and reported as deprecated (§8); an embedding CLI may also pass an
explicit path. The config is data, not code, so it can be read inside a
pre-commit hook without invoking a plugin system (§GOAL-002-tiny-footprint)
while still making limits and messages project-owned (§GOAL-005-configurable).

The concrete example shape is maintained in `examples/fissile.toml`.

## 0. Built-in Defaults

A repo with no config still gets a useful guard:

- scan the whole repository, honoring `exclude` and `.gitignore`, so adoption
  needs no guess about which layout a repo uses;
- exclude lockfiles, minified files, vendored directories, build output, VCS
  metadata, and common binary/media assets;
- apply a conservative byte budget to every non-excluded file;
- apply a line budget to common hand-written source extensions, wherever those
  files live, while leaving data and generated formats to the byte budget
  (§GND-001-fissile);
- apply a line budget to markdown under two rules, not one, because there are
  two ways a markdown file is read and they cost different things (§0.1);
- use generic messages that explain how to tune config rather than pretending to
  know the repository's architecture.

These defaults borrow the useful part of generic large-file hooks and platform
file-size guidance: catch obvious accidents immediately. They are not the product
identity. `fissile` earns its keep when projects replace the generic defaults
with named, project-specific rules and messages that can speak to local
architecture.

A hand-written config may omit any field and take its default; an omitted field
is not an error. The config that `fissile init` *generates*, however, is fully
populated — every field is written out at its default so the file is editable
without consulting this spec (§DF-002-explicit-config).

### 0.1 The two markdown reading modes

A line budget prices what a reader pays to load a file. For source that is the
whole file, every time. For markdown it depends on how the file is reached, and
the defaults carry one rule for each way:

- **A citable spec** — a document under `docs/`, or any other markdown that is
  reached by name and section — is opened when someone needs it, and in a
  repository with stable section IDs it is read one section at a time rather
  than whole. Its total length is not charged to a reader who wanted one
  declaration, so it takes the looser budget: **soft 750, hard 2000 lines**.
  The soft tier is not about the cost of the read at all — 750 lines is about
  where a document has started covering two subjects, which is a structure
  problem worth saying out loud. The hard tier is the backstop for a file that
  has stopped being a document.
- **An entrypoint** — `README.md`, `AGENTS.md`, `CLAUDE.md`, a file under
  `skills/`, and the rest of the family §FS-002-init.3 names — is read whole,
  into every agent session, before any work starts. Every line is charged on
  every one of those reads, so it keeps the tight budget: **soft 250, hard 500
  lines**.

The entrypoint selectors are exact filenames and a directory glob, and both
outrank the citable-spec rule's `**/*.md` on the specificity order in §3.2, so
the split needs no `priority`.

750 is roughly 3x the p95 of a spec tree that is already kept tidy, and 2000 is
a common ceiling for a file of any kind, so a spec that reaches it has a problem
no budget can describe. A repository that gives markdown one budget instead
prices a cost nobody pays: it pushes hardest on exactly the projects whose
grounding work made their documents cheap to read.

An append-only record — a changelog — is a third mode and fits neither tier;
splitting one is meaningless. The defaults do not guess at it, and a repository
that keeps one can exclude it from the citable-spec line rule while retaining
the byte catch-all (§3.4).

## 1. Top-level version

Every config starts with:

```toml
fissile_config_version = 1
```

Unknown major versions are a schema error. Version 1 is additive: unknown keys
inside known tables are errors, so a typo cannot silently disable a rule.

Every config diagnostic — parse error, unsupported version, schema error —
names the file it came from (`.agent-grounds/fissile.toml: config parse error: …
at line 100`), so a run driven by `--config` or an editor never leaves the reader
guessing which document broke (§GOAL-003-friendly-output.1).

## 2. Scan scope

`[scan]` controls whole-repo audit traversal:

- `include`: root directories or globs walked by `audit`;
- `exclude`: globs ignored before measurement;
- `respect_gitignore`: whether repository ignore files participate in traversal,
  default `true`.

Pre-commit checks receive their file set from git and do not use `include`, but
they still apply `exclude` so generated assets and lockfiles stay out of the
budget system.

Both `exclude` and `respect_gitignore` prune the walk rather than filter its
result: an excluded or ignored directory is never descended into. This is a
cost guarantee, not only a selection rule — a repository whose ignored build or
scratch directory dwarfs its source (a `target/`, `.venv/`, or `scratchpad/` of
several gigabytes is ordinary) must not pay to traverse it, or the whole-repo
budget in §GOAL-001-fast-feedback.1 is unreachable in exactly the repositories
that most need it. Pruning never changes which findings are emitted: the
contents of a pruned directory would have been discarded anyway.

## 3. Rules

Rules are declared as `[[rules]]` entries. Each rule has:

- `id`: stable machine-readable rule name;
- `include`: one or more globs;
- `exclude`: optional globs removed from this rule's scope, default `[]`;
- `unit`: `bytes`, `lines`, or `tokens`;
- `soft`: optional warning threshold;
- `soft_edit_limit`: optional number of continuous over-soft edits allowed before
  a staged soft finding blocks, default `5`;
- `hard`: optional blocking threshold;
- `priority`: optional integer tie-breaker, default `0`;
- `message`: the ID of a `[[messages]]` template, used for both severities;
- `soft_message`, `hard_message`: optional per-severity overrides of `message`.

At least one of `soft` or `hard` is required. If both are present, `soft <= hard`
is required. Every declared threshold must resolve a message — from its own
severity field or from `message` — or the config is invalid. A file above the
hard limit reports only the hard overflow; the soft overflow is implied
(§GOAL-006-graded-limits).

`soft_edit_limit` is a positive integer and is valid only on a rule with a
`soft` threshold. Omitting it from a version 1 config means `5`, including for
configs written before the field existed. `fissile init` and the maintained
example write it explicitly on every rule with a soft threshold, like every
other default (§DF-002-explicit-config). The setting affects only the bounded
staged-edit grace in §FS-004-check-audit.1.4; it does not add a third size
threshold or change what `check` without `--staged`, `audit`, or the library
checker considers soft or hard.

Rule IDs are user-facing names, not incidental labels. They should read like
bundle-size entries: `rust-source`, `api-docs`, `fixtures`, `generated-rust`.
Findings include the rule ID, JSON output carries it, and exceptions can target
it. A config with anonymous or auto-numbered rules is invalid.

### 3.1 Line Counting Policy

Line rules may define what counts:

- `count_blank_lines`: boolean, default `false`;
- `count_comment_lines`: boolean, default `true`.

The defaults count lines that carry content — code and comments — but ignore
blank separator lines, so readable spacing is never what pushes a file over
budget. Counting comments by default keeps documentation honest about its review
and token cost. Projects may flip either field: set `count_blank_lines = true`
to measure raw physical file size, or `count_comment_lines = false` for a
code-only budget. The policy is per rule because generated docs, tests, and
source files often need different treatment.

Blank- and comment-line classification applies to UTF-8 text. Non-UTF-8 content
still gets a line measurement — physical lines counted from raw bytes, every
line counting as content — so a stray encoding never turns the commit gate into
an error (§FS-004-check-audit.5); the byte catch-all remains the guard that
actually protects against binary blobs (§FS-004-check-audit.3).

### 3.2 Overlapping Rules

A file may match more than one applicable rule. Rule-local exclusions are
applied first (§3.4); overlap is then resolved independently for each measurement
unit (`bytes`, `lines`, `tokens`), because a project may reasonably check one
file by both line count and byte count. For a given `(file, unit)`, `fissile`
selects one effective rule:

1. Higher `priority` wins.
2. If priority ties, the most-specific selector wins:
   - exact path beats glob;
   - deeper or longer glob beats broader glob;
   - extension-only beats catch-all.
3. If specificity still ties, config validation fails with an ambiguity error.

Config file order is never a tie-breaker. Reordering equivalent `[[rules]]`
entries must not change whether a repository passes.

Examples:

```toml
[[rules]]
id = "docs"
include = ["docs/**/*.md"]
unit = "lines"
soft = 250
hard = 500
message = "split-doc"

[[rules]]
id = "api-docs"
include = ["docs/api/**/*.md"]
unit = "lines"
soft = 500
hard = 900
message = "split-api-doc"
```

`docs/guide.md` uses `docs`; `docs/api/openapi.md` uses `api-docs` because the
subfolder glob is more specific.

When specificity is not enough, the config must say which rule wins:

```toml
[[rules]]
id = "generated-rust"
include = ["src/**/*.gen.rs"]
unit = "lines"
soft = 1200
hard = 2000
priority = 20
message = "generated-file"

[[rules]]
id = "domain-rust"
include = ["src/domain/**/*.rs"]
unit = "lines"
soft = 350
hard = 550
message = "split-domain"
```

`src/domain/schema.gen.rs` uses `generated-rust` because its priority is higher.
Without `priority`, this overlap is ambiguous: each glob is specific in a
different dimension, and silently choosing one would make the local architecture
guidance unreliable.

### 3.3 What To Exclude Versus Except

Use `[scan].exclude` for files `fissile` should not reason about:

- vendored code;
- lockfiles;
- minified output;
- generated artifacts whose source is elsewhere;
- binary/media assets;
- build outputs and package caches.

Use the exception registries (§FS-003-exceptions) for files `fissile` should still
reason about, but that are accepted as oversized for a written reason: hand-made
fixtures, intentionally consolidated compatibility layers, generated sources
checked in for bootstrap reasons, or architectural seams that cannot yet be
split. Exclusions need no rationale because the tool does not apply. Exceptions
require rationale because the tool does apply and the repo is choosing to accept
the cost.

Use a rule's `exclude` for a file that should remain in the budget system but
should not be measured by that one rule. An append-only changelog can therefore
leave a line rule without leaving a byte catch-all. This is narrower than
`[scan].exclude`, which removes the file before any rule can apply, and unlike an
exception it does not assert that an overflow is temporarily or structurally
acceptable.

### 3.4 Rule-local Exclusions

A rule applies to a file exactly when at least one of its `include` globs matches
and none of its `exclude` globs matches. Both lists use the same glob semantics
and the same normalized, repository-relative path; an exclusion is not matched
against an absolute, platform-native, or unnormalized spelling of that path.
Omitting `exclude` and writing `exclude = []` are equivalent, so existing version
1 configurations retain their behavior.

Applicability is decided before same-unit priority and specificity resolution
(§3.2). A rule excluded for a path cannot win that path or make its remaining
candidates ambiguous. Only that rule becomes inapplicable: every other rule,
including rules for other measurement units, remains eligible. Checking,
measurement, audit rule coverage, and catch-all-only classification all use this
same applicability decision rather than interpreting rule scope independently.

Every rule still declares at least one threshold. Rule-local negative scope is
expressed directly instead of by a thresholdless, more-specific rule that wins
an overlap and means "do not measure" (§DF-011-rule-local-exclusions).

## 4. Messages

Messages are declared separately as `[[messages]]` entries so multiple rules can
share one remediation message. Each message has:

- `id`: stable message ID included in machine-readable findings;
- `text`: bounded template rendered for humans and agents.

A message has no separate `owner`, `destination`, or `action` fields: the
destination module, ownership boundary, and next step all live in `text`, so the
rendered guidance is a single human-readable string rather than a record the
caller must reassemble.

The supported template variables are `{path}`, `{rule}`, `{severity}`,
`{actual}`, `{limit}`, and `{unit}`. Missing message IDs are schema errors.
Grund citations are part of the message text, not a separate field, so the
rendered guidance remains the single source of human context.
Messages cannot execute code, inspect file contents, or change pass/fail
behavior (§GOAL-008-remediation-messages).

A rule may carry a different message per severity, because "should split" and
"must split" are different instructions with different next steps and different
escape hatches (§DF-003-severity-guidance). Guidance that interpolates a
per-file variable renders differently for every file and so is never grouped in
text output (§FS-004-check-audit.1); the built-in defaults use none, and name
the paths nowhere but the finding lines.

Default message text travels into other repositories through `fissile init`, so
it carries no `§` citation: an ID declared in fissile's own docs resolves
nowhere in the repository that installed it. The generated config marks the slot
where a project adds a citation into its own architecture
(§DF-003-severity-guidance.1).

## 5. Exceptions

`[exceptions]` names the severity-specific oversized-file rationale
registries:

- `soft_registry`: TOML path for soft-limit exceptions, default
  `docs/file-size-agent-exceptions.toml`;
- `hard_registry`: TOML path for hard-limit exceptions, default
  `docs/file-size-human-exceptions.toml`;
- `stale`: `warn`, `error`, or `ignore` for an exception entry that has outlived
  its file. It governs the subject, not one command: `check` reports the entries
  its own run proves are dead (§FS-004-check-audit.1.3) and
  `audit --stale-exceptions` reports the whole inventory
  (§FS-004-check-audit.2), and `error` fails whichever run raised it — including
  a commit, through the pre-commit hook.

`[exceptions.bump]` sets the step each unit's ceilings are quantized to:

```toml
[exceptions.bump]
lines = 100
bytes = 4096
tokens = 1000
```

A ceiling `fissile` writes from a measurement is the smallest multiple of the
unit's step at or above it, so an entry records a chosen round number rather
than one commit's measurement (§FS-005-exception-add.2,
§DF-006-quantized-ceilings); a ceiling stated with `--max` is written as stated
(§DF-010-stated-ceilings-are-exact). The same step bounds the slack before `audit` calls
a ceiling loose (§FS-003-exceptions.7). A step of `1` writes the measurement
exactly. The step governs what the commands write, never what a registry may
hold: any ceiling §FS-003-exceptions.4 already accepts stays valid.

Soft exceptions are for agent-facing warning debt: they keep soft findings from
being repeated when the repository has deliberately accepted the current shape.
Hard exceptions are for human-reviewed blocking debt: they are the only way to
accept a hard-limit overflow without disabling the rule
(§GOAL-007-justified-exceptions). Exceptions are distinct from `[scan].exclude`,
which removes files the tool does not apply to at all. Each exception entry
records a maximum accepted measurement so the finding reappears if the file grows
again. The registry file formats are specified in §FS-003-exceptions.

## 6. Output

`[output]` sets defaults only. Invocation flags may override these values:

- `format`: `text` or `json`;
- `color`: `auto`, `always`, or `never`;
- `success`: the exact success marker for text output, default `ok`.

The machine-readable finding fields, exit-code mapping, and severity model are
not configurable (§GOAL-003-friendly-output).

## 7. Tokens

`[tokens]` is opt-in. With `enabled = false`, token rules are schema-valid but
cannot be evaluated unless the caller supplies token measurements directly. With
`enabled = true`, `command` names an external counter command. `{path}` is
substituted with the file path. The command must print one integer token count.

The default build does not bundle a tokenizer model (§GOAL-002-tiny-footprint).

## 8. Where the Config Lives

The config's home is `.agent-grounds/fissile.toml`. `.agents/` holds agent
instructions and is mounted read-only by sandboxed agent runtimes, so a config
kept there is one the toolchain that owns it cannot maintain
(§DF-012-config-home). `.agents/fissile.toml` remains readable so that no
repository breaks on upgrade, and every run that reads it says it should move.

### 8.1 Discovery Order

Without an explicit path, `fissile` uses the first of these that exists:

1. `<root>/.agent-grounds/fissile.toml`;
2. `<root>/.agents/fissile.toml`;
3. the built-in defaults (§0).

Discovery stops at the first path that is present. A file that exists but does
not parse is an error naming that file (§1), not a miss: falling through to the
next candidate would govern the repository by a document the reader did not
mean to be in force, and say nothing about the one they were editing.

An explicit `--config <path>` is not discovery. The named file must exist, it is
read as given, and it carries no deprecation warning even when it names
`.agents/fissile.toml` — the caller said which document to read, so nothing is
being chosen behind them (§FS-002-init.1, §DF-002-explicit-config).

### 8.2 The Deprecation Warning

A run whose config was discovered at `.agents/fissile.toml` emits exactly one
warning line, naming both paths and the move:

```text
fissile: warning: .agents/fissile.toml is deprecated; move it to .agent-grounds/fissile.toml
```

The line goes to stderr in every mode, including `--format json`. Stdout carries
the findings, and under `--format json` it is a stream a caller parses
(§6, §FS-004-check-audit.1); a warning that entered it would break that caller,
and the deprecation is addressed to the person reading the terminal rather than
to the program reading the output.

It is a warning and never a failure. A deprecated path leaves every exit code
exactly as it was: the run that reports it passes or fails on its findings
alone.

The warning belongs to discovery, not to one command, so every command that
discovers a config carries it — `check`, `audit`, `measure`, `limits`, and the
`exception` family. A repository whose only contact with `fissile` is the
pre-commit hook would otherwise never be told.

### 8.3 Both Paths Present

`.agent-grounds/fissile.toml` takes effect and `.agents/fissile.toml` is not
read. That precedence is stated rather than silent: the run emits one warning
line naming the file it ignored and the one in force.

```text
fissile: warning: .agents/fissile.toml is ignored; .agent-grounds/fissile.toml is the config in effect
```

A config being edited to no effect is the one failure this move could introduce,
and it is invisible from outside: every run succeeds, every rule the reader
wrote is missing from it, and nothing says why. One line is what separates that
from a five-minute answer.
