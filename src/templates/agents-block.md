<!-- BEGIN FISSILE MANAGED BLOCK -->
## Keeping Files Small With fissile (v3)

This repository caps file size with [`fissile`](https://github.com/agent-grounds/fissile)
so that agents spend fewer tokens reading. Run `fissile check --staged` before
claiming work is done; its findings say what to split and how. Where the
pre-commit hook is installed it runs that same check — never get past it with
`--no-verify`.
<!-- END FISSILE MANAGED BLOCK -->
