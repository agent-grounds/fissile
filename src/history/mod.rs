//! Opt-in Git-backed debt direction and age for `fissile audit`
//! (§FS-004-check-audit.2.1).

mod data;
mod eval;
mod git;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

pub(crate) use data::History;
use data::{
    Address, Ceiling, CeilingChange, DeferredAge, FindingAddress, FindingAge, MovementKind,
    Renamed, Snapshot,
};
use git::{Rename, RenameKind, Repository};

#[derive(Clone, Debug)]
pub(crate) struct Revision {
    sha: String,
    timestamp: i64,
    parents: Vec<String>,
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
    let boundaries = repository.shallow_boundaries()?;
    let reached_boundary = history
        .deferred_ages
        .iter()
        .map(|age| age.first_seen_commit.as_str())
        .chain(
            history
                .soft_finding_ages
                .iter()
                .map(|age| age.first_seen_commit.as_str()),
        )
        .find(|commit| boundaries.contains(*commit));
    if let Some(boundary) = reached_boundary {
        return Err(repository.error_at(
            boundary,
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
) -> Result<Option<PathBuf>, HistoryError> {
    let Some(path) = config_path else {
        return Ok(None);
    };
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(HistoryError::new(
            range,
            "configured file is outside the committed tree",
        ));
    }
    let canonical_root = root.canonicalize().map_err(|error| {
        HistoryError::new(range, format!("repository root is unavailable: {error}"))
    })?;
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        canonical_root.join(path)
    };
    let canonical = full
        .canonicalize()
        .map_err(|_| HistoryError::new(range, "configured file is outside the committed tree"))?;
    let relative = canonical
        .strip_prefix(&canonical_root)
        .map_err(|_| HistoryError::new(range, "configured file is outside the committed tree"))?;
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(HistoryError::new(
            range,
            "configured file is outside the committed tree",
        ));
    }
    Ok(Some(relative.to_path_buf()))
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
    entry_renames: Vec<Rename>,
}

type Transitions = BTreeMap<(String, String), Transition>;

impl Transition {
    fn map_path(&self, path: &str) -> (String, Option<MovementKind>) {
        self.map_path_with(path, &self.renames)
    }

    fn map_entry_path(&self, path: &str) -> (String, Option<MovementKind>) {
        self.map_path_with(path, &self.entry_renames)
    }

    fn map_path_with(&self, path: &str, renames: &[Rename]) -> (String, Option<MovementKind>) {
        if let Some(rename) = renames.iter().find(|rename| rename.old == path) {
            return (
                rename.new.clone(),
                Some(match rename.kind {
                    RenameKind::ExactFile => MovementKind::ExactFile,
                    RenameKind::Directory => MovementKind::Directory,
                }),
            );
        }
        for rename in renames {
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
        let (registry, registry_kind) = self.map_entry_path(&address.registry);
        let (path, path_kind) = self.map_entry_path(&address.path);
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
) -> Result<Transitions, HistoryError> {
    let by_sha: BTreeMap<&str, &Snapshot> = snapshots
        .iter()
        .map(|snapshot| (snapshot.sha.as_str(), snapshot))
        .collect();
    let mut result = BTreeMap::new();
    for new in snapshots {
        for parent in &new.parents {
            let Some(old) = by_sha.get(parent.as_str()) else {
                continue;
            };
            let raw_renames = repository.renames(&old.sha, &new.sha)?;
            let renames = refine_exception_renames(old, new, raw_renames);
            refuse_ambiguous_blob_groups(repository, old, new)?;
            let entry_renames = renames
                .iter()
                .filter(|rename| rename_carries_entry_identity(old, new, rename))
                .cloned()
                .collect();
            result.insert(
                (old.sha.clone(), new.sha.clone()),
                Transition {
                    renames,
                    entry_renames,
                },
            );
        }
    }
    Ok(result)
}

fn refine_exception_renames(old: &Snapshot, new: &Snapshot, mut raw: Vec<Rename>) -> Vec<Rename> {
    let candidates: BTreeSet<(String, String)> = raw
        .iter()
        .filter_map(|rename| {
            let old_parent = Path::new(&rename.old).parent()?.to_str()?;
            let new_parent = Path::new(&rename.new).parent()?.to_str()?;
            (Path::new(&rename.old).file_name() == Path::new(&rename.new).file_name()
                && old_parent != new_parent)
                .then(|| (old_parent.to_owned(), new_parent.to_owned()))
        })
        .collect();
    for (old_prefix, new_prefix) in candidates {
        if directory_mapping_is_proven(old, new, &raw, &old_prefix, &new_prefix) {
            for rename in &mut raw {
                if replacement(&rename.old, &old_prefix, &new_prefix).as_deref()
                    == Some(rename.new.as_str())
                {
                    rename.kind = RenameKind::Directory;
                }
            }
        }
    }
    raw.sort_by(|left, right| left.old.cmp(&right.old).then(left.new.cmp(&right.new)));
    raw
}

fn same_address_except_path(old: &Address, new: &Address) -> bool {
    old.registry == new.registry
        && old.severity == new.severity
        && old.match_kind == new.match_kind
        && old.unit == new.unit
        && old.rules == new.rules
}

fn rename_carries_entry_identity(old: &Snapshot, new: &Snapshot, rename: &Rename) -> bool {
    if rename.kind == RenameKind::Directory {
        return true;
    }
    old.entries.iter().any(|old_entry| {
        let mut mapped = old_entry.address.clone();
        if mapped.path == rename.old {
            mapped.path = rename.new.clone();
        }
        if mapped.registry == rename.old {
            mapped.registry = rename.new.clone();
        }
        mapped != old_entry.address && new.entries.iter().any(|entry| entry.address == mapped)
    })
}

fn directory_mapping_is_proven(
    old: &Snapshot,
    new: &Snapshot,
    renames: &[Rename],
    old_prefix: &str,
    new_prefix: &str,
) -> bool {
    let sources: Vec<&str> = old
        .blobs
        .keys()
        .map(String::as_str)
        .filter(|path| replacement(path, old_prefix, new_prefix).is_some())
        .collect();
    let destinations: Vec<&str> = new
        .blobs
        .keys()
        .map(String::as_str)
        .filter(|path| replacement(path, new_prefix, old_prefix).is_some())
        .collect();
    let old_entries: Vec<&Address> = old
        .entries
        .iter()
        .map(|entry| &entry.address)
        .filter(|address| replacement(&address.path, old_prefix, new_prefix).is_some())
        .collect();
    let new_entries: Vec<&Address> = new
        .entries
        .iter()
        .map(|entry| &entry.address)
        .filter(|address| replacement(&address.path, new_prefix, old_prefix).is_some())
        .collect();
    let old_config: Vec<&str> = old
        .config_paths
        .iter()
        .map(String::as_str)
        .filter(|path| replacement(path, old_prefix, new_prefix).is_some())
        .collect();
    let new_config: Vec<&str> = new
        .config_paths
        .iter()
        .map(String::as_str)
        .filter(|path| replacement(path, new_prefix, old_prefix).is_some())
        .collect();
    !sources.is_empty()
        && sources.len() == destinations.len()
        && sources.iter().all(|source| {
            let expected = replacement(source, old_prefix, new_prefix).expect("descendant maps");
            renames
                .iter()
                .filter(|rename| rename.old == *source && rename.new == expected)
                .count()
                == 1
        })
        && destinations.iter().all(|destination| {
            let expected =
                replacement(destination, new_prefix, old_prefix).expect("descendant maps");
            renames
                .iter()
                .filter(|rename| rename.old == expected && rename.new == *destination)
                .count()
                == 1
        })
        && !old_entries.is_empty()
        && old_entries.iter().all(|old_entry| {
            let path =
                replacement(&old_entry.path, old_prefix, new_prefix).expect("affected entry maps");
            new_entries.iter().any(|new_entry| {
                new_entry.path == path && same_address_except_path(old_entry, new_entry)
            })
        })
        && new_entries.iter().all(|new_entry| {
            let path =
                replacement(&new_entry.path, new_prefix, old_prefix).expect("affected entry maps");
            old_entries.iter().any(|old_entry| {
                old_entry.path == path && same_address_except_path(old_entry, new_entry)
            })
        })
        && !old_config.is_empty()
        && old_config.iter().all(|path| {
            replacement(path, old_prefix, new_prefix)
                .is_some_and(|mapped| new.config_paths.contains(&mapped))
        })
        && new_config.iter().all(|path| {
            replacement(path, new_prefix, old_prefix)
                .is_some_and(|mapped| old.config_paths.contains(&mapped))
        })
}

fn replacement(path: &str, old_prefix: &str, new_prefix: &str) -> Option<String> {
    path.strip_prefix(old_prefix).and_then(|rest| {
        (rest.is_empty() || rest.starts_with('/')).then(|| format!("{new_prefix}{rest}"))
    })
}

fn refuse_ambiguous_blob_groups(
    repository: &Repository<'_>,
    old: &Snapshot,
    new: &Snapshot,
) -> Result<(), HistoryError> {
    let old_identity_paths: BTreeSet<&str> = old
        .entries
        .iter()
        .map(|entry| entry.address.path.as_str())
        .chain(
            old.findings
                .iter()
                .map(|finding| finding.address.path.as_str()),
        )
        .collect();
    let new_identity_paths: BTreeSet<&str> = new
        .entries
        .iter()
        .map(|entry| entry.address.path.as_str())
        .chain(
            new.findings
                .iter()
                .map(|finding| finding.address.path.as_str()),
        )
        .collect();
    let mut sources_by_blob: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for path in old_identity_paths {
        if !new.blobs.contains_key(path)
            && let Some(blob) = old.blobs.get(path)
        {
            sources_by_blob.entry(blob).or_default().insert(path);
        }
    }
    let mut destinations_by_blob: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for path in new_identity_paths {
        if !old.blobs.contains_key(path)
            && let Some(blob) = new.blobs.get(path)
        {
            destinations_by_blob.entry(blob).or_default().insert(path);
        }
    }
    for (blob, sources) in sources_by_blob {
        if let Some(destinations) = destinations_by_blob.get(blob)
            && (sources.len() > 1 || destinations.len() > 1)
        {
            let destination = destinations.iter().next().expect("group has a destination");
            return Err(repository.error_at(
                &new.sha,
                format!("ambiguous rename evidence for {destination}"),
            ));
        }
    }
    Ok(())
}

fn classify(
    snapshots: &[Snapshot],
    transitions: &Transitions,
    from_index: usize,
    range: &str,
) -> Result<History, HistoryError> {
    let first_seen_entries = entry_ages(snapshots, transitions);
    let first_seen_findings = finding_ages(snapshots, transitions);
    let from = &snapshots[from_index];
    let to = snapshots.last().expect("resolved history has one commit");
    let mut matched_to = BTreeSet::new();
    let mut added = Vec::new();
    let mut retired = Vec::new();
    let mut raised = Vec::new();
    let mut lowered = Vec::new();
    let mut renamed = Vec::new();

    for old in &from.entries {
        let mappings = mappings_at_to(snapshots, transitions, from_index, &old.address);
        let mut candidates = mappings.iter().filter_map(|(mapped, movement)| {
            to.entries
                .iter()
                .find(|entry| entry.address == *mapped)
                .map(|entry| (entry, *movement))
        });
        let Some((new, movement)) = candidates.next() else {
            retired.push(Ceiling {
                address: old.address.clone(),
                value: old.value,
            });
            continue;
        };
        if candidates.next().is_some() {
            return Err(HistoryError::new(
                range,
                format!("ambiguous rename evidence for {}", old.address.path),
            ));
        }
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

fn mappings_at_to(
    snapshots: &[Snapshot],
    transitions: &Transitions,
    from_index: usize,
    address: &Address,
) -> BTreeMap<Address, Option<MovementKind>> {
    let from = &snapshots[from_index];
    let mut by_commit = BTreeMap::new();
    by_commit.insert(from.sha.as_str(), BTreeMap::from([(address.clone(), None)]));
    for current in snapshots.iter().skip(from_index + 1) {
        let mut current_mappings = BTreeMap::new();
        for parent in &current.parents {
            let Some(parent_mappings) = by_commit.get(parent.as_str()) else {
                continue;
            };
            let Some(transition) = transitions.get(&(parent.clone(), current.sha.clone())) else {
                continue;
            };
            for (previous, inherited_kind) in parent_mappings {
                let (mapped, moved) = transition.map_address(previous);
                let kind = movement(*inherited_kind, moved);
                current_mappings
                    .entry(mapped)
                    .and_modify(|existing| *existing = movement(*existing, kind))
                    .or_insert(kind);
            }
        }
        by_commit.insert(current.sha.as_str(), current_mappings);
    }
    by_commit
        .remove(
            snapshots
                .last()
                .expect("history has an endpoint")
                .sha
                .as_str(),
        )
        .unwrap_or_default()
}

fn movement(
    inherited: Option<MovementKind>,
    current: Option<MovementKind>,
) -> Option<MovementKind> {
    if inherited == Some(MovementKind::Directory) || current == Some(MovementKind::Directory) {
        Some(MovementKind::Directory)
    } else {
        current.or(inherited)
    }
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

fn entry_ages(snapshots: &[Snapshot], transitions: &Transitions) -> BTreeMap<Address, FirstSeen> {
    let by_sha: BTreeMap<&str, &Snapshot> = snapshots
        .iter()
        .map(|snapshot| (snapshot.sha.as_str(), snapshot))
        .collect();
    let mut by_commit = BTreeMap::new();
    for current in snapshots {
        let mut next = BTreeMap::new();
        for entry in current
            .entries
            .iter()
            .filter(|entry| entry.kind == crate::exceptions::Kind::Deferred)
        {
            let inherited: Option<Vec<FirstSeen>> = current
                .parents
                .iter()
                .map(|parent| {
                    let previous = by_sha.get(parent.as_str())?;
                    let previous_ages: &BTreeMap<Address, FirstSeen> =
                        by_commit.get(parent.as_str())?;
                    let transition = transitions.get(&(parent.clone(), current.sha.clone()))?;
                    previous.entries.iter().find_map(|old| {
                        (old.kind == crate::exceptions::Kind::Deferred
                            && transition.map_address(&old.address).0 == entry.address)
                            .then(|| previous_ages.get(&old.address).cloned())
                            .flatten()
                    })
                })
                .collect();
            next.insert(
                entry.address.clone(),
                inherited
                    .and_then(|ages| continuous_first_seen(&ages, &by_sha))
                    .unwrap_or_else(|| FirstSeen::at(current)),
            );
        }
        by_commit.insert(current.sha.as_str(), next);
    }
    by_commit
        .remove(
            snapshots
                .last()
                .expect("history has an endpoint")
                .sha
                .as_str(),
        )
        .unwrap_or_default()
}

fn finding_ages(
    snapshots: &[Snapshot],
    transitions: &Transitions,
) -> BTreeMap<FindingAddress, FirstSeen> {
    let by_sha: BTreeMap<&str, &Snapshot> = snapshots
        .iter()
        .map(|snapshot| (snapshot.sha.as_str(), snapshot))
        .collect();
    let mut by_commit = BTreeMap::new();
    for current in snapshots {
        let mut next = BTreeMap::new();
        for finding in &current.findings {
            let inherited: Option<Vec<FirstSeen>> = current
                .parents
                .iter()
                .map(|parent| {
                    let previous = by_sha.get(parent.as_str())?;
                    let previous_ages: &BTreeMap<FindingAddress, FirstSeen> =
                        by_commit.get(parent.as_str())?;
                    let transition = transitions.get(&(parent.clone(), current.sha.clone()))?;
                    previous.findings.iter().find_map(|old| {
                        (transition.map_finding(&old.address) == finding.address)
                            .then(|| previous_ages.get(&old.address).cloned())
                            .flatten()
                    })
                })
                .collect();
            next.insert(
                finding.address.clone(),
                inherited
                    .and_then(|ages| continuous_first_seen(&ages, &by_sha))
                    .unwrap_or_else(|| FirstSeen::at(current)),
            );
        }
        by_commit.insert(current.sha.as_str(), next);
    }
    by_commit
        .remove(
            snapshots
                .last()
                .expect("history has an endpoint")
                .sha
                .as_str(),
        )
        .unwrap_or_default()
}

fn continuous_first_seen(
    ages: &[FirstSeen],
    snapshots: &BTreeMap<&str, &Snapshot>,
) -> Option<FirstSeen> {
    ages.iter()
        .find(|candidate| {
            ages.iter()
                .all(|other| is_ancestor(&other.sha, &candidate.sha, snapshots))
        })
        .cloned()
}

fn is_ancestor(ancestor: &str, descendant: &str, snapshots: &BTreeMap<&str, &Snapshot>) -> bool {
    let mut pending = vec![descendant.to_owned()];
    let mut visited = BTreeSet::new();
    while let Some(sha) = pending.pop() {
        if sha == ancestor {
            return true;
        }
        if !visited.insert(sha.clone()) {
            continue;
        }
        if let Some(snapshot) = snapshots.get(sha.as_str()) {
            pending.extend(snapshot.parents.iter().cloned());
        }
    }
    false
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
