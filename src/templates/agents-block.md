<!-- BEGIN FISSILE MANAGED BLOCK -->
## Keeping Files Small With fissile (v4)

This repository caps file size with [`fissile`](https://github.com/agent-grounds/fissile)
so that agents spend fewer tokens reading. Run `fissile check --staged` before
claiming work is done; its findings say what to split and how. The pre-commit
hook `fissile init` installs runs that command, while a hook that passes file
paths keeps soft findings advisory — never get past it with `--no-verify`.
<!-- END FISSILE MANAGED BLOCK -->
