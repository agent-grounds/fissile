# FS-001-config: fissile reads a versioned TOML config file

`fissile` is configured by a single TOML document. Its home is
`.agent-grounds/fissile.toml`, and the former home `.agents/fissile.toml` is
still read and reported as deprecated (§FS-001-config.8); an embedding CLI may also pass an
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
  two ways a markdown file is read and they cost different things (§FS-001-config.0.1);
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
outrank the citable-spec rule's `**/*.md` on the specificity order in §FS-001-config.3.2, so
the split needs no `priority`.

750 is roughly 3x the p95 of a spec tree that is already kept tidy, and 2000 is
a common ceiling for a file of any kind, so a spec that reaches it has a problem
no budget can describe. A repository that gives markdown one budget instead
prices a cost nobody pays: it pushes hardest on exactly the projects whose
grounding work made their documents cheap to read.

An append-only record — a changelog — is a third mode and fits neither tier;
splitting one is meaningless. The defaults do not guess at it, and a repository
that keeps one can exclude it from the citable-spec line rule while retaining
the byte catch-all (§FS-001-config.3.4).

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

- `include`: root directories or globs walked by `audit` and by a `check` that
  names no files;
- `exclude`: globs ignored before measurement;
- `respect_gitignore`: whether repository ignore files participate in traversal,
  default `true`.

Every `include` entry resolves from the repository root (§FS-001-config.8.1),
never from the directory the command was started in. A run that walks the scope
sees the whole scope from every directory of the work tree: with
`include = ["src"]`, `audit` started in `src/` walks `src`, not `src/src`, and
says what it says at the root. Where the command starts selects nothing; the
paths a caller names do (§FS-007-measure.1).

Pre-commit checks and `check --base <ref>` receive their file set from git and
do not use `include` or gitignore, but they still apply `exclude` so generated
assets and lockfiles stay out of the budget system (§FS-004-check-audit.1.5).

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
  a staged or base soft finding blocks, default `5`;
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
edit grace of `check --staged` and `check --base` in §FS-004-check-audit.1.4; it does not add
a third size threshold or change what any other `check`, `audit`, or the
library checker considers soft or hard.

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
applied first (§FS-001-config.3.4); overlap is then resolved independently for each measurement
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
(§FS-001-config.3.2). A rule excluded for a path cannot win that path or make its remaining
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

Both registry paths are relative to the repository root (§FS-001-config.8.1).
They are not relative to the config file, or to the directory the command was
started in. Every command reads the registries there, and `exception add`,
`retune` and `remove` write them there, from whichever directory of the work
tree they run.

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
It runs with the repository root (§FS-001-config.8.1) as its working directory,
whichever directory `fissile` was started from. So a counter the config names by
a repository-relative path is found from every directory, as is the path it is
handed.

The default build does not bundle a tokenizer model (§GOAL-002-tiny-footprint).

## 8. Where the Config Lives

The config's home is `.agent-grounds/fissile.toml`. `.agents/` holds agent
instructions and is mounted read-only by sandboxed agent runtimes, so a config
kept there is one the toolchain that owns it cannot maintain
(§DF-012-config-home). `.agents/fissile.toml` remains readable so that no
repository breaks on upgrade, and every run that reads it says it should move.

Discovery speaks for itself. It reports a deprecated home
(§FS-001-config.8.2), a config passed over at the root (§FS-001-config.8.3) or
below it (§FS-001-config.8.4), and a run on the built-in defaults
(§FS-001-config.8.5). Those lines are built in one place, from the root the run
loaded. So every command that discovers a config prints the same lines, in the
order of the points that define them. They go to stderr before anything the
run found, and none of them changes stdout or an exit code.

### 8.1 Discovery Order

Without an explicit path, `fissile` uses the first of these that exists:

1. `<root>/.agent-grounds/fissile.toml`;
2. `<root>/.agents/fissile.toml`;
3. the built-in defaults (§FS-001-config.0).

`<root>` is the top of the Git work tree the command was started in, from
whichever of its directories. Run from `sub/`, `fissile` reads the document it
reads at the top and holds the repository to it: where the shell stands never
changes the policy that governs the repository. The root is not the nearest
directory holding a config. That would let a subdirectory's config decide the
verdict on the repository-wide file set `check --staged` takes from Git
(§FS-004-check-audit.1.6). Git draws the boundary:

- a nested repository or a submodule is its own root, whether or not it holds a
  config, and discovery never continues into the repository around it;
- a linked worktree's root is its own checkout, not the main worktree's;
- outside a Git work tree, or when Git cannot answer, the root is the directory
  the command was started in, and no ancestor is searched.

A directory the enclosing repository ignores is part of that repository unless
it is a repository itself, as it is for `git status`. Run there, `fissile`
governs the enclosing repository. To hold such a directory to its own config,
`git init` it, or set `GIT_CEILING_DIRECTORIES` so that Git does not find the
repository around it.

Every repository-relative path a run reads is resolved from the root: the scan
scope (§FS-001-config.2), the registries (§FS-001-config.5) and the token
counter (§FS-001-config.7). What a run reports about the repository keeps its
repository-relative spelling from every directory: a finding's file, an
entry's `path`, a registry as the config names it. A path that locates a config
document, the one read or one passed over, is the exception. It is spelled so
that it opens from where the caller stands, so a config error met from `sub/`
names `../.agent-grounds/fissile.toml`. A path inside a command the run offers
is spelled so that the command runs as printed from where the caller stands:
the caller's own argument as typed, an exact entry's path spelled from the
caller's directory (`../other/x.py` from `sub/`), and a glob as written. At the
root every such path, and every byte a run prints, is what it was before the
root was defined.

Discovery stops at the first path that is present. A file that exists but does
not parse is an error naming that file (§FS-001-config.1), not a miss: falling through to the
next candidate would govern the repository by a document the reader did not
mean to be in force, and say nothing about the one they were editing.

An explicit `--config <path>` is not discovery. A relative path is resolved
once, against the directory the command was started in, and that one file is
what the whole run reads: the current tree and, under `audit --history`, every
revision (§FS-004-check-audit.2.1). `--config x.toml` from `sub/` is `sub/x.toml`
in both. It changes which document is read, not the root its paths resolve
from. A file outside the work tree still loads for the current tree, and
`--history` still refuses it. The named file must exist, it is read as given,
and it carries no deprecation warning even when it names
`.agents/fissile.toml` — the caller said which document to read, so nothing is
being chosen behind them (§FS-002-init.1, §DF-002-explicit-config).

### 8.2 The Deprecation Warning

A run whose config was discovered at `.agents/fissile.toml` emits exactly one
warning line, naming both paths and the move:

```text
fissile: warning: .agents/fissile.toml is deprecated; move it to .agent-grounds/fissile.toml
```

That is the line at the root, byte for byte. From below the root
(§FS-001-config.8.1) both paths are spelled from where the caller stands, so
the move it names can be made from there:

```text
fissile: warning: ../.agents/fissile.toml is deprecated; move it to ../.agent-grounds/fissile.toml
```

The line goes to stderr in every mode, including `--format json`. Stdout carries
the findings, and under `--format json` it is a stream a caller parses
(§FS-001-config.6, §FS-004-check-audit.1); a warning that entered it would break that caller,
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

From below the root both paths are spelled from where the caller stands, as in
§FS-001-config.8.2: `../.agents/fissile.toml is ignored;
../.agent-grounds/fissile.toml is the config in effect`.

A config being edited to no effect is the one failure this move could introduce,
and it is invisible from outside: every run succeeds, every rule the reader
wrote is missing from it, and nothing says why. One line is what separates that
from a five-minute answer.

### 8.4 A Config Below the Root

Discovery reads the root (§FS-001-config.8.1), so a config in the invocation
directory, the one the command was started in, is not read when that directory
is below the root. For
the reason §FS-001-config.8.3 gives, that is stated rather than silent: the run
emits one warning line. It names the file passed over and what is in effect
instead, each spelled from where the caller stands.

```text
fissile: warning: .agent-grounds/fissile.toml is ignored; discovery reads the repository root, and ../.agent-grounds/fissile.toml is the config in effect
```

What is in effect is the document discovery read at the root, which is
`../.agents/fissile.toml` when that is the one. When the root has neither, the
line ends `and the built-in defaults are in effect`.

Either home counts. A `.agents/fissile.toml` in the invocation directory is
passed over the same way and named the same way. When both are there, the line
names `.agent-grounds/fissile.toml`, the one discovery would read in that
directory (§FS-001-config.8.3), and it is still one line, never two. Only the
invocation directory is looked at, not the directories between it and the root.

There is no such line at the root, or outside a Git work tree, where the
invocation directory is the root. There is none with `--config` either: it names
the document itself, and it is still how a caller selects the file passed over.
Like §FS-001-config.8.2 the line goes to stderr in every mode and changes no
exit code.

### 8.5 The Built-in Defaults Are Announced

A run that found no config at the root (§FS-001-config.8.1), and so holds the
repository to the built-in defaults (§FS-001-config.0), says so in one line on
stderr:

```text
fissile: note: no .agent-grounds/fissile.toml in /work/repo; using the built-in defaults
```

The path is the root, absolute. Which directory was searched is exactly what is
in question, as in §FS-004-check-audit.2.2: a run that landed on the defaults
because it started outside the repository it meant is told so by that path. The
line names the home a config should be created at, not the deprecated one,
though both were looked for.

The note is said where a deprecation warning would be (§FS-001-config.8.2): by
every command that discovers a config, in text and JSON, dry runs included, once
per run, before stdout. It is not said by `init`, `--help` or `--version`, which
discover nothing. A run that aborts with a run-level error does not say it
either: it prints only its diagnostic (§FS-004-check-audit.5). Like a
deprecation warning it is said and never charged. Stdout, the success marker
and every exit code are exactly what they would be without it.

An explicit `--config` prints no note, even when it names a copy of the
defaults. Neither does a config that omits fields: an omitted field takes its
default (§FS-001-config.0), but the document is the repository's own.
