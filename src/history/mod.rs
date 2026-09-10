//! Opt-in Git-backed debt direction and age for `fissile audit`
//! (§FS-004-check-audit.2.1).

mod data;
mod eval;
mod git;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

pub(crate) use data::History;
use data::{
    Address, Ceiling, CeilingChange, DeferredAge, EntryState, FindingAddress, FindingAge,
    MovementKind, Renamed, Snapshot,
};
use git::{Rename, RenameKind, Repository};

#[derive(Clone, Debug)]
pub(crate) struct Revision {
    sha: String,
    timestamp: i64,
}

#[derive(Debug)]
pub(crate) struct HistoryError {
    range: Option<String>,
    cause: String,
}

impl HistoryError {
    fn new(range: &str, cause: impl Into<String>) -> Self {
        Self {
            range: Some(range.to_owned()),
            cause: cause.into(),
        }
    }
}

impl fmt::Display for HistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.range {
            Some(range) => write!(formatter, "history {range}: {}", self.cause),
            None => formatter.write_str(&self.cause),
        }
    }
}

impl From<HistoryError> for crate::cli::CommandError {
    fn from(error: HistoryError) -> Self {
        crate::cli::CommandError::Usage(error.to_string())
    }
}

pub(crate) fn evaluate(
    root: &Path,
    range: &str,
    config_path: Option<&Path>,
) -> Result<History, HistoryError> {
    let (from_expression, to_expression) = parse_range(range)?;
    let repository = Repository::open(root, range)?;
    let from = repository.resolve(from_expression)?;
    let to = repository.resolve(to_expression)?;
    repository.require_ancestor(from_expression, &from, &to)?;
    let revisions = repository.revisions(&to)?;
    let from_index = revisions
        .iter()
        .position(|revision| revision.sha == from)
        .ok_or_else(|| HistoryError::new(range, "comparison ancestry is unavailable"))?;

    let historical_config = historical_config(root, config_path, range)?;
    let mut snapshots = Vec::with_capacity(revisions.len());
    for revision in &revisions {
        snapshots.push(eval::snapshot(
            &repository,
            revision,
            historical_config.as_deref(),
        )?);
    }
    let transitions = transitions(&repository, &snapshots)?;
    let mut history = classify(&snapshots, &transitions, from_index, range)?;
    let reaches_boundary = history
        .deferred_ages
        .iter()
        .any(|age| age.first_seen_commit == snapshots[0].sha)
        || history
            .soft_finding_ages
            .iter()
            .any(|age| age.first_seen_commit == snapshots[0].sha);
    if repository.is_shallow()? && reaches_boundary {
        return Err(HistoryError::new(
            range,
            "history before the shallow boundary is unavailable",
        ));
    }
    sort_history(&mut history);
    Ok(history)
}

fn historical_config(
    root: &Path,
    config_path: Option<&Path>,
    range: &str,
) -> Result<Option<std::path::PathBuf>, HistoryError> {
    let Some(path) = config_path else {
        return Ok(None);
    };
    if !path.is_absolute() {
        return Ok(Some(path.to_path_buf()));
    }
    let canonical_root = root.canonicalize().map_err(|error| {
        HistoryError::new(range, format!("repository root is unavailable: {error}"))
    })?;
    path.strip_prefix(&canonical_root)
        .map(|relative| Some(relative.to_path_buf()))
        .map_err(|_| HistoryError::new(range, "configured file is outside the committed tree"))
}

fn parse_range(range: &str) -> Result<(&str, &str), HistoryError> {
    if range.contains("...") || range.matches("..").count() != 1 {
        return Err(HistoryError::new(range, "expected one <from>..<to> range"));
    }
    let (from, to) = range.split_once("..").unwrap_or(("", ""));
    if from.is_empty() || to.is_empty() {
        return Err(HistoryError::new(range, "expected one <from>..<to> range"));
    }
    Ok((from, to))
}

#[derive(Default)]
struct Transition {
    renames: Vec<Rename>,
}

impl Transition {
    fn map_path(&self, path: &str) -> (String, Option<MovementKind>) {
        if let Some(rename) = self.renames.iter().find(|rename| rename.old == path) {
            return (
                rename.new.clone(),
                Some(match rename.kind {
                    RenameKind::ExactFile => MovementKind::ExactFile,
                    RenameKind::Directory => MovementKind::Directory,
                }),
            );
        }
        for rename in &self.renames {
            if rename.kind != RenameKind::Directory {
                continue;
            }
            let Some(old_parent) = Path::new(&rename.old).parent() else {
                continue;
            };
            let Some(new_parent) = Path::new(&rename.new).parent() else {
                continue;
            };
            let old_prefix = old_parent.to_string_lossy();
            if let Some(rest) = path.strip_prefix(old_prefix.as_ref())
                && (rest.is_empty() || rest.starts_with('/'))
            {
                return (
                    format!("{}{rest}", new_parent.to_string_lossy()),
                    Some(MovementKind::Directory),
                );
            }
        }
        (path.to_owned(), None)
    }

    fn map_address(&self, address: &Address) -> (Address, Option<MovementKind>) {
        let mut mapped = address.clone();
        let (registry, registry_kind) = self.map_path(&address.registry);
        let (path, path_kind) = self.map_path(&address.path);
        mapped.registry = registry;
        mapped.path = path;
        (mapped, path_kind.or(registry_kind))
    }

    fn map_finding(&self, address: &FindingAddress) -> FindingAddress {
        let mut mapped = address.clone();
        mapped.path = self.map_path(&address.path).0;
        mapped
    }
}

fn transitions(
    repository: &Repository<'_>,
    snapshots: &[Snapshot],
) -> Result<Vec<Transition>, HistoryError> {
    let mut result = Vec::with_capacity(snapshots.len().saturating_sub(1));
    for pair in snapshots.windows(2) {
        let old = &pair[0];
        let new = &pair[1];
        let raw_renames = repository.renames(&old.sha, &new.sha)?;
        let renames = refine_exception_renames(old, new, raw_renames);
        refuse_ambiguous_blob_groups(repository, old, new, &renames)?;
        result.push(Transition { renames });
    }
    Ok(result)
}

fn refine_exception_renames(old: &Snapshot, new: &Snapshot, raw: Vec<Rename>) -> Vec<Rename> {
    let mut proven = Vec::new();
    for old_entry in &old.entries {
        if new
            .entries
            .iter()
            .any(|entry| entry.address == old_entry.address)
        {
            continue;
        }
        let candidates: Vec<&EntryState> = new
            .entries
            .iter()
            .filter(|new_entry| {
                !old.entries
                    .iter()
                    .any(|entry| entry.address == new_entry.address)
                    && same_address_except_path(&old_entry.address, &new_entry.address)
                    && old_entry.rename_hint == new_entry.rename_hint
                    && same_blob(old, &old_entry.address.path, new, &new_entry.address.path)
            })
            .collect();
        if candidates.len() != 1 {
            continue;
        }
        let candidate = candidates[0];
        let reverse_count = old
            .entries
            .iter()
            .filter(|other| {
                !new.entries
                    .iter()
                    .any(|entry| entry.address == other.address)
                    && same_address_except_path(&other.address, &candidate.address)
                    && other.rename_hint == candidate.rename_hint
                    && same_blob(old, &other.address.path, new, &candidate.address.path)
            })
            .count();
        if reverse_count == 1 {
            proven.push(Rename {
                old: old_entry.address.path.clone(),
                new: candidate.address.path.clone(),
                kind: rename_kind(&old_entry.address.path, &candidate.address.path),
            });
        }
    }
    let touched_old: BTreeSet<String> = proven.iter().map(|rename| rename.old.clone()).collect();
    let touched_new: BTreeSet<String> = proven.iter().map(|rename| rename.new.clone()).collect();
    proven.extend(raw.into_iter().filter(|rename| {
        !touched_old.contains(rename.old.as_str()) && !touched_new.contains(rename.new.as_str())
    }));
    proven.sort_by(|left, right| left.old.cmp(&right.old).then(left.new.cmp(&right.new)));
    proven
}

fn same_address_except_path(old: &Address, new: &Address) -> bool {
    old.registry == new.registry
        && old.severity == new.severity
        && old.match_kind == new.match_kind
        && old.unit == new.unit
        && old.rules == new.rules
}

fn same_blob(old: &Snapshot, old_path: &str, new: &Snapshot, new_path: &str) -> bool {
    old.blobs
        .get(old_path)
        .zip(new.blobs.get(new_path))
        .is_some_and(|(old_blob, new_blob)| old_blob == new_blob)
}

fn rename_kind(old: &str, new: &str) -> RenameKind {
    if Path::new(old).file_name() == Path::new(new).file_name()
        && Path::new(old).parent() != Path::new(new).parent()
    {
        RenameKind::Directory
    } else {
        RenameKind::ExactFile
    }
}

fn refuse_ambiguous_blob_groups(
    repository: &Repository<'_>,
    old: &Snapshot,
    new: &Snapshot,
    renames: &[Rename],
) -> Result<(), HistoryError> {
    let mut old_debt_by_blob: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &old.entries {
        if new.blobs.contains_key(&entry.address.path) {
            continue;
        }
        if let Some(blob) = old.blobs.get(&entry.address.path) {
            *old_debt_by_blob.entry(blob).or_default() += 1;
        }
    }
    for (blob, old_count) in old_debt_by_blob {
        let mut destinations: Vec<&str> = renames
            .iter()
            .filter(|rename| {
                new.blobs
                    .get(&rename.new)
                    .is_some_and(|new_blob| new_blob == blob)
            })
            .filter(|rename| {
                new.entries
                    .iter()
                    .any(|entry| entry.address.path == rename.new)
                    || new
                        .findings
                        .iter()
                        .any(|finding| finding.address.path == rename.new)
            })
            .map(|rename| rename.new.as_str())
            .collect();
        destinations.sort_unstable();
        destinations.dedup();
        if !destinations.is_empty() && old_count > destinations.len() {
            return Err(repository.error_at(
                &new.sha,
                format!("ambiguous rename evidence for {}", destinations[0]),
            ));
        }
    }
    Ok(())
}

fn classify(
    snapshots: &[Snapshot],
    transitions: &[Transition],
    from_index: usize,
    range: &str,
) -> Result<History, HistoryError> {
    let first_seen_entries = entry_ages(snapshots, transitions);
    let first_seen_findings = finding_ages(snapshots, transitions);
    let from = &snapshots[from_index];
    let to = snapshots.last().expect("resolved history has one commit");
    let range_transitions = &transitions[from_index..];
    let mut matched_to = BTreeSet::new();
    let mut added = Vec::new();
    let mut retired = Vec::new();
    let mut raised = Vec::new();
    let mut lowered = Vec::new();
    let mut renamed = Vec::new();

    for old in &from.entries {
        let (mapped, movement) = map_through(&old.address, range_transitions);
        let Some(new) = to.entries.iter().find(|entry| entry.address == mapped) else {
            retired.push(Ceiling {
                address: old.address.clone(),
                value: old.value,
            });
            continue;
        };
        matched_to.insert(new.address.clone());
        if old.value < new.value {
            raised.push(CeilingChange {
                address: new.address.clone(),
                old_value: old.value,
                new_value: new.value,
            });
        } else if old.value > new.value {
            lowered.push(CeilingChange {
                address: new.address.clone(),
                old_value: old.value,
                new_value: new.value,
            });
        }
        if old.address != new.address {
            renamed.push(Renamed {
                kind: movement.unwrap_or(MovementKind::ExactFile),
                old: old.address.clone(),
                new: new.address.clone(),
            });
        }
    }
    for new in &to.entries {
        if !matched_to.contains(&new.address) {
            added.push(Ceiling {
                address: new.address.clone(),
                value: new.value,
            });
        }
    }

    let deferred_ages = to
        .entries
        .iter()
        .filter(|entry| entry.kind == crate::exceptions::Kind::Deferred)
        .map(|entry| {
            let first = first_seen_entries
                .get(&entry.address)
                .expect("current entry has provenance");
            let age_days = age_days(first.timestamp, to.timestamp, range)?;
            Ok(DeferredAge {
                entry: Ceiling {
                    address: entry.address.clone(),
                    value: entry.value,
                },
                first_seen_commit: first.sha.clone(),
                first_seen_date: first.date.clone(),
                age_days,
            })
        })
        .collect::<Result<Vec<_>, HistoryError>>()?;
    let soft_finding_ages = to
        .findings
        .iter()
        .map(|finding| {
            let first = first_seen_findings
                .get(&finding.address)
                .expect("current finding has provenance");
            let age_days = age_days(first.timestamp, to.timestamp, range)?;
            Ok(FindingAge {
                finding: finding.clone(),
                first_seen_commit: first.sha.clone(),
                first_seen_date: first.date.clone(),
                age_days,
            })
        })
        .collect::<Result<Vec<_>, HistoryError>>()?;

    Ok(History {
        from: from.sha.clone(),
        to: to.sha.clone(),
        added,
        retired,
        raised,
        lowered,
        renamed,
        deferred_ages,
        soft_finding_ages,
    })
}

fn map_through(address: &Address, transitions: &[Transition]) -> (Address, Option<MovementKind>) {
    let mut mapped = address.clone();
    let mut kind = None;
    for transition in transitions {
        let (next, moved) = transition.map_address(&mapped);
        mapped = next;
        if moved == Some(MovementKind::Directory) || kind.is_none() {
            kind = moved.or(kind);
        }
    }
    (mapped, kind)
}

#[derive(Clone)]
struct FirstSeen {
    sha: String,
    timestamp: i64,
    date: String,
}

impl FirstSeen {
    fn at(snapshot: &Snapshot) -> Self {
        Self {
            sha: snapshot.sha.clone(),
            timestamp: snapshot.timestamp,
            date: snapshot.date.clone(),
        }
    }
}

fn entry_ages(snapshots: &[Snapshot], transitions: &[Transition]) -> BTreeMap<Address, FirstSeen> {
    let mut ages = BTreeMap::new();
    for entry in snapshots[0]
        .entries
        .iter()
        .filter(|entry| entry.kind == crate::exceptions::Kind::Deferred)
    {
        ages.insert(entry.address.clone(), FirstSeen::at(&snapshots[0]));
    }
    for (index, current) in snapshots.iter().enumerate().skip(1) {
        let previous = &snapshots[index - 1];
        let transition = &transitions[index - 1];
        let mut next = BTreeMap::new();
        for entry in current
            .entries
            .iter()
            .filter(|entry| entry.kind == crate::exceptions::Kind::Deferred)
        {
            let inherited = previous.entries.iter().find_map(|old| {
                (old.kind == crate::exceptions::Kind::Deferred
                    && transition.map_address(&old.address).0 == entry.address)
                    .then(|| ages.get(&old.address).cloned())
                    .flatten()
            });
            next.insert(
                entry.address.clone(),
                inherited.unwrap_or_else(|| FirstSeen::at(current)),
            );
        }
        ages = next;
    }
    ages
}

fn finding_ages(
    snapshots: &[Snapshot],
    transitions: &[Transition],
) -> BTreeMap<FindingAddress, FirstSeen> {
    let mut ages = snapshots[0]
        .findings
        .iter()
        .map(|finding| (finding.address.clone(), FirstSeen::at(&snapshots[0])))
        .collect::<BTreeMap<_, _>>();
    for (index, current) in snapshots.iter().enumerate().skip(1) {
        let previous = &snapshots[index - 1];
        let transition = &transitions[index - 1];
        let mut next = BTreeMap::new();
        for finding in &current.findings {
            let inherited = previous.findings.iter().find_map(|old| {
                (transition.map_finding(&old.address) == finding.address)
                    .then(|| ages.get(&old.address).cloned())
                    .flatten()
            });
            next.insert(
                finding.address.clone(),
                inherited.unwrap_or_else(|| FirstSeen::at(current)),
            );
        }
        ages = next;
    }
    ages
}

fn age_days(first: i64, to: i64, range: &str) -> Result<u64, HistoryError> {
    let elapsed = to
        .checked_sub(first)
        .filter(|elapsed| *elapsed >= 0)
        .ok_or_else(|| {
            HistoryError::new(range, "first-seen timestamp is later than the endpoint")
        })?;
    Ok((elapsed / 86_400) as u64)
}

fn sort_history(history: &mut History) {
    history
        .added
        .sort_by(|a, b| a.address.cmp(&b.address).then(a.value.cmp(&b.value)));
    history
        .retired
        .sort_by(|a, b| a.address.cmp(&b.address).then(a.value.cmp(&b.value)));
    history.raised.sort_by(|a, b| {
        a.address
            .cmp(&b.address)
            .then(a.old_value.cmp(&b.old_value))
            .then(a.new_value.cmp(&b.new_value))
    });
    history.lowered.sort_by(|a, b| {
        a.address
            .cmp(&b.address)
            .then(a.old_value.cmp(&b.old_value))
            .then(a.new_value.cmp(&b.new_value))
    });
    history
        .renamed
        .sort_by(|a, b| a.new.cmp(&b.new).then(a.old.cmp(&b.old)));
    history.deferred_ages.sort_by(|a, b| {
        b.age_days
            .cmp(&a.age_days)
            .then(a.first_seen_commit.cmp(&b.first_seen_commit))
            .then(a.entry.address.cmp(&b.entry.address))
    });
    history.soft_finding_ages.sort_by(|a, b| {
        b.age_days
            .cmp(&a.age_days)
            .then(a.first_seen_commit.cmp(&b.first_seen_commit))
            .then(a.finding.address.cmp(&b.finding.address))
    });
}

pub(crate) fn utc_date(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}
