# Goals

What this checker measures itself against. If a change does not advance one of these, it is not worth doing. Goals are declared inline below; each is a stable ID and may be cited from anywhere in the repo.

Current goals:

- [§GOAL-001-fast-feedback](goals.md#goal-001-fast-feedback-the-hook-is-imperceptible)
- [§GOAL-002-tiny-footprint](goals.md#goal-002-tiny-footprint-one-small-static-binary-no-runtime)
- [§GOAL-003-friendly-output](goals.md#goal-003-friendly-output-tell-the-user-exactly-what-broke-and-how-to-fix-it)
- [§GOAL-004-token-thrift](goals.md#goal-004-token-thrift-the-tool-itself-spends-as-few-tokens-as-it-saves)
- [§GOAL-005-configurable](goals.md#goal-005-configurable-every-limit-and-message-overridable-per-file-type-and-path)
- [§GOAL-006-graded-limits](goals.md#goal-006-graded-limits-soft-warns-hard-blocks-ai-minimizes)
- [§GOAL-007-justified-exceptions](goals.md#goal-007-justified-exceptions-every-oversized-file-has-a-written-reason)
- [§GOAL-008-remediation-messages](goals.md#goal-008-remediation-messages-overflows-can-carry-local-remediation-guidance)

## GOAL-001-fast-feedback: the hook is imperceptible

A pre-commit hook is run by a human waiting on a prompt. If it is slower than a heartbeat the human routes around it — `--no-verify`, uninstall, or quiet resentment — and the protection [§GND-001-fissile](grund.md#gnd-001-fissile-steer-agents-toward-leaner-files--fewer-tokens-architecture-intact) promised never lands. So speed is not a target; it is the *ordering principle* of this tool. When a design choice trades clarity, generality, or features for raw throughput on the hook path, raw throughput wins.

### 1. Performance targets

- Under **50 ms** on a typical commit (a handful of staged files on a developer laptop). The hook must be invisible relative to git's own cost.
- Under **500 ms** for a 10k-file batch through the library core. The whole-repo scan a CLI builds on top must be cheap enough to run from a watch loop or a CI smoke job, not just nightly.
- Single allocation per file at most; zero allocations on the hot size-check path where possible.

### 2. How we get there

- Linear pass per file, no second walks. Size in bytes is `stat`; size in lines is a streaming `memchr` for `\n`; both stop the moment the budget is exceeded.
- Skip files that obviously cannot violate the budget (size in bytes < smallest configured limit) before opening them.
- Parallel walk via `rayon` once the single-thread version stops winning on real repos.
- No regex on the hot path. Glob match for path → rule is compiled once and reused.

### 3. Measurable

Manual timing on the library core and on a synthetic 10k-file batch should stay within the targets above. The instruction-counting `cargo bench` harness over the hot paths ([§AR-002-instruction-benchmarks](architecture/AR-002-instruction-benchmarks.md#ar-002-instruction-benchmarks-instruction-counting-benchmarks-for-hot-library-paths), run in CI by [§AR-001-ci.5](architecture/AR-001-ci.md#5-benchmark-job)) records the per-commit number and fails pull requests that regress beyond the configured threshold. Alongside it, CI runs a release-mode 10k-file smoke test under a generous timeout ([§AR-001-ci.4](architecture/AR-001-ci.md#4-performance-smoke-guard)) so a catastrophic regression — an accidental quadratic path or a repeated pass over every file — fails the build outright.

## GOAL-002-tiny-footprint: one small static binary, no runtime

This tool is dropped into other people's repos. Every megabyte it adds and every runtime it requires is friction that the next contributor pays. So: a single statically-linked binary, on the order of a few MB stripped, with no Python/Node/JVM dependency at install time and a dependency tree small enough to audit on one screen. This is the *adoption* contract for [§GND-001-fissile](grund.md#gnd-001-fissile-steer-agents-toward-leaner-files--fewer-tokens-architecture-intact): a tool that nags you to keep your repo small had better keep itself small first.

### 1. Hard requirements

- One artifact per platform. Linux / macOS / Windows, each a single executable.
- No shared-library dependency beyond libc.
- No bundled tokenizer model files in the default build. Token-mode support, when on, links a small embedded tokenizer or shells out to a configured one ([§GOAL-005-configurable](goals.md#goal-005-configurable-every-limit-and-message-overridable-per-file-type-and-path)); the default binary stays small.
- Cold start under **10 ms** on a cached binary. Anything slower turns up in the [§GOAL-001-fast-feedback](goals.md#goal-001-fast-feedback-the-hook-is-imperceptible) budget.

### 2. What this rules out

Plugin systems, scripting languages embedded in the checker, "extensible" detector pipelines. Configuration via [§GOAL-005-configurable](goals.md#goal-005-configurable-every-limit-and-message-overridable-per-file-type-and-path) is data, not code; if a project needs custom logic, it can shell out to another binary from its own pre-commit config — not from inside this one.

### 3. Measurable

CI publishes the stripped binary size per platform on every release. A regression past a documented threshold fails the build. The dependency count in `Cargo.lock` (excluding stdlib and proc-macros) stays bounded and is reviewed at release time.

## GOAL-003-friendly-output: tell the user exactly what broke and how to fix it

A pre-commit hook that says "rejected" is worse than no hook at all. When the checker stops a commit, the contributor must know — without re-running anything, without reading docs — which file, which limit, by how much, and whatever guidance the project configured for that rule. This is the half of the contract a human reads. The other half ([§GOAL-004-token-thrift](goals.md#goal-004-token-thrift-the-tool-itself-spends-as-few-tokens-as-it-saves)) is what an agent reads; the two compose.

### 1. Hard requirements

- **Errors point at the file.** Every line diagnostic leads with `<path>: <actual> <counting basis> (budget <limit>)`; byte and token diagnostics lead with `<path>: <actual> <unit>` (§FS-004-check-audit). Past that prefix each one names the ceiling a `fissile exception add` with no `--max` would accept for the file, so a reader choosing between the plain form and a stated ceiling is never left to compute what the command already knows, and it is withheld only where that call would be refused (§FS-004-check-audit.1). Editors and agents jump to the path unmodified.
- **The fix is named.** When a limit is crossed, the diagnostic names the rule that owns the budget and the message that produced the remediation text, so the reader can find both in the config without re-running anything.
- **The configured guidance is present.** An overflow includes whatever message the rule names — which may point at a destination module, ownership boundary, or extraction pattern, or may be deliberately generic ([§GOAL-008-remediation-messages](goals.md#goal-008-remediation-messages-overflows-can-carry-local-remediation-guidance)).
- **Output is parseable.** A `--format json` flag emits a stable JSON shape, one record per violation for `check`, suitable for LLM consumption and editor integration.
- **Help is one screen.** `fissile --help` fits in 24 lines; subcommand help includes compact examples.
- **Explicit success.** A passing text run prints exactly `ok` on stdout; the JSON form stays diagnostics-only.

### 2. What this rules out

Configurable severity levels (would let two installs disagree on whether a repo passes), interactive prompts on the hook path (would block CI), and "did you mean" suggestions on config keys (a clean schema error is shorter and more honest).

### 3. Measurable

Every diagnostic emitted by the tool is matched by an e2e fixture asserting its exact finding shape and selected message ID. The `--help` screen length is enforced by a test. The JSON schema is published alongside the binary.

## GOAL-004-token-thrift: the tool itself spends as few tokens as it saves

The whole point of this checker is to lower the token cost of working in the repo. A checker whose own output is verbose, repetitive, or wraps each finding in generic preamble is at war with its own purpose. So: every byte the tool prints, an agent will eventually read — make each one carry weight. Custom messages are allowed because they replace rediscovery with the project's own guidance, but they stay bounded and structured.

### 1. What this requires

- **One compact record per finding.** A line violation has one required finding line leading with `<path>: <actual> <counting basis> (budget <limit>)`; byte and token violations lead with `<path>: <actual> <unit>`. The ceiling a plain exception would accept is a clause on that same line, never a line of its own (§FS-004-check-audit.1). Text output may add one configured guidance line; JSON carries the same message fields in the record.
- **No re-statement of inputs.** The tool does not echo back its configuration or the list of files it scanned unless the caller asks for inventory sections such as `audit --top`.
- **`--format json` is the agent surface.** Agents are nudged toward JSON; `check` uses a flat array of records and `audit` uses a compact object for optional inventory sections.
- **The audit summary is compact.** Default `audit` emits the same finding surface as `check`; optional inventory sections are short lists, not paragraphs.
- **Stable byte output.** Same `(tree, config)` → same bytes. Findings are sorted deterministically (path, then rule name). An agent that diffs two runs sees only the real change.

### 2. What this rules out

Progress bars on the hook path (an agent piping the output gets a wall of escape sequences). Generic "helpful" copy ("Looks like you've added a large file!"), emoji status, and unbounded templates. Output that varies with terminal width on stdout (color/TTY affordances stay on stderr, opt-out via `--no-color`).

### 3. Measurable

On a 10k-file fixture with N violations, total stdout is under `N * 240` bytes plus a one-line trailer. An e2e fixture pipes the JSON output of `check` into `check` again (idempotence of the violation list) and diffs to zero.

## GOAL-005-configurable: every limit and message overridable per file type and path

Sensible defaults out of the box; full override when a project's reality diverges. A 2,000-line generated SQL dump is fine and a 2,000-line hand-written TypeScript module is not, and the tool earns its keep by telling those apart; when a project wants, a rule's message can also explain the local split. Configuration is data — a single TOML file at the repo root — not a plugin surface ([§GOAL-002-tiny-footprint.2](goals.md#2-what-this-rules-out)).

### 1. What is configurable

- **Limits per unit.** Bytes, lines, or tokens. A rule names the unit it uses; mixing is allowed across rules but not within one.
- **Limits per file type.** Per extension (`.ts`, `.py`, …) and per glob (`src/**/*.gen.rs`). Each rule carries both a soft and a hard limit ([§GOAL-006-graded-limits](goals.md#goal-006-graded-limits-soft-warns-hard-blocks-ai-minimizes)).
- **Exclusions.** Globs that opt files out of being checked at all — lockfiles, vendored code, generated artifacts, binaries. Exclusions need no rationale because the tool obviously does not apply; defaults cover the obvious cases so a fresh install does not immediately false-positive. Distinct from *exceptions* ([§GOAL-007-justified-exceptions](goals.md#goal-007-justified-exceptions-every-oversized-file-has-a-written-reason)), which keep the file under check but accept it as oversized for a written reason in the soft or hard registry.
- **Overflow messages.** Each rule may name a message template. Templates are short text blocks with stable IDs and inline citations when useful; the destination, owner, and split hints live in the message text.
- **Scope.** Which directories are walked by `audit`; pre-commit always scopes to staged files.
- **Output defaults.** Format (`text` / `json`) and color, overridable per invocation.

### 2. What is NOT configurable

Per [§GOAL-003-friendly-output.2](goals.md#2-what-this-rules-out), the severity model, exit-code mapping, and machine-readable diagnostic fields are fixed. Two correctly-configured installs must agree on whether a repo passes. The human guidance text is intentionally project-owned.

### 3. Composition with the others

Configurability is the safety valve that lets [§GOAL-001-fast-feedback](goals.md#goal-001-fast-feedback-the-hook-is-imperceptible) hold its ground: the hot path stays fast because the user can exclude the file types that would otherwise force expensive checks (token counting on a vendored corpus, line counting on a minified bundle). It is also what keeps [§GOAL-002-tiny-footprint](goals.md#goal-002-tiny-footprint-one-small-static-binary-no-runtime) honest — a tokenizer is opt-in, not bundled by default.

### 4. Measurable

An e2e fixture with a non-default config (custom per-extension limits, a glob exclusion, a token-unit rule, and a custom overflow message) passes (`tests/e2e/cases/E2E-009-check-token-rule`). Because the token-unit rule needs an external counter ([§DA-001-token-external-command](decisions/architectural/DA-001-token-external-command.md#da-001-token-external-command-token-counting-shells-out-no-tokenizer-is-bundled)) and a portable counter stub is not cross-platform, that fixture is Unix-gated — the same Linux-only concession the instruction benchmarks already make ([§DA-002-instruction-count-benchmarks](decisions/architectural/DA-002-instruction-count-benchmarks.md#da-002-instruction-count-benchmarks-ci-meters-performance-in-instructions-not-wall-clock)); token mode itself is cross-platform. The default config — applied implicitly when no config file is present — is checked into the e2e suite and snapshot-tested so a silent change to defaults fails CI.

## GOAL-006-graded-limits: soft warns, hard blocks, AI minimizes

A single threshold is a false economy. Set it low and the tool nags constantly until contributors disable it; set it high and the tool catches nothing until a file is already a problem. So this checker carries two limits per rule: a **soft** limit that warns, and a **hard** limit that fails. The contract is asymmetric on purpose — the warning is for the AI agent who can refactor cheaply mid-edit, the failure is for the human reviewer who can't be relied on to notice.

### 1. The two tiers

- **Soft limit.** A file strictly above the soft limit emits a warning. A file exactly at the limit passes. On the first four continuous over-soft staged edits the default remains advisory; the fifth is promoted into a commit block, provided complete Git history proves the run (§FS-004-check-audit.1.4). The diagnostic names the file, size, soft limit, rule, edit count, and configured edit limit. An AI agent reading it is expected to reduce the file — split it, extract a helper, prune dead code — or record the soft debt before claiming the task done. The friction is intentionally bounded in both directions: one encounter does not block, and repeated deferral does not remain free forever.
- **Hard limit.** A file strictly above the hard limit fails the commit; a file exactly at it passes. There is no override flag and no severity knob ([§GOAL-003-friendly-output.2](goals.md#2-what-this-rules-out)). The only way past it is the structured hard exception registry of [§GOAL-007-justified-exceptions](goals.md#goal-007-justified-exceptions-every-oversized-file-has-a-written-reason).
- **Order.** A file above the hard limit reports only the hard violation; the soft warning is implied. If a hard exception silences that hard violation, the soft warning appears again unless the soft registry also accepts it — or unless the accepting entry is `structural`, which says the split is illegal and there is therefore nothing left to ask for (§FS-003-exceptions.3).

### 2. The AI-minimize contract

The warning is the load-bearing surface for an agent. Its shape is fixed so an agent can pattern-match on it without prose parsing. Findings are grouped, so the shape spans two lines: a block header, `<severity>: <n> files over the <soft-limit>-<unit> budget [rule: <name>, message: <id>]`, and one indented line per file. For line rules the file line leads with `<path>: <actual> <counting basis> (budget <limit>)`, where the basis says which configured line categories count; byte and token rules lead with `<path>: <actual> <unit>`. Each file line then names the ceiling a plain `fissile exception add` would record wherever that call would be accepted, so the agent that cannot split the file reads the entry it would get without asking for one (§FS-004-check-audit.1). Guidance is printed once per block, never per file — the same reason the warning is worth having is the reason it must not cost a repetition of itself per file ([§GOAL-004-token-thrift](goals.md#goal-004-token-thrift-the-tool-itself-spends-as-few-tokens-as-it-saves)). The companion line in the managed `AGENTS.md` block (written by `fissile init`, parallel to grund's pattern) names this output and the expected response: *if you wrote this file in this turn, follow the configured guidance and try to bring it back to the soft limit; if you did not, leave it alone unless the task is about that file*.

This is the half of [§GND-001-fissile](grund.md#gnd-001-fissile-steer-agents-toward-leaner-files--fewer-tokens-architecture-intact)'s promise that pays off continuously: first by making "shrink the file you just grew in the way this repo expects" the path of least resistance, then by blocking only after the configured grace has been spent. The block stays on the soft-exception route, and incomplete history stays advisory rather than inventing proof (§DF-013-bounded-soft-grace).

### 3. What this rules out

- **A third size tier.** Two installs of the tool that share a config and Git history must agree on whether a file warns, fails, or passes. The soft/hard pair remains the size schema; edit promotion is bounded provenance on the soft finding, not another measurement threshold.
- **A `--strict` flag that turns warnings into errors.** That would let CI and local hook disagree, which is exactly the surprise [§GOAL-003-friendly-output](goals.md#goal-003-friendly-output-tell-the-user-exactly-what-broke-and-how-to-fix-it) refuses.
- **Unstructured warning text.** The finding is byte-stable and the configured message has a stable ID so agents can pattern-match on it ([§GOAL-004-token-thrift.1](goals.md#1-what-this-requires)).

### 4. Measurable

E2E fixtures cover all four size states per rule: clean, soft-only, hard-only, both. The hard-only case must exit non-zero; a pre-limit soft-only case exits zero, while the history fixture proves the configured-limit edit blocks, shrinking resets the run, exceptions silence it, and shallow history fails open. An integration test drives the real binary through the minimize loop — warn, shrink the named file, re-check clean — and asserts the byte-exact soft-finding line an agent keys off (`tests/agent_loop.rs`).

## GOAL-007-justified-exceptions: every oversized file has a written reason

The hard limit ([§GOAL-006-graded-limits](goals.md#goal-006-graded-limits-soft-warns-hard-blocks-ai-minimizes)) has no override flag. It does have one escape hatch — a structured hard exceptions registry where each oversized file is declared with a written rationale and a maximum accepted measurement. Soft warnings have a parallel soft exceptions registry for agent-facing debt that the repository has deliberately accepted. The registries are the paper trail: every accepted oversized file has, somewhere in the repo, a paragraph explaining why, filed under the path it accepts in the registry whose severity it waives ([§DF-005-exception-identity](decisions/functional/DF-005-exception-identity.md)).

This is the contract that lets [§GND-001-fissile](grund.md#gnd-001-fissile-steer-agents-toward-leaner-files--fewer-tokens-architecture-intact) hold under real-world adoption. A team turning the tool on for the first time will have files over the soft and hard limits on day one; the registries are how they accept the current state without disabling the guard, and how the next reviewer or agent can tell "this file is large for a reason" from "this file is large because nobody noticed."

### 1. What the registries are

Two TOML files at configured paths in the target repo hold one structured entry per exempted file. The soft registry defaults to `docs/file-size-agent-exceptions.toml`; the hard registry defaults to `docs/file-size-human-exceptions.toml`. Each entry looks like:

```toml
fissile_exceptions_version = 2

[[exceptions]]
path = "path/to/file.ts"
match = "exact"
rules = ["rule-id"]
kind = "deferred"
max_accepted = { value = 800, unit = "lines" }
until = "condition, date, or indefinite"
reason = """
One paragraph that establishes what the kind requires: the architectural
constraint that makes the split illegal, or the boundary that is missing and
what has to exist before the split is possible.
"""
```

An entry declares which of two claims it is making, because they are not the same claim and they do not expire the same way: `kind = "structural"` says the split is illegal and nothing retires the entry; `kind = "deferred"` says the boundary is simply missing and `until` names what retires it ([§DF-004-exception-kind](decisions/functional/DF-004-exception-kind.md)). Demanding one phrasing for both is what produces fabricated justifications for ordinary debt.

An entry has no name of its own. What identifies it is the registry it lives in
and what it accepts — path matcher, rules, unit — so a diagnostic names the
registry file and the entry's `path`, which is the line a reader has to edit
([§DF-005-exception-identity](decisions/functional/DF-005-exception-identity.md)).
The parsing contract lives in §FS-003-exceptions.

### 2. How the checker uses it

- On startup the checker loads both registries, parses each `[[exceptions]]` entry, extracts the path or glob from the structured fields, and builds a path → exception index per severity.
- A file matched by a soft-registry exception is silenced for soft findings at or below the entry's `max_accepted` value. A file matched by a hard-registry exception is silenced for hard findings at or below the entry's `max_accepted` value.
- A silenced hard finding leaves the soft warning to the accepting entry's kind (§FS-003-exceptions.3). A `deferred` entry keeps the warning, because that is debt someone is expected to retire. A `structural` entry silences it as well: the split is illegal, so the warning asks for work nobody may do, and the only way to quiet it before the kind existed was a second entry saying the same thing in the other registry.
- If the file grows past `max_accepted`, the finding appears again even though the path still has an exception entry.
- `fissile exception add` (§FS-005-exception-add) is the supported way to append
  entries, so users do not need to hand-edit registry TOML for current
  overflows.
- An exception whose path matches no file under scan is reported by `audit --stale-exceptions` — dead exceptions rot fast and the tool refuses to pretend they are load-bearing.
- `audit` names the accepted file and the ceiling that accepts it when an exception applies, so a reviewer looks the rationale up under the path already on the line, and counts the registries by kind so accepted-permanently and carrying-debt are two numbers rather than one total (§FS-004-check-audit.2).

### 3. What this rules out

- **An exception without a rationale.** The schema requires the prose paragraph; an entry with empty body is a parse error. Silent override is exactly what [§GOAL-003-friendly-output](goals.md#goal-003-friendly-output-tell-the-user-exactly-what-broke-and-how-to-fix-it) refuses.
- **An unstated question.** A reason is not free-form prose about the file. The entry's kind fixes what the paragraph must establish, and the shipped guidance says so rather than only demanding that *some* reason exist — "a written reason and a revisit trigger" reads as satisfied by any sentence, which is how a registry fills with descriptions of file contents ([§DF-004-exception-kind](decisions/functional/DF-004-exception-kind.md)).
- **A flag-based override.** No `--allow path/to/file`, no `# fissile: allow` magic comment. The registries are the only escape hatch, because they are the only form that survives review and shows up in history.
- **A name for the entry that is not what it accepts.** No `EX-NNN` id to allocate, keep unique by hand, and re-point when one entry becomes two. An entry is already identified by the registry it lives in and the condition it accepts, and that pair — unlike a slug — is the line the reader has to edit ([§DF-005-exception-identity](decisions/functional/DF-005-exception-identity.md)).
- **An exception that lives next to its file.** Centralizing the registries is what makes the inventory legible: one file lists soft agent debt and one file lists hard human debt. A scattered "exemption per file" cannot answer "show me everything we have given up on."

### 4. Composition with the other goals

- [§GOAL-006-graded-limits](goals.md#goal-006-graded-limits-soft-warns-hard-blocks-ai-minimizes) defines what the registries override; this goal defines the shape of the override.
- [§GOAL-005-configurable.1](goals.md#1-what-is-configurable) separates exclusions (no rationale, for files the tool obviously does not apply to) from exceptions (rationale required, for files the tool applies to but accepts). The split is deliberate — silencing a `.png` and silencing a 4,000-line module are not the same kind of decision and the registry shapes should not let them look the same.
- [§GOAL-004-token-thrift](goals.md#goal-004-token-thrift-the-tool-itself-spends-as-few-tokens-as-it-saves) holds: a file under exception emits no blocking diagnostic by default, while `audit` can still attribute accepted debt.

### 5. Measurable

E2E fixtures cover: `fissile exception add` appending soft and hard entries; a hard registry with a single exception silencing a hard violation; a soft registry with a single exception silencing a soft warning; a file that outgrows its exception's maximum accepted size and reports again; a registry whose exception names a path that no longer exists (audit must flag it stale); and a registry entry whose rationale is empty (parse error). Kind coverage adds: `exception add` refusing a `structural` entry with a dated `until` and a `deferred` entry with an `indefinite` one, an entry that omits `kind` still loading and counting as deferred, `audit` reporting the two counts, and the same fixture going quiet under a `structural` hard entry where a `deferred` one keeps warning (`tests/e2e/cases/E2E-019-structural-silences-soft`). A registry that has not been migrated to version 2 is refused with both edits named (`tests/e2e/cases/E2E-020-registry-version-2`). A snapshot test on the default registry paths and the parse rules guards against silent schema changes.

## GOAL-008-remediation-messages: overflows can carry local remediation guidance

The key promise of [§GND-001-fissile](grund.md#gnd-001-fissile-steer-agents-toward-leaner-files--fewer-tokens-architecture-intact) is not only that the library notices large files on every commit. It gives the repository a place to say what it already knows: which boundary the file is pressing against, where new code should move, and which local rule explains that move. The tool guarantees the slot and keeps it bounded; whether a given message names real architecture is the project's choice, and the built-in defaults stay deliberately generic.

### 1. What a message may carry

- **The matched rule.** The message is selected by the same rule that selected the budget, so `src/http/**` and `src/domain/**` can teach different split patterns when a project configures them to.
- **An architecture citation.** A message's text may cite a `§AR-`, `§FS-`, or `§GOAL-` ID so the reader can pull deeper context only when needed.
- **A destination hint.** A message may name a module, directory, owner, or interface boundary that should receive the extracted code.
- **A next step.** The text may say what to do next: split a helper, extract a fixture, move generated output, add an exception, or tighten the rule.

None of these are required. A rule may point at a generic message, and the defaults do exactly that rather than pretending to know a repository's layout.

### 2. Constraints

Messages are project-owned but not arbitrary scripts. They are static templates with bounded length and stable IDs, carrying any destination, owner, or next-step hint inline in the text. They cannot run code, inspect file contents beyond the matched rule, or change pass/fail behavior. The finding remains machine-readable; the message is whatever local guidance the project chose to layer on top.

### 3. Measurable

E2E fixtures cover rule-specific messages, a message that cites an architecture declaration, a generic message with no citation that still validates, and JSON output that includes both the message ID and rendered guidance.
