# FS-006-cli: the top-level command line is small, self-describing, and versioned

`fissile` is one binary with six commands: `init`, `check`, `measure`, `audit`,
`limits`, and `exception`, the last carrying the `add`, `retune` and `remove`
subcommands (§FS-002-init, §FS-004-check-audit, §FS-007-measure,
§FS-010-limits, §FS-005-exception-add, §FS-008-exception-retune,
§FS-009-exception-remove). Everything above the commands — dispatch, help,
version — is specified here so the top level cannot drift between surfaces.

`check` and `measure` are separate commands over the same measurement, because
they answer different questions under different contracts: `check` is a gate that
exits non-zero on a standing hard overflow, `measure` an inspection that always
exits `0` (§FS-007-measure.3). `limits` asks a question over no measurement at
all — it describes the configured rules themselves, for a caller with no file in
hand — and takes the same always-`0` inspection contract (§FS-010-limits.1).

## 1. Dispatch

`fissile <command> [options]` dispatches on the first argument. An unknown
command prints `fissile: unknown command \`<arg>\`` followed by the usage screen
on stderr and exits `2`. No arguments at all prints the usage screen on stdout
and exits `0`.

## 2. Help

`--help`/`-h` at the top level prints the usage screen; after a command it
prints that command's usage with compact examples. Every help screen fits in 24
lines (§GOAL-003-friendly-output.1) and the bound is enforced by a test.
`check`'s screen names its three input modes as exclusive — paths, `--staged`,
`--base <ref>` — and carries a `fissile check --base origin/main` example
(§FS-004-check-audit.1.5).

The top-level screen carries one short paragraph above the command list saying
what `fissile` is for and how to work with it: the two tiers, the
`check --staged` habit, and the rule that a budget is never met by damaging the
design. It closes by pointing at `fissile check --staged` itself.

The paragraph is deliberately short, so the pointer carries the rest. A
repository that installs `fissile` may not carry the managed agent block — the
block is one `init` target among several, and an agent may be reading an
entrypoint that never received it — so the usage screen is the one surface
guaranteed to be present, and it must lead somewhere complete.

What it leads to is a run, not a document. The managed block is three sentences
(§FS-002-init.4) because the instructions live in the surfaces that raise each
question (§DF-007-instructions-at-the-error-site.1), so the complete answer is
the finding: it names the file, the severity, the crossed limit, the remediation
this repository configured, and the command that records an exception. A test
pins the screen's load-bearing clauses.

What left the top-level screen is the pointer at `fissile init --dry-run`, the
one way to read the managed block without writing a file (§FS-002-init.4). It
moves to `fissile init --help`, where the flag it describes already lives.

## 3. Version

`fissile --version` (alias `-V`) prints exactly one line to stdout and exits `0`:

```text
fissile 0.1.0
```

The value is the crate version baked in at compile time, so a bug report, an
upgrade check, and a release self-check all read the same number the registry
shows. The line is stable output in the §GOAL-004-token-thrift sense: no
banner, no build metadata, one token-cheap line. The release pipeline asserts
it against the version being released before publishing an artifact
(§AR-001-ci.8), and an e2e case pins the exact bytes.
