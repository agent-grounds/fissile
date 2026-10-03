//! `fissile exception remove` (§FS-009-exception-remove): delete one exception
//! entry. It addresses the entry exactly as `retune` does, refuses to delete one
//! that is still silencing a finding, and — alone among the commands — loads a
//! registry the rule check rejects, because repairing that state is what it is
//! for (§FS-009-exception-remove.2).

use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::{self, CommandError, Loaded};
use crate::entry::{self, Address};
use crate::exceptions::{Exception, MatchKind, Registries, RegistrySource, RemovalEntry};
use crate::report::{self, EvalError};
use crate::toml_lines;
use crate::{FileMeasurement, Glob, RuleHit, Severity, Unit, scan};

/// Inputs to `exception remove`. No `--max` and no `--unit`: the command states
/// no ceiling (§FS-009-exception-remove.1).
#[derive(Clone, Debug)]
pub struct RemoveOptions {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub path: String,
    pub severity: Severity,
    pub rules: Vec<String>,
    pub match_kind: MatchKind,
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub output: String,
    /// What discovery owes the reader: the deprecated config home this run was
    /// governed by (§FS-001-config.8.2).
    pub notes: Vec<String>,
}

pub fn run(options: &RemoveOptions) -> Result<Run, CommandError> {
    // Only soft removal may carry a missing-twin shadow, and only in the
    // separate address-only representation. Hard removal and every other
    // command keep the strict structural load (§FS-009-exception-remove.2).
    let (loaded, removal_entries) = match options.severity {
        Severity::Soft => {
            cli::load_for_soft_removal(&options.root, options.config_path.as_deref())?
        }
        Severity::Hard => {
            let loaded = cli::load_unvalidated(&options.root, options.config_path.as_deref())?;
            let entries = loaded
                .registries
                .hard
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, entry)| RemovalEntry::from_resolved(entry, index))
                .collect();
            (loaded, entries)
        }
    };
    let path = entry::match_path(&loaded, options.match_kind, &options.path)?;

    entry::validate_match(options.match_kind, &path)?;
    let rules = entry::resolve_rules(&loaded, &options.rules)?;
    let unit = rules[0].budget.unit;
    let registry_rel = entry::registry_path(&loaded, options.severity);

    let address = Address {
        severity: options.severity,
        path: &path,
        match_kind: options.match_kind,
        rules: &options.rules,
        unit,
    };
    // An address with no entry behind it has nothing to delete, and the entries
    // that are there are listed from what was just read
    // (§FS-009-exception-remove.1).
    let Some((document_index, existing)) = locate_removal(&removal_entries, &address)? else {
        return Err(CommandError::Usage(format!(
            "{}: no entry accepts {path} for this rule and unit, so there is nothing to \
             remove{}",
            registry_rel.display(),
            addressable(&removal_entries)
        )));
    };
    let existing = existing.clone();
    check_matcher(&existing, options.match_kind, &path, &registry_rel)?;

    // What the registry holds once the entry is gone. The refusal below reads it,
    // and the write is held to producing exactly this (§FS-009-exception-remove.5).
    let after = if let Some((resolved_index, resolved)) = existing.resolved() {
        let after = without(&loaded.registries, options.severity, resolved_index);
        check_silenced(&loaded, &after, resolved, &registry_rel)?;
        after
    } else {
        // An orphan was never a valid exception and cannot silence a finding;
        // removing it leaves the resolved registry view unchanged.
        loaded.registries.clone()
    };
    let mut after_removal_entries = removal_entries.clone();
    after_removal_entries.remove(document_index);

    let registry_path = loaded.root.join(&registry_rel);
    let text = cli::read_optional(&registry_path)?
        .ok_or_else(|| CommandError::Usage(format!("{} is missing", registry_rel.display())))?;
    let new_text = delete_block(&text, document_index, &registry_rel, existing.path())?;
    check_written(
        &loaded,
        options.severity,
        &after,
        &after_removal_entries,
        &new_text,
        &registry_rel,
    )?;

    let note = twin_note(twin(&loaded, options, &path, unit), unit);
    let head = format!(
        "{}: {} {} (accepted up to {} {unit})",
        registry_rel.display(),
        if options.dry_run {
            "would remove"
        } else {
            "removed"
        },
        existing.path(),
        existing.max_value()
    );
    if options.dry_run {
        return Ok(Run {
            output: with_note(
                format!("{head}\nwould update {}", registry_rel.display()),
                note,
            ),
            notes: cli::config_notes(&loaded.source),
        });
    }

    fs::write(&registry_path, &new_text)?;
    Ok(Run {
        output: with_note(head, note),
        notes: cli::config_notes(&loaded.source),
    })
}

/// How many entries a refusal lists before it says how many more there are. An
/// error message is not a report (§GOAL-003-friendly-output).
const LISTED: usize = 10;

/// Locate one entry in the selected registry's document-order repair view.
/// This mirrors normal address resolution, but can see an orphan that is
/// deliberately absent from [`Registries`] (§FS-009-exception-remove.2).
fn locate_removal<'a>(
    entries: &'a [RemovalEntry],
    address: &Address<'_>,
) -> Result<Option<(usize, &'a RemovalEntry)>, CommandError> {
    let mut found = entries.iter().enumerate().filter(|(_, existing)| {
        existing.max_unit() == address.unit
            && address
                .rules
                .iter()
                .any(|rule| existing.applies_to_rule(rule))
            && removal_matchers_overlap(existing, address.match_kind, address.path)
    });
    let Some((index, entry)) = found.next() else {
        return Ok(None);
    };
    if let Some((_, second)) = found.next() {
        return Err(CommandError::Usage(format!(
            "{}: {} spans more than one entry ({} and {}); address one at a time — \
             each entry is named by its own path matcher",
            entry.registry(),
            address.path,
            entry.path(),
            second.path()
        )));
    }
    Ok(Some((index, entry)))
}

fn removal_matchers_overlap(existing: &RemovalEntry, match_kind: MatchKind, path: &str) -> bool {
    match (existing.match_kind(), match_kind) {
        (MatchKind::Exact, MatchKind::Exact) => existing.path() == path,
        (MatchKind::Glob, MatchKind::Exact) => existing.matches_path(path),
        (MatchKind::Exact, MatchKind::Glob) => Glob::new(path).matches(existing.path()),
        (MatchKind::Glob, MatchKind::Glob) => {
            Glob::new(existing.path()).intersects(&Glob::new(path))
        }
    }
}

/// The entries the caller could have addressed, from the registry `remove` has
/// already read. `audit --stale-exceptions` would abort in exactly the state
/// this command exists to repair, so the refusal answers its own question
/// rather than naming a command that cannot run (§FS-009-exception-remove.1,
/// §DF-007-instructions-at-the-error-site).
fn addressable(entries: &[RemovalEntry]) -> String {
    if entries.is_empty() {
        return "; it holds no entries".to_owned();
    }
    let mut listed = String::from("; it holds:");
    for existing in entries.iter().take(LISTED) {
        listed.push_str(&format!(
            "\n  {} ({}, rules {}, up to {} {})",
            existing.path(),
            entry::match_str(existing.match_kind()),
            existing.rules().join(" "),
            existing.max_value(),
            existing.max_unit()
        ));
    }
    if entries.len() > LISTED {
        listed.push_str(&format!("\n  and {} more", entries.len() - LISTED));
    }
    listed
}

/// An address is a matcher, not just a path (§DF-005-exception-identity), so a
/// matcher that only *overlaps* the entry is the wrong address — and for
/// removal each direction is wrong for its own reason
/// (§FS-009-exception-remove.1).
fn check_matcher(
    existing: &RemovalEntry,
    addressed: MatchKind,
    path: &str,
    registry_rel: &Path,
) -> Result<(), CommandError> {
    match (existing.match_kind(), addressed) {
        // Deleting the class-wide entry from under one member drops the ceiling
        // for every other file the glob names, which the caller did not ask for.
        (MatchKind::Glob, MatchKind::Exact) => Err(CommandError::Usage(format!(
            "{}: {path} is covered by the glob entry {}, whose ceiling covers every file \
             that glob names; remove the class as `--match glob \"{}\"` if that is what \
             should go",
            registry_rel.display(),
            existing.path(),
            existing.path()
        ))),
        // The reverse deletes one file's entry under a spelling no entry carries,
        // and reports the change against a path the registry never held.
        (MatchKind::Exact, MatchKind::Glob) => Err(CommandError::Usage(format!(
            "{}: no glob entry accepts {path}; it spans the exact entry {}, which is \
             removed as `--match exact {}`",
            registry_rel.display(),
            existing.path(),
            existing.path()
        ))),
        _ => Ok(()),
    }
}

/// The registries as they stand with the addressed entry gone. Every check
/// §FS-003-exceptions.4 applies is per entry, so this is the whole effect of the
/// removal on the document's validity (§FS-009-exception-remove.2).
fn without(registries: &Registries, severity: Severity, index: usize) -> Registries {
    let mut after = registries.clone();
    match severity {
        Severity::Soft => {
            after.soft.remove(index);
        }
        Severity::Hard => {
            after.hard.remove(index);
        }
    }
    after
}

/// One finding, by what identifies it: the same finding standing before and
/// after the removal is the same tuple, and one that appears only after it is
/// what the removal would surface (§FS-009-exception-remove.3).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Finding {
    path: String,
    rule_id: String,
    severity: Severity,
    unit: Unit,
    actual: u64,
    limit: u64,
}

/// Refuse a removal that would report a file the repository decided to accept
/// (§FS-009-exception-remove.3). Only the files the entry's own matcher covers
/// can change verdict, so those are the only ones measured.
fn check_silenced(
    loaded: &Loaded,
    after: &Registries,
    existing: &Exception,
    registry_rel: &Path,
) -> Result<(), CommandError> {
    let mut surfaced: Vec<Finding> = Vec::new();
    for file in scan::walk_scope(&loaded.root, &loaded.config.scan)? {
        if !existing.matches_path(&file) {
            continue;
        }
        // A path that cannot be measured reports nothing under either registry,
        // so it silences nothing either (§FS-004-check-audit.5).
        let Ok(measurement) = scan::measure_file(&loaded.root, &file, &loaded.config.tokens) else {
            continue;
        };
        let hits = loaded
            .checker
            .evaluate(&measurement)
            .map_err(EvalError::from)?;
        let standing = reported(&loaded.registries, &measurement, &hits)?;
        for finding in reported(after, &measurement, &hits)? {
            if !standing.contains(&finding) {
                surfaced.push(finding);
            }
        }
    }

    let Some(first) = surfaced.first() else {
        return Ok(());
    };
    let more = match surfaced.len() {
        1 => String::new(),
        count => format!(", and {} more", count - 1),
    };
    Err(CommandError::Usage(format!(
        "{}: {} still silences a finding: {} measures {} {}, over rule {} {} limit {}{more}. \
         Removing the entry would report the file rather than record it — split the file \
         first, or leave the entry where it is",
        registry_rel.display(),
        existing.path,
        first.path,
        first.actual,
        first.unit,
        first.rule_id,
        first.severity,
        first.limit,
    )))
}

/// The standing findings for one measured file under a given registry state.
/// The rule hits are passed in because both states read the same ones, and
/// evaluating a file twice is waste (§GOAL-001-fast-feedback).
fn reported(
    registries: &Registries,
    measurement: &FileMeasurement,
    hits: &[RuleHit<'_>],
) -> Result<Vec<Finding>, CommandError> {
    Ok(report::evaluate_hits(registries, measurement, hits)?
        .iter()
        .filter(|outcome| outcome.is_reported())
        .map(|outcome| {
            let overflow = outcome.overflow();
            Finding {
                path: overflow.path.to_string_lossy().replace('\\', "/"),
                rule_id: overflow.rule_id.clone(),
                severity: overflow.severity,
                unit: overflow.unit,
                actual: overflow.actual,
                limit: overflow.limit,
            }
        })
        .collect())
}

/// Delete the `index`-th `[[exceptions]]` block — its header, its fields, and
/// the comment run written directly above it — preserving every other byte,
/// including the comments that lead into the entries that stay and the notes a
/// blank line leaves attached to no entry (§FS-009-exception-remove.4).
fn delete_block(
    text: &str,
    index: usize,
    registry: &Path,
    path: &str,
) -> Result<String, CommandError> {
    // Split rather than `lines()`: this round-trips the bytes that stay, the
    // final newline and any `\r` among them.
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let starts = toml_lines::block_starts(&lines);
    // Reached only by a registry whose blocks the parser and the line reader
    // count differently. Refusing beats cutting lines out of the wrong entry.
    let Some(&start) = starts.get(index) else {
        return Err(CommandError::Usage(format!(
            "{}: cannot find the entry for {path} in the document; edit it by hand",
            registry.display()
        )));
    };
    let structural = toml_lines::structural_lines(&lines);
    // Where the next entry's own lines begin: its header, less the comment run
    // written directly above it, which documents that entry and stays with it.
    let next = starts.get(index + 1).copied();
    let boundary = next.map_or(lines.len(), |next| lead_run(&lines, &structural, next));

    // The block's own last line. What trails it — blank lines, and comments a
    // blank line detached from every header — belongs to no entry, so it stays.
    let mut end = boundary;
    while end > start + 1 && is_gap(&lines[end - 1], structural[end - 1]) {
        end -= 1;
    }
    // The blank lines that separated this block from what follows go with it, so
    // the entries that remain keep the spacing they had.
    let mut cut = end;
    while cut < boundary && lines[cut].trim().is_empty() {
        cut += 1;
    }
    // The comment run written directly above the header records why this entry
    // is here, so it goes with the entry.
    let block = lead_run(&lines, &structural, start);
    lines.drain(block..cut);

    // Nothing follows the removed block but blank lines, so the document ends
    // where its last remaining line does, with one trailing newline.
    if next.is_none() {
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        lines.push(String::new());
    }
    Ok(lines.join("\n"))
}

/// The first line of the comment run written directly above `at`, or `at` itself
/// when a blank line separates the two: a comment belongs to the entry it leads
/// into, and a detached one belongs to no entry (§FS-009-exception-remove.4).
fn lead_run(lines: &[String], structural: &[bool], at: usize) -> usize {
    let mut start = at;
    while start > 0 && is_comment(&lines[start - 1], structural[start - 1]) {
        start -= 1;
    }
    start
}

/// Whether a line carries no entry field — blank, or a comment.
fn is_gap(line: &str, structural: bool) -> bool {
    line.trim().is_empty() || is_comment(line, structural)
}

/// A `#` line that is TOML structure rather than the prose of a `reason`.
fn is_comment(line: &str, structural: bool) -> bool {
    structural && line.trim_start().starts_with('#')
}

/// The document about to be written must hold exactly the entries that were
/// read, less the one addressed (§FS-009-exception-remove.5). Re-validating it
/// the way `add` and `retune` do would be the wrong guard here: the registry
/// `remove` repairs is one validation already rejects.
fn check_written(
    loaded: &Loaded,
    severity: Severity,
    after: &Registries,
    after_removal_entries: &[RemovalEntry],
    new_text: &str,
    registry_rel: &Path,
) -> Result<(), CommandError> {
    let configured = match severity {
        Severity::Soft => &loaded.config.exceptions.soft_registry,
        Severity::Hard => &loaded.config.exceptions.hard_registry,
    };
    let source = RegistrySource::new(configured, new_text);
    let unchanged = match severity {
        Severity::Soft => {
            let hard_text = cli::read_optional(&loaded.root.join(&loaded.hard_registry))?;
            let hard = hard_text
                .as_deref()
                .map(|text| RegistrySource::new(&loaded.config.exceptions.hard_registry, text));
            let (written, entries) = Registries::load_for_soft_removal(Some(source), hard)?;
            written.soft == after.soft
                && entries.len() == after_removal_entries.len()
                && entries
                    .iter()
                    .zip(after_removal_entries)
                    .all(|(written, expected)| written.same_written_entry(expected))
        }
        Severity::Hard => Registries::load(None, Some(source))?.hard == after.hard,
    };
    if !unchanged {
        return Err(CommandError::Usage(format!(
            "{}: removing that entry would change the ones that remain; edit the file by hand",
            registry_rel.display()
        )));
    }
    Ok(())
}

fn with_note(head: String, note: Option<String>) -> String {
    match note {
        Some(note) => format!("{head}\n{note}"),
        None => head,
    }
}

/// The same address in the registry the caller did not select.
fn twin<'a>(
    loaded: &'a Loaded,
    options: &RemoveOptions,
    path: &str,
    unit: Unit,
) -> Option<&'a Exception> {
    let address = Address {
        severity: options.severity.other(),
        path,
        match_kind: options.match_kind,
        rules: &options.rules,
        unit,
    };
    let (_, twin) = entry::locate(&loaded.registries, &address).ok().flatten()?;
    Some(twin)
}

/// `remove` never writes to the twin's registry — twin consistency is a
/// repository's policy, not the tool's — but a caller who has just deleted one
/// half should learn the other half is still accepting the file
/// (§FS-009-exception-remove.4).
fn twin_note(twin: Option<&Exception>, unit: Unit) -> Option<String> {
    let twin = twin?;
    Some(format!(
        "note: {} still accepts {} up to {} {unit}",
        twin.registry, twin.path, twin.max_value
    ))
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod tests;
