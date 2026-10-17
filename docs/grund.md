# GND-001-fissile: Steer agents toward leaner files — fewer tokens, architecture intact.

Large files are an invisible tax on every AI-assisted workflow in a repo. A 4,000-line module
or a multi-megabyte fixture gets dragged into context whenever an agent (or a reviewer) needs
to understand a single function inside it, and that context is paid for in tokens, latency,
and attention budget. The cost is paid by every contributor, every run, forever — but it is
not visible in any one diff, so it accretes silently until a codebase becomes painful to work
with at machine speed.

## 1. The problem

Files grow. Reviewers see deltas, not totals, so a file that crossed a sensible size threshold
years ago keeps gaining lines one PR at a time. There is no natural feedback loop that pushes
back on bloat at the moment it is introduced — only after the fact, when someone notices that
loading a single source file consumes a meaningful slice of a model's context window or that
a fixture is being shipped to every clone of the repo.

Raw size feedback is not enough. A contributor or agent who sees only "this file is too big"
still has to rediscover where the code belongs, which boundary it crossed, and what the local
team considers a good split. The useful feedback loop is architectural: at commit time, the
overflow message should explain the violated budget in terms of the repository's own modules,
ownership, and extraction patterns.

The same problem applies unevenly across file types: a 2,000-line generated SQL dump is fine,
a 2,000-line hand-written TypeScript module is not, and a checked-in PNG of any size is
usually a mistake. A useful guard has to know the difference.

## 2. What this project does about it

`fissile` does one simple thing. At the moment new content is introduced, it flags files that
have outgrown a per-repo, per-file-type size budget, so that agents and reviewers spend fewer
tokens whenever one of those files is pulled into context. It only measures and reports; it
never rewrites code, so the repository's architecture is left untouched — how to split a
flagged file is always the contributor's decision. The name carries the
contract: a file over budget is *fissile* because it is ready to split under its own
accumulated mass (§DF-001-tool-name). Each overflow can also carry a short, project-configured
message suggesting how this repository prefers such a file to be split, but that guidance is
help layered on top, not the point (§GOAL-008-remediation-messages). The CLI runs in
three modes:

- as a **pre-commit hook** that checks staged files and refuses the commit when a hard budget
  is exceeded, with a configured message that explains the local architectural remedy;
- as a **branch check** that judges the files a branch changed against its base,
  as they stand at its head, with the grace the hook spends, so a rebase or a
  pull-request job gets the hook's verdict without staging anything;
- as an **audit subcommand** that scans the whole repo and reports current offenders, so a
  team can adopt the tool against an existing codebase without immediately blocking work.

Budgets are configurable per extension, per glob, and per unit (bytes, lines, or — when the
goal is literally to bound token cost — tokens). Overflow messages are configurable per rule
and can cite local architecture docs, owners, destination modules, or extraction patterns.
Defaults are conservative; exclusions for lockfiles, generated artifacts, and binary assets
are first-class so the tool stays useful without an escape-hatch culture.

## 3. Who it is for

- **Maintainers of agent-friendly codebases** who want a mechanical guard against
  context-window bloat rather than relying on review discipline.
- **Teams adopting AI-assisted development** who have noticed that token spend correlates
  with a handful of overgrown files and want a cheap, local, language-agnostic fix.
- **Solo developers** who want a one-binary pre-commit check they can drop into any repo
  without standing up a Python/Node toolchain first.

## 4. Positioning

`fissile` sits next to large-file hooks, linter line-count rules, bundle-size checks, and
PR-size gates, but it is not the same tool.

Large-file hooks protect the repository from accidentally committed blobs. Linter line-count
rules protect one language's local style. Bundle-size checks protect shipped artifacts. PR-size
gates protect reviewer throughput. `fissile` protects the **source layout itself**: it notices
when a file is becoming too large to be a cheap unit of understanding, and it tells the next
human or agent how this repository wants that file split.

The difference is the message. A generic "file too large" check stops a bad commit; `fissile`
also names the local architectural move — destination module, ownership boundary, extraction
pattern, or cited rationale — that makes the next commit better (§GOAL-008-remediation-messages).
The result is a file budget system with memory: defaults catch obvious bloat, custom rules encode
local architecture (§FS-001-config), and justified exceptions stay reviewable instead of becoming
inline ignore comments (§FS-003-exceptions).
