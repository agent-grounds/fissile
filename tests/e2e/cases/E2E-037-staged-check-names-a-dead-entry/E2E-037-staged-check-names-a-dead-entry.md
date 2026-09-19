# E2E-037-staged-check-names-a-dead-entry: a rejected staged snapshot names the dead entry

Under `[exceptions].stale = "error"` a leftover exception fails the run
(§FS-003-exceptions.4). Under `--staged`, the epilogue says the command rejected
the staged snapshot and directs the caller to remove the exception entry or
point it at the file's new path. If this command is the commit hook, bypassing
it with `--no-verify` leaves a dead registry entry (§FS-004-check-audit.1.2).

The case also pins what makes the entry dead. The commit stages the removal of
`src/moved.rs`, which is the file set proving the entry has outlived its file
(§FS-004-check-audit.1.3) — not the mere absence of a path from the working
tree, which `E2E-038-an-unbuilt-file-is-not-a-dead-entry` holds separately.
