#!/usr/bin/env python3
"""A stand-in for the `gh` CLI that answers from a scenario file, never from GitHub.

The release-notes tests (§AR-001-ci.8.3) put this on PATH as `gh`. It serves the
two REST reads preparation makes - the commit-to-pull-request endpoint and a
pull request's details - from the JSON file `FAKE_GH_SCENARIO` names:

    {
      "repo": "agent-grounds/fissile",
      "pulls": {"89": {<pull request object>}},
      "commits": {"<sha>": [[89], [98]]},    # pages of PR numbers
      "raw": {"<sha>": "<body served verbatim>"},
      "fail": {"all": "<message>"} | {"sha": "<sha>", "page": 2, "message": "..."},
      "log": "<file every call is appended to>"
    }

`api` accepts `--paginate`, `--slurp`, `-H`/`--header`, and `-X`/`--method GET`.
`--paginate --slurp` prints one JSON array of pages, `--paginate` alone prints
the pages back to back, and neither prints the first page only, as `gh` does.
Anything else exits 2 naming the argument, so an unsupported use fails loudly.
"""

from __future__ import annotations

import json
import os
import re
import sys


def main(argv: list[str]) -> int:
    scenario = _load()
    log = scenario.get("log")
    if log:
        with open(log, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(argv) + "\n")

    if argv[:1] == ["auth"]:
        if argv[1:2] == ["token"]:
            print("fake-token")
        return 0
    if argv[:1] != ["api"]:
        return _unsupported(argv[:1])

    path, paginate, slurp = _parse_api(argv[1:])
    if path is None:
        return 2

    failure = scenario.get("fail") or {}
    if "all" in failure:
        print(failure["all"], file=sys.stderr)
        return 1

    repo = scenario["repo"]
    path = path.lstrip("/").split("?", 1)[0]
    path = path.replace("{owner}/{repo}", repo)

    commit = re.fullmatch(r"repos/([^/]+/[^/]+)/commits/([0-9a-f]+)/pulls", path)
    if commit and commit.group(1) == repo:
        sha = commit.group(2)
        if sha in scenario.get("raw", {}):
            sys.stdout.write(scenario["raw"][sha])
            return 0
        numbers = scenario["commits"].get(sha, [[]])
        if not paginate:
            numbers = numbers[:1]
        if failure.get("sha") == sha and failure.get("page", 1) <= len(numbers):
            print(failure.get("message", "HTTP 502: Bad Gateway"), file=sys.stderr)
            return 1
        pages = [[_pull(scenario, n) for n in page] for page in numbers]
        if slurp:
            print(json.dumps(pages))
        else:
            for page in pages:
                print(json.dumps(page))
        return 0

    pull = re.fullmatch(r"repos/([^/]+/[^/]+)/pulls/([0-9]+)", path)
    if pull and pull.group(1) == repo and pull.group(2) in scenario["pulls"]:
        print(json.dumps(_pull(scenario, int(pull.group(2)))))
        return 0

    print(f"gh: Not Found (HTTP 404): {path}", file=sys.stderr)
    return 1


def _load() -> dict:
    with open(os.environ["FAKE_GH_SCENARIO"], encoding="utf-8") as handle:
        return json.load(handle)


def _pull(scenario: dict, number: int) -> dict:
    return scenario["pulls"][str(number)]


def _parse_api(args: list[str]) -> tuple[str | None, bool, bool]:
    path = None
    paginate = slurp = False
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--paginate":
            paginate = True
        elif arg == "--slurp":
            slurp = True
        elif arg in ("-H", "--header"):
            index += 1
        elif arg in ("-X", "--method"):
            index += 1
            if index >= len(args) or args[index] != "GET":
                _unsupported(args[index : index + 1] or [arg])
                return None, False, False
        elif arg.startswith("-") or path is not None:
            _unsupported([arg])
            return None, False, False
        else:
            path = arg
        index += 1
    if path is None:
        print("fake gh: api needs a path", file=sys.stderr)
    return path, paginate, slurp


def _unsupported(args: list[str]) -> int:
    print(f"fake gh: unsupported argument {args!r}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
