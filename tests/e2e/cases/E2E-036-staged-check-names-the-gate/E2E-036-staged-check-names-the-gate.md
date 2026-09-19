# E2E-036-staged-check-names-the-gate: a rejected staged snapshot names the remedy

For a standing hard overflow, `check --staged` closes by saying that it rejected
the staged snapshot and offers a split or a reviewed hard exception. If this
command is the commit hook, bypassing it with `--no-verify` leaves the overflow
for review or CI (§FS-004-check-audit.1.2).

Only `--staged` prints this command-specific epilogue. A plain check or an
explicit-path check keeps its own verdict without this epilogue, which is why
`E2E-002-check-hard-blocks` sees the findings and the hint alone.
