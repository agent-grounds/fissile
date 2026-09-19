# FS-002-init: fissile init installs config, exceptions, and agent instructions

`fissile init` bootstraps a repository for commit-time file-size discipline. It
writes the starter config file, creates the exception registries if requested,
and adds a versioned managed block to agent instruction files so coding agents
know how to react to soft and hard overflows. It follows the same non-intrusive shape
as `grund init`: preserve user-authored content, update only managed blocks, make
every choice a flag, and support `--dry-run`.

## 1. Inputs

```text
fissile init [<path>] [--name <name>] [--force] [--dry-run]
             [--config <path>] [--exceptions] [--hook] [--no-hook]
             [--agents-md] [--claude] [--gemini] [--copilot]
             [--cursor] [--windsurf] [--zed]
```

- `<path>` is the repository root to initialize. It defaults to `.` and must
  already exist.
- `--name <name>` is the human-readable project name used in a newly created
  `AGENTS.md` heading. It defaults to the target directory basename.
- `--config <path>` changes the config path written under `<path>`. The default
  is `.agent-grounds/fissile.toml` (§FS-001-config.8).
- `--exceptions` also creates the configured soft and hard exception registry
  paths when absent.
- `--hook` forces installation of the managed pre-commit hook (§6) and errors if
  the target is not a git repository. `--no-hook` suppresses the install that
  automatic mode would otherwise perform. The two flags are mutually exclusive.
- `--force` refreshes managed agent blocks and generated starter files. It does
  not overwrite an existing config or any existing exception registries.
- `--dry-run` reports what would be written, appended, or updated without
  changing the filesystem.
- Agent flags explicitly select entrypoint families. Without any agent flag,
  automatic mode updates existing known entrypoints and otherwise creates
  `AGENTS.md`.

`init` is non-interactive. It never prompts and never guesses beyond the automatic
entrypoint selection rules in §3 and the automatic hook install in §6.

## 2. Files

Default `fissile init` writes:

- one agent entrypoint or managed block, per §3;
- `<path>/.agent-grounds/fissile.toml`, when absent, using the schema from
  §FS-001-config. The generated config is fully populated: every schema field is
  written at its default value, ready to edit in place, rather than a minimal
  skeleton (§DF-002-explicit-config).

With `--exceptions`, it also writes the configured exception registries, default
`docs/file-size-agent-exceptions.toml` and
`docs/file-size-human-exceptions.toml`, when absent. Each starter registry
contains `fissile_exceptions_version = 2`, explanatory comments, and no
exception entries.

An existing config and existing exception registries are project-owned. They
are reported as `exists` and left byte-for-byte unchanged, even with `--force`.

A config at the deprecated `.agents/fissile.toml` (§FS-001-config.8) is that
same existing config, so a repository that has one and no
`.agent-grounds/fissile.toml` gets **no** config written. `init` reports the old
path as `exists`, leaves it byte-for-byte, and emits the deprecation warning so
the reader knows to move it. Generating the fully populated default at the new
path instead would take precedence over the project's own rules on the very next
run — a config silently replaced by generic limits, which is the one outcome
this promise exists to prevent.

## 3. Agent Entrypoints

`AGENTS.md` is the one file that holds the block; every other entrypoint `init`
touches is a symbolic link to it, so a project states its instructions once and
every agent reads the same bytes (§DF-009-one-file-agents-read). The known set is:

- `AGENTS.md`
- `AGENTS.override.md`
- `CLAUDE.md`
- `.claude/CLAUDE.md`
- `GEMINI.md`
- `.github/copilot-instructions.md`
- `.cursor/rules/fissile.mdc`
- `.cursorrules`
- `.windsurfrules`
- `.rules`

Explicit flags create or update the requested families. Automatic mode creates
workspace-triggered aliases only when the matching tool-specific directory
already exists: `.claude/`, `.gemini/`, `.cursor/`, or `.zed/`. It does not
create `.github/copilot-instructions.md` merely because `.github/` exists, and
it does not create `.windsurfrules` or `.rules` without an explicit flag or
workspace signal. `AGENTS.md` is written whichever families are asked for: a
link has to point at something.

Each link is relative to the file that carries it — `AGENTS.md` beside the root,
`../AGENTS.md` from `.claude/` — so cloning or moving the tree keeps it resolving.

A link is laid over a companion only when nothing is lost. It is created when the
companion does not exist; left alone when it already links to `AGENTS.md`; and
laid over a regular file in exactly two cases — the file's bytes match
`AGENTS.md`, so a copy becomes a link to the original; or `AGENTS.md` does not
exist yet and this is the only entrypoint that does, in which case its bytes
*become* `AGENTS.md`, since that content is what the project already told agents.

Any other companion holding bytes of its own is kept as a regular file with the
block written into it, and the run says it was kept: overwriting would destroy
what §4 promises to preserve, and `init` cannot know whether two entrypoints
disagree on purpose. Where the filesystem refuses a link — Windows without
Developer Mode — `init` writes the block into the file and says so rather than
failing, because a duplicated block beats no instructions.

## 4. Managed Block

The managed block is delimited by named markers, with its version stated on the
first line of the content:

```markdown
<!-- BEGIN FISSILE MANAGED BLOCK -->
## Keeping Files Small With fissile (v3)
...
<!-- END FISSILE MANAGED BLOCK -->
```

That is the shape generated Markdown regions take everywhere — `terraform-docs`,
`doctoc`, `all-contributors`, and `grund` all use named, symmetric,
version-free HTML comments — and where a generator states a version, it states
it inside the region, as `protoc-gen-go` does on its first comment lines. The
markers say which tool owns the span; the heading says which generation of the
text it is. Nothing has to parse a delimiter to answer the second question.

The hook block (§6) states its version in its marker instead. A shell file has
no heading to carry it, and `# >>> ... >>>` is the shape `conda init` writes
into the same kind of file.

The block is exactly the marker lines and everything between them. Every byte
outside is user-authored and preserved, including a heading of any depth
written directly beneath the block. A fresh `AGENTS.md` may have an unmanaged
H1 above it; companion entrypoints contain only the managed block unless they
already had user-authored content.

If an entrypoint has no managed block, `init` appends the current block. If it
has a supported block version, `init` replaces only the block and preserves the
bytes before and after it, including the block position. If it has a newer
unsupported block version, `init` exits with a schema error and leaves the file
unchanged.

A block between our markers that states no version this build can read is
unsupported too. The markers carry no version, so there is nothing to fall back
to: a later generation that renames the heading would otherwise be read as
current and overwritten wholesale, which is the downgrade the paragraph above
rules out. The error names the file and says the block declares no readable
version.

A begin marker with no end marker is a truncated block, and it is replaced
wholesale — leaving part of the old body outside the new markers would put a
duplicate in the file that no later run can clean up. How far it runs depends on
what else bounds a block in that file. In Markdown, headings do: the span ends
at the next H1 or H2 that is not the block's own, or at end of file, so sections
a user wrote below survive. The hook block (§6) has no such boundary — a shell
file has no headings, and every `# ` comment in one would be mistaken for one —
so its truncated span runs to end of file.

Blocks v1 and v2 had no markers; the heading was the whole boundary, and the
span ran to the next H1 or H2 heading, or end of file. `init` still recognizes
that span and replaces it with the delimited block, so a repository that adopted
an earlier version upgrades in place on its next run rather than growing a
second block. This is why the heading keeps a fixed prefix: it is the one line
present in every version of the block.

The v3 block teaches three things, and deliberately no more
(§DF-007-instructions-at-the-error-site.1):

- that this repository caps file size, and why — the one fact that has to arrive
  before the code is written, since a finding cannot reach an agent until the
  file is already too long;
- that `fissile check --staged` is run before calling work done, which reaches
  the repositories where no hook fires (§6);
- that the gate is not bypassed with `--no-verify`.

The block is written by every `init` run, including the ones that install no
hook — `--no-hook`, and a target that is not a git repository (§6) — so it
states the habit as the instruction and the hook as the conditional. A block
asserting a pre-commit hook that the same run just reported it did not install
(§5) would be the first thing an agent read about this repository and false.

Everything the v2 block said about severities, seams, exceptions, ceilings, and
stale entries is said by the surface that raises each one: the finding and its
guidance (§FS-004-check-audit.1), the `exception add` refusals
(§FS-005-exception-add.4), and the `audit` inventory (§FS-004-check-audit.2).

An example rendered block lives at `examples/AGENTS.fissile.md`.

`--dry-run` prints that block to stdout, under the planned writes, which is the
one way to read it without changing a file. The text is the same constant `init`
installs, so the printed and the written block cannot drift. `fissile init
--help` says so, since the top-level screen leads to a run rather than to a
document (§FS-006-cli.2) and would otherwise leave the flag undiscoverable.

## 5. Reporting

On success, stderr reports one path per line:

```text
wrote .agent-grounds/fissile.toml
appended AGENTS.md
```

Prefixes are `wrote`, `appended`, `updated`, and `exists`. Under `--dry-run`,
write prefixes become `would-write`, `would-append`, and `would-update`; `exists`
is unchanged. Stdout is empty.

After a run that wrote, appended, or updated something, stderr prints a short
`next:` block:

```text
next:
1. Review .agent-grounds/fissile.toml and tune rule limits.
2. Commit a change to see the pre-commit hook run fissile check --staged.
3. Run fissile audit once and add justified exceptions with fissile exception add.
see AGENTS.md for what agents are told; the findings carry the rest.
```

The `next:` block is suppressed when every selected file already exists with the
current managed block.

The block must not promise machinery the run did not install, and must not send
the reader to a file that is not there. Four clauses follow from that:

- Step 1 names the config path this run wrote or found, not a fixed filename:
  `--config` moves it, and a repository still on the deprecated
  `.agents/fissile.toml` (§FS-001-config.8) keeps its config where the default
  no longer looks. Naming the literal would send the reader to a file the run
  did not touch.
- Step 2 reports the hook the run leaves behind (§6), not the flag it was
  given: a managed block there — this run's, or an earlier run's that
  `--no-hook` declined to touch — earns the invitation above.
- With no such block, step 2 says what to do instead, and `--no-hook` answers
  first when both apply: `--no-hook skipped the managed hook; wire fissile
  check --staged into your commit flow — a hook manager or core.hooksPath, if
  this repo uses one.` Outside a git repository (§6) the step is the repair —
  `Run git init && fissile init to install the pre-commit hook, or wire fissile
  check --staged into your commit flow.` Neither asserts a hook manager: `init`
  looks for none, and `--no-hook` is equally the flag of a reader who wants no
  hook at all.
- The closing `see <path> for what agents are told; the findings carry the
  rest.` line names an agent entrypoint this run handled (§3), not a fixed
  filename: automatic mode
  updates whichever entrypoints already exist and only falls back to
  `AGENTS.md` when none do, and the per-agent flags select the file directly.
  Naming a handled entrypoint — whether it was written, updated, or already
  current — is what makes the path resolve. When several were handled, the
  first in the reported order is named; when none were, the line is omitted
  rather than invented.

## 6. Pre-commit Hook

The headline use case is a commit-time gate (§GND-001-fissile), so `init`
installs it rather than only describing it. The hook is a managed block inside
`hooks/pre-commit` in the repository's common git directory, delimited by
begin/end markers so it composes with hooks a project already maintains:

```sh
# >>> fissile managed block (v1) >>>
fissile check --staged || exit 1
# <<< fissile managed block (v1) <<<
```

- **When it installs.** Automatic mode installs the hook when `<path>/.git`
  names a repository — a directory, or a `gitdir:` file (linked worktree or
  submodule) pointing at one — and skips silently otherwise (no git
  repository, nothing to hook). The hook goes into that repository's shared
  `hooks/pre-commit`: `<path>/.git/hooks/` for a plain checkout, the main
  repository's for a linked worktree. `--hook` forces the install and errors
  outside a git repository; `--no-hook` suppresses the automatic install.
- **How it edits.** When `pre-commit` is absent, `init` writes it with a
  `#!/bin/sh` shebang above the block and marks it executable. When it exists
  without the block, `init` appends the block and preserves prior content. When
  it exists with a supported block version, `init` replaces only the block,
  preserving bytes before and after. A newer unsupported block version is a
  schema error that leaves the file unchanged, exactly as in §4.
- **Reporting.** The hook path uses the same prefixes as §5
  (`wrote`/`appended`/`updated`/`exists`) and honors `--dry-run`.

`init` targets the repository's own `hooks/pre-commit` only. A repository that
relocates hooks via `core.hooksPath` or drives them through a hook manager
should wire `fissile check --staged` through that manager instead. A hook
manager that enforces bounded soft-edit promotion must run that command without
injecting filenames; for pre-commit this means `pass_filenames: false`. A hook
that calls `fissile check <paths>` is deliberately a working-tree snapshot
check and keeps soft findings advisory (§FS-004-check-audit.1.4).
