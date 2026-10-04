"""Read a release's notes from git and GitHub: the merged pull requests. §AR-001-ci.8.3

`collect` returns the lines `prepare_changelog_release.py prepare` writes as the
body of the new release section. It only reads - git locally and GitHub through
the `gh` CLI - and raises `ReleaseNotesError` for every refusal of
§AR-001-ci.8.3.6, so the caller can refuse before anything is written.
"""

from __future__ import annotations

import datetime as _datetime
import json
import os
import re
import subprocess
from dataclasses import dataclass
from typing import Sequence


EXACT_TAG_RE = re.compile(r"^v(?P<major>[0-9]+)\.(?P<minor>[0-9]+)\.(?P<patch>[0-9]+)$")
REPOSITORY_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
GITHUB_REMOTE_RE = re.compile(r"github\.com[:/]+(?P<repo>[^/]+/[^/]+?)(?:\.git)?/*$")
MARKDOWN_SPECIAL = "\\`*_[]<>&~"


class ReleaseNotesError(Exception):
    pass


@dataclass(frozen=True)
class PullRequest:
    number: int
    title: str
    merged_at: _datetime.datetime


def collect(inline_version: str) -> list[str]:
    """The release section's body: one line per merged pull request in the range."""
    tag, tip = release_range(inline_version)
    commits = _git("rev-list", f"{tag}..{tip}").split()
    if not commits:
        raise ReleaseNotesError(f"no merged pull request in {tag}..{tip}: the range has no commits")
    repository = _repository()
    pulls = merged_pull_requests(repository, commits)
    if not pulls:
        raise ReleaseNotesError(
            f"no merged pull request in {tag}..{tip}: none of its {len(commits)} commits came from "
            f"a pull request merged into main of {repository}"
        )
    return [format_line(repository, pull) for pull in pulls]


def release_range(inline_version: str) -> tuple[str, str]:
    """The previous release's tag and the frozen candidate tip. §AR-001-ci.8.3.1"""
    if _git("rev-parse", "--is-shallow-repository") == "true":
        raise ReleaseNotesError(
            "this is a shallow clone, so the commits since the previous tag cannot be listed; "
            "fetch the full history (git fetch --unshallow, or fetch-depth: 0)"
        )
    tip = _git("rev-parse", "--verify", "HEAD^{commit}")
    tags = []
    for name in _git("tag", "--list", "v*.*.*", "--merged", tip).split():
        match = EXACT_TAG_RE.match(name)
        if match is not None:
            tags.append((tuple(int(part) for part in match.groups()), name))
    if not tags:
        raise ReleaseNotesError(f"no vX.Y.Z tag is reachable from HEAD ({tip}); cut the first release by hand")
    tag = max(tags)[1]
    if tag[1:] != inline_version:
        raise ReleaseNotesError(
            f"the highest reachable tag {tag} is not the inline release {inline_version} of docs/changelog.md"
        )
    return tag, tip


def merged_pull_requests(repository: str, commits: Sequence[str]) -> list[PullRequest]:
    """Every pull request merged into main whose merge commit is in the range. §AR-001-ci.8.3.2"""
    in_range = {sha.lower() for sha in commits}
    seen: set[int] = set()
    kept: list[PullRequest] = []
    for sha in commits:
        for raw in _pulls_of_commit(repository, sha):
            number = _field(raw, "number", int, sha)
            if number in seen:
                continue
            seen.add(number)
            pull = _merged_into_main(repository, raw, number, in_range)
            if pull is not None:
                kept.append(pull)
    # §AR-001-ci.8.3.4: merged_at newest first, then the highest number.
    kept.sort(key=lambda pull: (pull.merged_at, pull.number), reverse=True)
    return kept


def format_line(repository: str, pull: PullRequest) -> str:
    """One line of the notes, the title escaped to show literally. §AR-001-ci.8.3.4"""
    url = f"https://github.com/{repository}/pull/{pull.number}"
    return f"- [{escape_markdown(pull.title)}]({url}) (PR #{pull.number})\n"


def escape_markdown(text: str) -> str:
    return "".join(f"\\{char}" if char in MARKDOWN_SPECIAL else char for char in text)


def _merged_into_main(repository: str, raw: dict, number: int, in_range: set[str]) -> PullRequest | None:
    title = _field(raw, "title", str, f"pull request #{number}")
    merged_at = _optional(raw, "merged_at", str, number)
    merge_sha = _optional(raw, "merge_commit_sha", str, number)
    base = _field(raw, "base", dict, f"pull request #{number}")
    base_ref = _field(base, "ref", str, f"pull request #{number} base")
    base_repo = _optional(base, "repo", dict, number)
    if merged_at is None:
        return None
    if merge_sha is None:
        raise ReleaseNotesError(f"malformed data from GitHub: merged pull request #{number} has no merge_commit_sha")
    if base_repo is None or base_ref != "main":
        return None
    if _field(base_repo, "full_name", str, f"pull request #{number} base repo").lower() != repository.lower():
        return None
    if merge_sha.lower() not in in_range:
        return None
    return PullRequest(number, title.strip(), _timestamp(merged_at, number))


def _pulls_of_commit(repository: str, sha: str) -> list[dict]:
    """Every page the commit-to-pull-request endpoint returns for one commit."""
    path = f"repos/{repository}/commits/{sha}/pulls?per_page=100"
    command = ["gh", "api", "--paginate", "--slurp", "-H", "Accept: application/vnd.github+json", path]
    try:
        result = subprocess.run(command, text=True, capture_output=True)
    except FileNotFoundError as exc:
        raise ReleaseNotesError("the gh CLI is not installed; it reads the merged pull requests") from exc
    if result.returncode != 0:
        if "unknown flag: --slurp" in result.stderr:
            raise ReleaseNotesError("gh 2.48 or newer is required; this gh has no `api --slurp`")
        detail = result.stderr.strip() or f"gh exited {result.returncode}"
        raise ReleaseNotesError(f"GitHub API request {path} failed, so its pages could not be read: {detail}")
    try:
        pages = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ReleaseNotesError(f"malformed JSON from GitHub for {path}: {exc}") from exc
    if not isinstance(pages, list) or not all(isinstance(page, list) for page in pages):
        raise ReleaseNotesError(f"malformed data from GitHub for {path}: expected a list of pages")
    pulls = [pull for page in pages for pull in page]
    if not all(isinstance(pull, dict) for pull in pulls):
        raise ReleaseNotesError(f"malformed data from GitHub for {path}: a page holds a non-object")
    return pulls


def _field(data: dict, key: str, kind: type, owner: object):
    value = data.get(key)
    if not isinstance(value, kind) or (kind is int and isinstance(value, bool)):
        raise ReleaseNotesError(f"malformed data from GitHub: {owner} has no valid {key}")
    return value


def _optional(data: dict, key: str, kind: type, number: int):
    if key not in data:
        raise ReleaseNotesError(f"malformed data from GitHub: pull request #{number} has no {key}")
    value = data[key]
    if value is not None and not isinstance(value, kind):
        raise ReleaseNotesError(f"malformed data from GitHub: pull request #{number} has an invalid {key}")
    return value


def _timestamp(text: str, number: int) -> _datetime.datetime:
    try:
        value = _datetime.datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ReleaseNotesError(f"malformed data from GitHub: pull request #{number} merged_at {text!r}") from exc
    if value.tzinfo is None:
        value = value.replace(tzinfo=_datetime.timezone.utc)
    return value


def _repository() -> str:
    """GITHUB_REPOSITORY, or the GitHub `origin` remote when run locally. §AR-001-ci.8.3.2"""
    repository = os.environ.get("GITHUB_REPOSITORY", "").strip()
    if not repository:
        try:
            remote = _git("remote", "get-url", "origin")
        except ReleaseNotesError as exc:
            raise ReleaseNotesError("GITHUB_REPOSITORY is unset and there is no origin remote") from exc
        match = GITHUB_REMOTE_RE.search(remote)
        if match is None:
            raise ReleaseNotesError(f"GITHUB_REPOSITORY is unset and origin is not a GitHub remote: {remote}")
        repository = match.group("repo")
    if REPOSITORY_RE.match(repository) is None:
        raise ReleaseNotesError(f"the repository must look like owner/name, got {repository!r}")
    return repository


def _git(*args: str) -> str:
    try:
        result = subprocess.run(["git", *args], text=True, capture_output=True)
    except FileNotFoundError as exc:
        raise ReleaseNotesError("git is not installed") from exc
    if result.returncode != 0:
        raise ReleaseNotesError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout.strip()
