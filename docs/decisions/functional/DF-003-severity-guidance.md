# DF-003-severity-guidance: Soft and hard overflows carry different instructions, and neither cites another repository's docs.

A soft overflow and a hard overflow are not the same request at two volumes.
Soft says *split now or record the debt now*; hard says *must split before more
code lands*. Each needs its own next step, and each needs its own way out
for the reader who cannot split the file without damaging it. `fissile` gives a
rule one message per severity, and ships defaults that name the escape hatch
instead of demanding a split unconditionally (§GOAL-008-remediation-messages).

## 1. Decision

A rule resolves guidance per severity: `soft_message` and `hard_message`
override the shared `message` for their own severity (§FS-001-config.4). The
built-in defaults use that slot for three things beyond "split this file".

- **The soft message bounds the split.** It asks for a split or a recorded-debt
  decision in the current commit, and says what not to do: never break up code that belongs
  together, never add indirection to fit a line count. A file whose only
  available split would make the architecture worse is not a failure to fix —
  it is debt to record with `fissile exception add --severity soft`.
- **The hard message escalates to a human.** A blocking finding cannot be
  answered with "I could not think of a split", so the message says what to do
  instead: stop and ask a human. The human-reviewed
  `fissile exception add --severity hard` is the only other way past the gate,
  and it is deliberately not something an agent grants itself
  (§FS-003-exceptions).
- **Neither message carries a `§` citation.** Default message text is copied
  into other repositories by `fissile init` (§FS-002-init). A citation to
  fissile's own goals or specs resolves nowhere in the repository that installed
  it, and describes fissile's architecture rather than the reader's. The
  generated config carries a comment marking where a project adds a citation
  into its *own* docs; the shipped text stays generic, as
  §GOAL-008-remediation-messages already requires of defaults.

## 2. Rejected alternatives

- **One message per rule, worded for both severities.** The wording has to be
  vague enough to fit a warning and a block, which is exactly where "split this
  file" comes from — a sentence with no next step for either reader.
- **Severity-aware phrasing via `{severity}`.** The variable can only vary a
  word, not the instruction, the escape hatch, or who to escalate to.
- **A structured `escalation` or `owner` field.** Rejected for the reason
  §FS-001-config.4 already gives: the guidance stays one rendered string, not a
  record the caller reassembles.

## 3. Consequences

Findings distinguish `split-source-soft` from `split-source-hard` in text and in
JSON `message_id`, so a repository can measure which half of the graded model
its agents actually act on. Rules that want one voice for both severities still
write a single `message` and change nothing.
