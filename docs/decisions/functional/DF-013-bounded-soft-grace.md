# DF-013-bounded-soft-grace: Repeated soft debt blocks only when history proves the grace is spent

A soft warning is useful on the edit that first crosses a budget: it gives an
agent room to find a responsibility seam without turning a provisional shape
into a broken commit. The same warning becomes permission to defer forever when
it remains advisory after every later edit. Making all soft overflows block
would remove the grace; leaving all of them advisory would preserve the drift.

## 1. Decision

Each soft rule has a configurable edit limit, defaulting to five. Only `check
--staged` and `check --base <ref>` spend it. The staged edit and each committed
edit in the continuous run since the file crossed the soft limit count; a
committed version at or below soft resets the run. Earlier edits warn, and the
edit at the limit promotes the soft finding into a commit block
(§FS-004-check-audit.1.4).

Caller-selected paths, plain no-path checks, audit, and the library checker do
not inherit promotion. They are potentially divergent, Git-free snapshots:
they may read working-tree bytes that differ from the index, and a path-list
invocation does not prove that its caller is a commit hook. Equal bytes in one
run do not supply either missing fact. Keeping those surfaces snapshot-only
preserves their exit status and cost while reserving grace accounting for the
invocations that select from Git and prove their history.

`check --base <ref>` answers both halves of that reason. It reads only Git
objects — the files changed between `<ref>` and `HEAD`, as they stand at `HEAD` —
so it cannot diverge from what was committed; and it is an explicit opt-in, so
no caller that chose snapshot semantics gains a blocking exit. It counts the
committed edits once per commit, with no staged edit, and the base bounds what
is judged without resetting the run (§FS-004-check-audit.1.4, §FS-004-check-audit.1.5).

Promotion changes consequence, not ownership. The finding remains soft debt,
uses the soft message, and is silenced by the soft registry. A true hard-size
overflow still wins and uses the hard route. This lets the repository make the
same decision before or after grace expires: split along a real seam, or record
why it cannot happen now.

The block requires proof of the full continuous run. A shallow repository,
missing Git history, or a rename that cannot be followed may provide a lower
bound, but not the reset boundary that gives the count its meaning. Fissile
reports only the edits it can establish and stays advisory. False negatives can
be repaired when history is available; a false commit block is exactly the
kind of gate surprise §GOAL-003-friendly-output forbids.

## 2. Why five

Five gives the first edit room to cross while bounding repeated deferral to four
further edits. It is long enough that one refactor need not be forced into the
crossing commit, and short enough that a repeatedly active oversized file cannot
quietly accumulate indefinitely. Making the value per rule lets a repository
price generated, test, source, and document debt differently without inventing
a repository-wide severity switch (§FS-001-config.3).

## 3. Rejected alternatives

**Block every soft overflow.** That collapses the two tiers in practice and
turns an early architectural signal into the hard ceiling under another name.

**Never promote.** This is the reported failure: “next time” remains a third
response beside splitting and recording the debt, with no event that retires
it.

**Promote from incomplete history.** A visible suffix can show that a file has
been edited often, but cannot show whether an unseen at-or-below-soft version
reset the run. Blocking from that suffix would manufacture the premise of the
decision.

**Promote caller-selected snapshots.** Explicit paths can name a different
working-tree snapshot from the index and can run without Git. Walking history
would not make that selection equivalent to `--staged`, and would silently add
blocking exits and repository work to callers that chose snapshot semantics.
The base mode is not this alternative: it selects from Git objects and is
requested by its own flag.

**Count a branch as one squashed edit.** Replaying `<ref>..HEAD` as one staged
diff is one edit, but the hook that ran on each of its commits saw one edit per
commit, so the squashed count passes files the per-commit hook blocked.

**Replay every hook on the branch.** Judging each intermediate commit would block
a branch for a violation it repaired before `HEAD`. The base mode gives the
endpoint verdict instead: the net changes at `HEAD`, counted per commit.

**Use the hard exception route after promotion.** Edit age does not turn the
file into a hard-size overflow. Requiring human-reviewed hard debt would make
the escape hatch depend on elapsed edits rather than on the size tier the rule
declared.
