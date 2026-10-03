"""`prepare` writes the release notes from the merged pull requests. §AR-001-ci.8.3"""

from __future__ import annotations

import re
import unittest

from history import ARCHIVE_0_10_1, CHANGELOG, History, HistoryCase, pr_line


class PrepareWritesTheMergedPullRequests(HistoryCase):
    def test_prepare_lists_merged_prs_archives_the_previous_release_and_round_trips_notes(self):
        # §AR-001-ci.8.3: nothing is written by hand; a docs-only PR is listed too.
        code = self.h.commit("Generate release notes from merged pull requests", touch="src/lib.rs")
        self.h.merged(89, "Generate release notes from merged pull requests", code, "2026-10-04T10:00:00Z")
        self.h.commit("Open 0.11.1-dev for development", touch="Cargo.toml")
        docs = self.h.commit("Fix the README install line", touch="README.md")
        self.h.merged(90, "Fix the README install line", docs, "2026-10-04T12:00:00Z")

        self.assert_ok(self.h.prepare("0.11.1"))

        guide = CHANGELOG.split("## 1.", 1)[0]
        self.assertEqual(
            self.h.read("docs/changelog.md"),
            guide
            + "## 1. [0.11.1] — 2026-10-05\n\n"
            + pr_line(90, "Fix the README install line")
            + pr_line(89, "Generate release notes from merged pull requests")
            + "\n## 2. Older releases\n\n"
            + "- [0.11.0](changelog/0.11.0.md) — 2026-10-03: Fixes the audit note.\n"
            + "- [0.10.1](changelog/0.10.1.md) — 2026-09-21: Staged failures describe the verdict.\n"
            + "<!-- Populated by `prepare_changelog_release.py prepare` when a release ships. -->\n",
        )
        self.assertEqual(
            self.h.read("docs/changelog/0.11.0.md"),
            "# 0.11.0 — 2026-10-03\n\nFixes the audit note.\n\n### Fixed\n\n"
            "- The audit note names the [scan root](../architecture/AR-001-ci.md#8-release-workflow)."
            " (PR #85)\n\n",
        )
        self.assertEqual(self.h.read("docs/changelog/0.10.1.md"), ARCHIVE_0_10_1)

        output = self.h.dir / "release-notes.md"
        self.assert_ok(self.h.notes("0.11.1", output))
        self.assertEqual(
            output.read_text(encoding="utf-8"),
            pr_line(90, "Fix the README install line")
            + pr_line(89, "Generate release notes from merged pull requests")
            + "\n",
        )

    def test_a_generated_release_is_archived_with_its_first_title_as_summary(self):
        first = self.h.commit("Fix the README install line", touch="README.md")
        self.h.merged(90, "Fix the README install line", first, "2026-10-04T12:00:00Z")
        self.assert_ok(self.h.prepare("0.11.1"))
        archived_0_11_0 = self.h.read("docs/changelog/0.11.0.md")
        self.h.commit("Release v0.11.1", ["docs"])
        self.h.tag("v0.11.1")
        second = self.h.commit("Measure symlinks once", touch="src/lib.rs")
        self.h.merged(91, "Measure symlinks once", second, "2026-10-06T08:00:00Z")

        self.assert_ok(self.h.prepare("0.11.2", date="2026-10-07"))

        self.assertEqual(self.inline_section(), [pr_line(91, "Measure symlinks once")])
        self.assertIn(
            "\n## 2. Older releases\n\n"
            "- [0.11.1](changelog/0.11.1.md) — 2026-10-05: Fix the README install line\n"
            "- [0.11.0](changelog/0.11.0.md) — 2026-10-03: Fixes the audit note.\n",
            self.h.read("docs/changelog.md"),
        )
        self.assertEqual(
            self.h.read("docs/changelog/0.11.1.md"),
            "# 0.11.1 — 2026-10-05\n\n" + pr_line(90, "Fix the README install line") + "\n",
        )
        self.assertEqual(self.h.read("docs/changelog/0.11.0.md"), archived_0_11_0)
        self.assertEqual(self.h.read("docs/changelog/0.10.1.md"), ARCHIVE_0_10_1)

    def test_docs_only_and_ci_only_prs_are_listed(self):
        # §AR-001-ci.8.3.3: no filter; the auto-bump gate is a separate question.
        ci = self.h.commit("Pin grund in CI", touch=".github/workflows/ci.yml")
        self.h.merged(103, "Pin grund in CI", ci, "2026-10-04T09:00:00Z")
        docs = self.h.commit("Reword the measure spec", touch="docs/functional-spec/measure.md")
        self.h.merged(104, "Reword the measure spec", docs, "2026-10-04T10:00:00Z")
        meta = self.h.commit("Update the licence year", touch="LICENSE")
        self.h.merged(105, "Update the licence year", meta, "2026-10-04T11:00:00Z")

        self.assert_ok(self.h.prepare("0.11.1"))

        self.assertEqual(
            self.inline_section(),
            [
                pr_line(105, "Update the licence year"),
                pr_line(104, "Reword the measure spec"),
                pr_line(103, "Pin grund in CI"),
            ],
        )


class TheRangeIsTagToFrozenTip(HistoryCase):
    def test_only_merged_prs_into_main_with_a_merge_commit_in_range_are_listed(self):
        # §AR-001-ci.8.3.2: merged, base main of this repository, merge_commit_sha in range.
        first = self.h.commit("Split the measure module", touch="src/lib.rs")
        second = self.h.commit("Cite the split", touch="src/lib.rs")
        self.h.merged(92, "Split the measure module", second, "2026-10-04T10:00:00Z", [first, second])
        squash = self.h.commit("Name the root as a directory", touch="src/lib.rs")
        self.h.merged(93, "Name the root as a directory", squash, "2026-10-04T11:00:00Z")
        tip = self.h.git("rev-parse", "HEAD")
        self.h.git("checkout", "-q", "-b", "later")
        after_tip = self.h.commit("Merged after the frozen tip", touch="src/lib.rs")
        self.h.git("checkout", "-q", "main")
        self.assertEqual(self.h.git("rev-parse", "HEAD"), tip)

        self.h.merged(94, "Merged after the frozen tip", after_tip, "2026-10-05T10:00:00Z", [first])
        self.h.opened(95, "Still open", [first], merge_sha=second)
        self.h.merged(96, "Into a release branch", second, "2026-10-04T12:00:00Z", [first], "release-0.11")
        self.h.merged(97, "Into a fork", second, "2026-10-04T12:30:00Z", [first], base_repo="someone/fork")
        self.h.associate(squash, 85)  # merged before the tag

        self.assert_ok(self.h.prepare("0.11.1"))

        self.assertEqual(
            self.inline_section(),
            [pr_line(93, "Name the root as a directory"), pr_line(92, "Split the measure module")],
        )

    def test_the_previous_release_is_the_highest_reachable_exact_tag(self):
        changelog = CHANGELOG.replace("[0.11.0] — 2026-10-03", "[0.10.0] — 2026-09-14")
        h = History(changelog)
        self.addCleanup(h.cleanup)
        h.tag("v0.9.0")
        between = h.commit("Between 0.9.0 and 0.10.0", touch="src/lib.rs")
        h.merged(60, "Between 0.9.0 and 0.10.0", between, "2026-09-10T10:00:00Z")
        h.tag("v0.10.0")
        h.git("checkout", "-q", "-b", "side")
        h.commit("Unreachable release", touch="side.txt")
        h.tag("v0.12.0")
        h.git("checkout", "-q", "main")
        candidate = h.commit("Release candidate work", touch="src/lib.rs")
        h.merged(70, "Release candidate work", candidate, "2026-09-15T10:00:00Z")
        h.tag("v0.10.1-rc1")
        h.tag("0.11.0")
        last = h.commit("After the candidate tag", touch="src/lib.rs")
        h.merged(71, "After the candidate tag", last, "2026-09-16T10:00:00Z")

        result = h.prepare("0.10.1")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            self.inline_section(h.read("docs/changelog.md")),
            [pr_line(71, "After the candidate tag"), pr_line(70, "Release candidate work")],
        )


class AcquisitionAndOrder(HistoryCase):
    def test_every_page_of_the_commit_to_pr_endpoint_is_read_and_numbers_deduplicated(self):
        first = self.h.commit("Read every page", touch="src/lib.rs")
        second = self.h.commit("Direct push", touch="Cargo.toml")
        self.h.merged(89, "Read every page", first, "2026-10-04T10:00:00Z")
        self.h.merged(98, "Listed on the second page", second, "2026-10-04T11:00:00Z", commits=[])
        self.h.associate(first, 98, page=1)
        self.h.associate(first, 89, page=1)

        self.assert_ok(self.h.prepare("0.11.1"))

        self.assertEqual(
            self.inline_section(),
            [pr_line(98, "Listed on the second page"), pr_line(89, "Read every page")],
        )

    def test_order_is_merged_at_newest_first_then_number_highest_first(self):
        a = self.h.commit("A", touch="src/a.rs")
        b = self.h.commit("B", touch="src/b.rs")
        c = self.h.commit("C", touch="src/c.rs")
        self.h.merged(101, "Merged first", a, "2026-10-04T09:00:00Z")
        self.h.merged(100, "Merged last", b, "2026-10-04T15:00:00Z")
        self.h.merged(102, "Merged with 101", c, "2026-10-04T09:00:00Z")

        self.assert_ok(self.h.prepare("0.11.1"))

        self.assertEqual(
            self.inline_section(),
            [pr_line(100, "Merged last"), pr_line(102, "Merged with 101"), pr_line(101, "Merged first")],
        )

    def test_markdown_in_a_title_is_escaped_to_show_literally(self):
        # §AR-001-ci.8.3.4: backslash escapes, so the line still links and reads as the title.
        title = r"Escape *stars*, _under_, `ticks`, [brackets](x) and <b> and a \ backslash"
        sha = self.h.commit("Escaping", touch="src/lib.rs")
        self.h.merged(106, title, sha, "2026-10-04T10:00:00Z")

        self.assert_ok(self.h.prepare("0.11.1"))

        [line] = self.inline_section()
        match = re.fullmatch(
            r"- \[(?P<text>(?:\\.|[^\\\[\]])*)\]\((?P<url>[^()\s]+)\) \(PR #(?P<n>[0-9]+)\)\n", line
        )
        self.assertIsNotNone(match, line)
        self.assertEqual(match["url"], "https://github.com/agent-grounds/fissile/pull/106")
        self.assertEqual(match["n"], "106")
        text = match["text"]
        self.assertEqual(re.sub(r"\\([!-/:-@\[-`{-~])", r"\1", text), title, text)
        unescaped = re.sub(r"\\.", "", text)
        for special in "*_`[]<>\\":
            self.assertNotIn(special, unescaped, f"{special!r} is not escaped in {text!r}")

    def test_the_repository_comes_from_the_origin_remote_when_run_locally(self):
        self.h.env_overrides["GITHUB_REPOSITORY"] = None
        self.h.git("remote", "add", "origin", "git@github.com:agent-grounds/fissile.git")
        sha = self.h.commit("Run locally", touch="src/lib.rs")
        self.h.merged(107, "Run locally", sha, "2026-10-04T10:00:00Z")

        self.assert_ok(self.h.prepare("0.11.1"))

        self.assertEqual(self.inline_section(), [pr_line(107, "Run locally")])


if __name__ == "__main__":
    unittest.main()
