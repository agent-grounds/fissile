"""The repository's own changelog and workflows are ready to prepare a release. §AR-001-ci.8.3"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
import unittest

from history import ROOT, History, pr_line


RELEASE_RE = re.compile(r"^## (?P<n>[0-9]+)\. \[(?P<version>[0-9]+\.[0-9]+\.[0-9]+)\] — [0-9-]{10}$")
BUMP_WORKFLOWS = ["auto-bump.yml", "release-minor.yml"]


def top_level_headings(text: str) -> list[str]:
    return [line for line in text.splitlines() if line.startswith("## ")]


class TheRepositoryChangelog(unittest.TestCase):
    def test_prepare_writes_the_next_release_with_nothing_written_by_hand(self):
        # The ticket's own example: today this stops at the empty pending section.
        changelog = (ROOT / "docs" / "changelog.md").read_text(encoding="utf-8")
        inline = next(m for m in map(RELEASE_RE.match, top_level_headings(changelog)) if m)["version"]
        major, minor, patch = map(int, inline.split("."))
        following = f"{major}.{minor}.{patch + 1}"

        h = History(changelog)
        self.addCleanup(h.cleanup)
        shutil.rmtree(h.root / "docs" / "changelog")
        shutil.copytree(ROOT / "docs" / "changelog", h.root / "docs" / "changelog")
        h.commit("Take the repository's changelog", ["docs"])
        h.tag(f"v{inline}")
        sha = h.commit("Audit prints ok over a scan that selected no files", touch="src/lib.rs")
        h.merged(91, "Audit prints ok over a scan that selected no files", sha, "2026-10-04T10:00:00Z")

        result = h.prepare(following)

        self.assertEqual(result.returncode, 0, f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}")
        text = h.read("docs/changelog.md")
        self.assertEqual(
            top_level_headings(text),
            [f"## 1. [{following}] — 2026-10-05", "## 2. Older releases"],
        )
        self.assertIn(
            f"## 1. [{following}] — 2026-10-05\n\n"
            + pr_line(91, "Audit prints ok over a scan that selected no files")
            + "\n## 2. Older releases\n",
            text,
        )

    def test_the_changelog_has_no_pending_section_or_entry_conventions(self):
        # §AR-001-ci.8.3.5: the latest release is `## 1.` and the older ones `## 2.`.
        headings = top_level_headings((ROOT / "docs" / "changelog.md").read_text(encoding="utf-8"))
        self.assertNotIn("## Unreleased", headings)
        self.assertEqual(len(headings), 2, headings)
        latest = RELEASE_RE.match(headings[0])
        self.assertIsNotNone(latest, headings[0])
        self.assertEqual(latest["n"], "1")
        self.assertEqual(headings[1], "## 2. Older releases")

    def test_the_help_no_longer_describes_promoting_unreleased(self):
        script = ROOT / "scripts" / "prepare_changelog_release.py"
        result = subprocess.run([sys.executable, str(script), "--help"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("Unreleased", result.stdout, result.stdout)


class TheBumpWorkflows(unittest.TestCase):
    def workflow(self, name: str) -> str:
        return (ROOT / ".github" / "workflows" / name).read_text(encoding="utf-8")

    def test_they_declare_pull_request_read_permission(self):
        # §AR-001-ci.8.1: preparation reads pull requests.
        for name in BUMP_WORKFLOWS:
            with self.subTest(workflow=name):
                permissions = self.workflow(name).split("\npermissions:\n", 1)[1].split("\n\n", 1)[0]
                self.assertRegex(permissions, r"(?m)^  pull-requests: read\b")

    def test_the_prepare_step_gets_the_release_pat_as_gh_token(self):
        for name in BUMP_WORKFLOWS:
            with self.subTest(workflow=name):
                steps = re.split(r"(?m)^      - ", self.workflow(name))
                [step] = [s for s in steps if "prepare_changelog_release.py prepare" in s]
                self.assertRegex(step, r"(?m)^          GH_TOKEN: \$\{\{ secrets\.RELEASE_PAT \}\}\s*$")

    def test_the_current_version_is_the_highest_tag_main_can_reach(self):
        # §AR-001-ci.8.3.1: the workflows pick the previous release by the same rule.
        for name in BUMP_WORKFLOWS:
            with self.subTest(workflow=name):
                for lookup in re.findall(r"git tag --list[^\n]*", self.workflow(name)):
                    self.assertIn("--merged", lookup)

    def test_ci_runs_the_release_suite(self):
        self.assertIn("tests/release", self.workflow("ci.yml"))


if __name__ == "__main__":
    unittest.main()
