#!/usr/bin/env python3
"""Prepare and read changelog release sections. §AR-001-ci.8, §AR-001-ci.8.3"""

from __future__ import annotations

import argparse
import datetime as _datetime
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Sequence

import release_notes


VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
RELEASE_RE = re.compile(
    r"^## (?P<number>[0-9]+)\. \[(?P<version>[0-9]+\.[0-9]+\.[0-9]+)\] — (?P<date>[0-9]{4}-[0-9]{2}-[0-9]{2})\s*$"
)
OLDER_RE = re.compile(r"^## (?P<number>[0-9]+)\. Older releases\s*$")
PULL_LINE_RE = re.compile(r"^- \[(?P<title>(?:\\.|[^\\\[\]])*)\]\(https://github\.com/[^()\s]+/pull/[0-9]+\) \(PR #[0-9]+\)$")


class ChangelogError(Exception):
    pass


def prepare_release(changelog: Path, version: str, release_date: str) -> None:
    """Write `## 1. [version]` from the merged pull requests and archive the previous release.

    Everything is read and validated before anything is written (§AR-001-ci.8.3.6); the
    section's shape is §AR-001-ci.8.3.5 and its body is §AR-001-ci.8.3.
    """
    _validate_version(version)
    _validate_date(release_date)

    lines = _read_lines(changelog)
    sections = _find_top_level_sections(lines)
    if not sections:
        raise ChangelogError(f"{changelog} has no inline release section")
    latest = sections[0]
    latest_match = RELEASE_RE.match(_line_text(lines[latest]))
    if latest_match is None:
        raise ChangelogError(
            f"expected the inline release `## 1. [X.Y.Z] — YYYY-MM-DD` as the first section of {changelog}, "
            f"got: {_line_text(lines[latest])}"
        )
    older = _next_section_after(sections, latest, "Older releases")
    if OLDER_RE.match(_line_text(lines[older])) is None:
        raise ChangelogError(f"expected `## 2. Older releases` after the inline release, got: {_line_text(lines[older])}")

    previous_version = latest_match.group("version")
    previous_date = latest_match.group("date")
    if previous_version == version:
        raise ChangelogError(f"docs/changelog.md already has {version} as the inline latest release")

    archive_dir = changelog.parent / "changelog"
    archive_path = archive_dir / f"{previous_version}.md"
    if archive_path.exists():
        raise ChangelogError(f"archive already exists: {archive_path}")

    try:
        notes = release_notes.collect(previous_version)
    except release_notes.ReleaseNotesError as exc:
        raise ChangelogError(str(exc)) from exc

    previous_body = lines[latest + 1 : older]
    archived_body = [_rewrite_relative_links_for_archive(line) for line in previous_body]
    summary = _summary_from(previous_body)
    archive_lines = [f"# {previous_version} — {previous_date}\n", *archived_body]

    older_body = lines[older + 1 :]
    older_body = _drop_leading_blank_lines(older_body)
    archive_link = f"- [{previous_version}](changelog/{previous_version}.md) — {previous_date}: {summary}\n"

    new_lines = [
        *lines[:latest],
        f"## 1. [{version}] — {release_date}\n",
        "\n",
        *notes,
        "\n",
        "## 2. Older releases\n",
        "\n",
        archive_link,
        *older_body,
    ]
    _write_release(changelog, new_lines, archive_path, archive_lines)


def _write_release(changelog: Path, lines: Sequence[str], archive: Path, archive_lines: Sequence[str]) -> None:
    """Create the archive, then replace the changelog; undo the archive if that fails. §AR-001-ci.8.3.6"""
    created_dir = not archive.parent.exists()
    try:
        archive.parent.mkdir(parents=True, exist_ok=True)
        with archive.open("x", encoding="utf-8") as handle:
            handle.write("".join(archive_lines))
    except OSError as exc:
        _remove_quietly(archive, archive.parent if created_dir else None)
        raise ChangelogError(f"could not write the archive {archive}: {exc}") from exc

    temporary = None
    try:
        descriptor, name = tempfile.mkstemp(prefix=".changelog-", dir=changelog.parent)
        temporary = Path(name)
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write("".join(lines))
        shutil.copymode(changelog, temporary)
        os.replace(temporary, changelog)
    except OSError as exc:
        if temporary is not None:
            _remove_quietly(temporary, None)
        _remove_quietly(archive, archive.parent if created_dir else None)
        raise ChangelogError(f"could not replace {changelog}, so the archive {archive} was removed again: {exc}") from exc


def _remove_quietly(path: Path, directory: Path | None) -> None:
    try:
        path.unlink(missing_ok=True)
        if directory is not None:
            directory.rmdir()
    except OSError:
        pass


def extract_notes(changelog: Path, version: str, output: Path) -> None:
    _validate_version(version)
    lines = _read_lines(changelog)
    sections = _find_top_level_sections(lines)

    for index, section_start in enumerate(sections):
        match = RELEASE_RE.match(_line_text(lines[section_start]))
        if match is None or match.group("version") != version:
            continue
        section_end = sections[index + 1] if index + 1 < len(sections) else len(lines)
        body = _trim_blank_lines(lines[section_start + 1 : section_end])
        if not body:
            raise ChangelogError(f"release {version} has an empty changelog section")
        _write_lines(output, [*body, "\n"])
        return

    raise ChangelogError(f"release {version} is not the inline changelog release")


def _find_top_level_sections(lines: Sequence[str]) -> list[int]:
    return [index for index, line in enumerate(lines) if line.startswith("## ") and not line.startswith("### ")]


def _next_section_after(sections: Sequence[int], after: int, name: str) -> int:
    for section in sections:
        if section > after:
            return section
    raise ChangelogError(f"missing {name} section")


def _read_lines(path: Path) -> list[str]:
    try:
        return path.read_text(encoding="utf-8").splitlines(keepends=True)
    except FileNotFoundError as exc:
        raise ChangelogError(f"missing changelog: {path}") from exc


def _write_lines(path: Path, lines: Sequence[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(lines), encoding="utf-8")


def _line_text(line: str) -> str:
    return line.rstrip("\r\n")


def _trim_blank_lines(lines: Sequence[str]) -> list[str]:
    trimmed = list(lines)
    while trimmed and not trimmed[0].strip():
        trimmed.pop(0)
    while trimmed and not trimmed[-1].strip():
        trimmed.pop()
    if trimmed and not trimmed[-1].endswith(("\n", "\r")):
        trimmed[-1] += "\n"
    return trimmed


def _drop_leading_blank_lines(lines: Sequence[str]) -> list[str]:
    trimmed = list(lines)
    while trimmed and not trimmed[0].strip():
        trimmed.pop(0)
    return trimmed


def _summary_from(lines: Sequence[str]) -> str:
    """A prose body's first sentence; a generated list's first title. §AR-001-ci.8.3.5"""
    listed = [PULL_LINE_RE.match(_line_text(line)) for line in lines if line.strip()]
    if listed and all(listed):
        return listed[0].group("title")

    paragraph: list[str] = []
    for line in lines:
        stripped = line.strip()
        if not stripped:
            if paragraph:
                break
            continue
        if stripped.startswith("#"):
            continue
        paragraph.append(stripped)

    if not paragraph:
        return "release notes."

    text = re.sub(r"\s+", " ", " ".join(paragraph))
    first_sentence = re.match(r"(.+?[.!?])(?:\s|$)", text)
    if first_sentence is not None:
        return first_sentence.group(1)
    return text


def _rewrite_relative_links_for_archive(line: str) -> str:
    def rewrite(match: re.Match[str]) -> str:
        destination = match.group("destination")
        if destination.startswith(("#", "/", "../", "http://", "https://", "mailto:")):
            return match.group(0)
        fragment = match.group("fragment") or ""
        return f"{match.group('prefix')}../{destination}{fragment})"

    return re.sub(
        r"(?P<prefix>\]\()(?P<destination>[^)#][^)#]*)(?P<fragment>#[^)]*)?\)",
        rewrite,
        line,
    )


def _validate_version(version: str) -> None:
    if VERSION_RE.match(version) is None:
        raise ChangelogError(f"version must look like 0.1.0, got {version!r}")


def _validate_date(release_date: str) -> None:
    try:
        _datetime.date.fromisoformat(release_date)
    except ValueError as exc:
        raise ChangelogError(f"date must look like YYYY-MM-DD, got {release_date!r}") from exc


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Prepare or read docs/changelog.md release sections.")
    parser.add_argument("--changelog", type=Path, default=Path("docs/changelog.md"))
    subparsers = parser.add_subparsers(dest="command", required=True)

    prepare = subparsers.add_parser("prepare", help="write the next release from the pull requests merged since the previous tag")
    prepare.add_argument("version")
    prepare.add_argument("--date", default=_datetime.date.today().isoformat())

    notes = subparsers.add_parser("notes", help="write release notes for the inline release")
    notes.add_argument("version")
    notes.add_argument("--output", type=Path, required=True)

    args = parser.parse_args(argv)
    try:
        if args.command == "prepare":
            prepare_release(args.changelog, args.version, args.date)
        elif args.command == "notes":
            extract_notes(args.changelog, args.version, args.output)
        else:
            raise AssertionError(args.command)
    except ChangelogError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
