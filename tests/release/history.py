"""A throwaway git history and a fake `gh` to run release preparation against.

Shared by the release-notes tests (§AR-001-ci.8.3). Each `History` is a fresh
repository in a temporary directory (it honours `TMPDIR`), holding a changelog
whose inline release is `0.11.0`, and `prepare` runs the real
`scripts/prepare_changelog_release.py` CLI inside it with `fake_gh.py` as `gh`.
"""

from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "prepare_changelog_release.py"
FAKE_GH = Path(__file__).resolve().with_name("fake_gh.py")
REPO = "agent-grounds/fissile"

CHANGELOG = """\
# Changelog

Records every notable change to `fissile`. The latest release is inline in this
file; older releases live one per file under `docs/changelog/`.

## 1. [0.11.0] — 2026-10-03

Fixes the audit note.

### Fixed

- The audit note names the [scan root](architecture/AR-001-ci.md#8-release-workflow). (PR #85)

## 2. Older releases

- [0.10.1](changelog/0.10.1.md) — 2026-09-21: Staged failures describe the verdict.
<!-- Populated by `prepare_changelog_release.py prepare` when a release ships. -->
"""

ARCHIVE_0_10_1 = "# 0.10.1 — 2026-09-21\n\nStaged failures describe the verdict.\n"


def pr_line(number: int, title: str) -> str:
    return f"- [{title}](https://github.com/{REPO}/pull/{number}) (PR #{number})\n"


class History:
    def __init__(self, changelog: str = CHANGELOG) -> None:
        self.dir = Path(tempfile.mkdtemp(prefix="fissile-release-"))
        self.root = self.dir / "repo"
        self.root.mkdir()
        self.pulls: dict[str, dict] = {}
        self.commits: dict[str, list[list[int]]] = {}
        self.raw: dict[str, str] = {}
        self.fail: dict = {}
        self.env_overrides: dict[str, str | None] = {"GITHUB_REPOSITORY": REPO}
        self.git("init", "-q", "-b", "main")
        self.write("docs/changelog.md", changelog)
        self.write("docs/changelog/0.10.1.md", ARCHIVE_0_10_1)
        self.write("src/lib.rs", "pub fn measure() {}\n")
        self.base = self.commit("Initial import", ["docs", "src"])

    def cleanup(self) -> None:
        for path in self.dir.rglob("*"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(stat.S_IRWXU)
        shutil.rmtree(self.dir, ignore_errors=True)

    # Git history

    def git(self, *args: str, cwd: Path | None = None) -> str:
        result = subprocess.run(
            ["git", *args], cwd=cwd or self.root, env=_git_env(), text=True, capture_output=True
        )
        if result.returncode != 0:
            raise AssertionError(f"git {' '.join(args)} failed: {result.stderr}")
        return result.stdout.strip()

    def write(self, relative: str, text: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit(self, message: str, paths: list[str] | None = None, touch: str | None = None) -> str:
        if touch is not None:
            target = self.root / touch
            previous = target.read_text(encoding="utf-8") if target.exists() else ""
            self.write(touch, previous + f"{message}\n")
            paths = [touch]
        self.git("add", "--", *(paths or ["."]))
        self.git("commit", "-q", "--allow-empty", "-m", message)
        return self.git("rev-parse", "HEAD")

    def tag(self, name: str, rev: str = "HEAD") -> None:
        self.git("tag", name, rev)

    # The forge

    def merged(
        self,
        number: int,
        title: str,
        merge_sha: str,
        merged_at: str,
        commits: list[str] | None = None,
        base_ref: str = "main",
        base_repo: str = REPO,
    ) -> None:
        """Record a merged PR and associate it with `commits` (default: its merge commit)."""
        self.pulls[str(number)] = _pull(number, title, merge_sha, merged_at, base_ref, base_repo)
        for sha in commits if commits is not None else [merge_sha]:
            self.associate(sha, number)

    def opened(self, number: int, title: str, commits: list[str], merge_sha: str) -> None:
        pull = _pull(number, title, merge_sha, None, "main", REPO)
        pull["state"] = "open"
        self.pulls[str(number)] = pull
        for sha in commits:
            self.associate(sha, number)

    def associate(self, sha: str, number: int, page: int = 0) -> None:
        pages = self.commits.setdefault(sha, [[]])
        while len(pages) <= page:
            pages.append([])
        pages[page].append(number)

    # Running the script

    def prepare(self, version: str, date: str = "2026-10-05", cwd: Path | None = None):
        return self.run_script(["prepare", version, "--date", date], cwd)

    def notes(self, version: str, output: Path):
        return self.run_script(["notes", version, "--output", str(output)])

    def run_script(self, args: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess:
        bin_dir = self.dir / "bin"
        bin_dir.mkdir(exist_ok=True)
        gh = bin_dir / "gh"
        gh.write_text(f"#!/bin/sh\nexec {sys.executable} {FAKE_GH} \"$@\"\n", encoding="utf-8")
        gh.chmod(0o755)
        scenario = self.dir / "scenario.json"
        scenario.write_text(
            json.dumps(
                {
                    "repo": REPO,
                    "pulls": self.pulls,
                    "commits": self.commits,
                    "raw": self.raw,
                    "fail": self.fail,
                    "log": str(self.dir / "gh.log"),
                }
            ),
            encoding="utf-8",
        )
        env = _git_env()
        env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
        env["FAKE_GH_SCENARIO"] = str(scenario)
        env["GH_TOKEN"] = "fake-token"
        for key, value in self.env_overrides.items():
            if value is None:
                env.pop(key, None)
            else:
                env[key] = value
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            cwd=cwd or self.root,
            env=env,
            text=True,
            capture_output=True,
        )

    # Reading the result

    def read(self, relative: str, cwd: Path | None = None) -> str:
        return ((cwd or self.root) / relative).read_text(encoding="utf-8")

    def snapshot(self, cwd: Path | None = None) -> dict[str, bytes]:
        docs = (cwd or self.root) / "docs"
        return {str(p.relative_to(docs)): p.read_bytes() for p in sorted(docs.rglob("*")) if p.is_file()}


class HistoryCase(unittest.TestCase):
    """A test case with a fresh `History` in `self.h`, tagged `v0.11.0` at a release commit."""

    def setUp(self) -> None:
        self.h = History()
        self.addCleanup(self.h.cleanup)
        self.pre_tag = self.h.commit("Fix the audit note", touch="src/lib.rs")
        self.h.merged(85, "Fix the audit note", self.pre_tag, "2026-10-02T09:00:00Z")
        self.release = self.h.commit("Release v0.11.0", touch="Cargo.toml")
        self.h.tag("v0.11.0")

    def assert_ok(self, result: subprocess.CompletedProcess) -> None:
        self.assertEqual(
            result.returncode, 0, f"prepare failed\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )

    def assert_refused(self, result: subprocess.CompletedProcess, before: dict, pattern: str) -> None:
        detail = f"\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        self.assertEqual(result.returncode, 1, f"expected exit 1{detail}")
        self.assertNotIn("Traceback", result.stderr, detail)
        self.assertRegex(result.stderr, pattern, f"the error does not name the case{detail}")
        self.assertEqual(self.h.snapshot(), before, f"docs/ changed on a refusal{detail}")

    def inline_section(self, text: str | None = None) -> list[str]:
        """The lines between the first `## 1.` heading and the next top-level heading."""
        lines = (text if text is not None else self.h.read("docs/changelog.md")).splitlines(keepends=True)
        start = next(i for i, line in enumerate(lines) if line.startswith("## 1. ["))
        end = next(i for i in range(start + 1, len(lines)) if lines[i].startswith("## "))
        return [line for line in lines[start + 1 : end] if line.strip()]


def _pull(number, title, merge_sha, merged_at, base_ref, base_repo) -> dict:
    return {
        "number": number,
        "title": title,
        "html_url": f"https://github.com/{REPO}/pull/{number}",
        "state": "closed",
        "merged_at": merged_at,
        "merge_commit_sha": merge_sha,
        "base": {"ref": base_ref, "repo": {"full_name": base_repo}},
        "head": {"ref": f"feature-{number}", "repo": {"full_name": REPO}},
    }


def _git_env() -> dict[str, str]:
    env = dict(os.environ)
    env.update(
        {
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_AUTHOR_NAME": "Release Test",
            "GIT_AUTHOR_EMAIL": "release-test@example.invalid",
            "GIT_COMMITTER_NAME": "Release Test",
            "GIT_COMMITTER_EMAIL": "release-test@example.invalid",
        }
    )
    return env
