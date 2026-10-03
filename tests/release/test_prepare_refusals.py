"""A refused preparation exits 1, names the case, and writes nothing. §AR-001-ci.8.3.6"""

from __future__ import annotations

import os
import subprocess
import unittest

from history import HistoryCase


class RefusalsLeaveTheChangelogAlone(HistoryCase):
    def setUp(self) -> None:
        super().setUp()
        self.code = self.h.commit("Generate release notes", touch="src/lib.rs")
        self.h.merged(89, "Generate release notes", self.code, "2026-10-04T10:00:00Z")

    def refuse(self, pattern: str, version: str = "0.11.1", date: str = "2026-10-05") -> None:
        before = self.h.snapshot()
        self.assert_refused(self.h.prepare(version, date), before, pattern)

    def test_no_reachable_tag(self):
        self.h.git("tag", "-d", "v0.11.0")
        self.refuse(r"(?i)tag")

    def test_a_tag_that_differs_from_the_inline_release(self):
        self.h.tag("v0.11.1", self.release)
        self.refuse(r"0\.11\.1")

    def test_a_shallow_clone(self):
        clone = self.h.dir / "shallow"
        subprocess.run(
            ["git", "clone", "-q", "--depth", "2", f"file://{self.h.root}", str(clone)],
            check=True,
            capture_output=True,
        )
        self.assertTrue((clone / ".git" / "shallow").exists())
        before = self.h.snapshot(clone)
        result = self.h.prepare("0.11.1", cwd=clone)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertRegex(result.stderr, r"(?i)shallow")
        self.assertEqual(self.h.snapshot(clone), before)

    def test_no_merged_pull_request_in_the_range(self):
        self.h.pulls.clear()
        self.h.commits.clear()
        self.h.commit("Open 0.11.1-dev for development", touch="Cargo.toml")
        self.refuse(r"(?i)pull request")

    def test_an_empty_range(self):
        self.h.git("reset", "-q", "--hard", self.release)
        self.refuse(r"(?i)pull request")

    def test_an_invalid_version(self):
        self.refuse(r"(?i)version", version="0.11")

    def test_an_invalid_date(self):
        self.refuse(r"(?i)date", date="2026-13-01")

    def test_the_version_is_already_the_inline_release(self):
        self.refuse(r"0\.11\.0", version="0.11.0")

    def test_the_archive_already_exists(self):
        self.h.write("docs/changelog/0.11.0.md", "# 0.11.0 — 2026-10-03\n\nalready archived\n")
        self.refuse(r"(?i)archive")

    def test_an_authentication_failure(self):
        self.h.fail = {"all": "gh: Bad credentials (HTTP 401)"}
        self.refuse(r"(?i)401|credential|auth")

    def test_a_page_that_could_not_be_read(self):
        self.h.merged(98, "Listed on the second page", self.code, "2026-10-04T11:00:00Z", commits=[])
        self.h.associate(self.code, 98, page=1)
        self.h.fail = {"sha": self.code, "page": 2, "message": "gh: Bad Gateway (HTTP 502)"}
        self.refuse(r"(?i)502|page|api")

    def test_malformed_json_from_github(self):
        self.h.raw[self.code] = '[[{"number": 89, "title": '
        self.refuse(r"(?i)json|malformed|invalid|unexpected")

    def test_a_pull_request_without_its_merge_commit(self):
        del self.h.pulls["89"]["merge_commit_sha"]
        self.refuse(r"(?i)merge_commit_sha|malformed")


@unittest.skipIf(hasattr(os, "geteuid") and os.geteuid() == 0, "root ignores file modes")
class AFailedWriteLeavesNothingBehind(HistoryCase):
    def test_the_archive_is_rolled_back_when_the_changelog_cannot_be_replaced(self):
        code = self.h.commit("Generate release notes", touch="src/lib.rs")
        self.h.merged(89, "Generate release notes", code, "2026-10-04T10:00:00Z")
        docs = self.h.root / "docs"
        before = self.h.snapshot()
        (docs / "changelog.md").chmod(0o444)
        docs.chmod(0o555)
        try:
            result = self.h.prepare("0.11.1")
        finally:
            docs.chmod(0o755)
            (docs / "changelog.md").chmod(0o644)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(self.h.snapshot(), before, result.stderr)



if __name__ == "__main__":
    unittest.main()
