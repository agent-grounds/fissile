//! Evaluation of one committed repository snapshot using the ordinary fissile
//! config, scan, checker, measurement, and exception paths.

use std::path::Path;

use crate::cli;
use crate::exceptions::{Kind, Verdict};
use crate::{Severity, scan};

use super::data::{Address, EntryState, FindingState, Snapshot, finding_address};
use super::git::Repository;
use super::{HistoryError, Revision};

pub(crate) fn snapshot(
    repository: &Repository<'_>,
    revision: &Revision,
    config_path: Option<&Path>,
) -> Result<Snapshot, HistoryError> {
    let materialized = repository.materialize(&revision.sha)?;
    let historical_config = config_path.map(Path::to_path_buf);
    let loaded = cli::load(&materialized.root, historical_config.as_deref())
        .map_err(|error| at(repository, revision, error.to_string()))?;
    let files = scan::walk_scope(&loaded.root, &loaded.config.scan)
        .map_err(|error| at(repository, revision, error.to_string()))?;

    let mut findings = Vec::new();
    for path in &files {
        let measured = scan::measure_file_with_context(&loaded.root, path, &loaded.config.tokens)
            .map_err(|error| {
            let cause = if loaded.config.tokens.enabled {
                "token evaluation unavailable".to_owned()
            } else {
                format!("measurement unavailable for {path}: {error}")
            };
            at(repository, revision, cause)
        })?;
        let hits = loaded
            .checker
            .evaluate(&measured.measurement)
            .map_err(|error| at(repository, revision, error.to_string()))?;
        for hit in hits {
            let Some(limit) = hit.rule.budget.soft.filter(|limit| hit.actual > *limit) else {
                continue;
            };
            if structural_hard_silences(&loaded.registries, path, &hit)
                .map_err(|error| at(repository, revision, error))?
            {
                continue;
            }
            let verdict = loaded
                .registries
                .verdict(
                    Severity::Soft,
                    path,
                    &hit.rule.id,
                    hit.rule.budget.unit,
                    hit.actual,
                )
                .map_err(|error| at(repository, revision, error.to_string()))?;
            if matches!(verdict, Verdict::Silenced(_)) {
                continue;
            }
            findings.push(FindingState {
                address: finding_address(path, &hit.rule.id, hit.rule.budget.unit),
                actual: hit.actual,
                limit,
            });
        }
    }
    findings.sort_by(|left, right| left.address.cmp(&right.address));

    let mut entries: Vec<EntryState> = loaded
        .registries
        .all()
        .map(|entry| EntryState {
            address: Address::from_exception(entry),
            value: entry.max_value,
            kind: entry.kind,
        })
        .collect();
    entries.sort_by(|left, right| left.address.cmp(&right.address));
    let config_paths = loaded
        .config
        .scan
        .include
        .iter()
        .chain(&loaded.config.scan.exclude)
        .chain(
            loaded
                .config
                .rules
                .iter()
                .flat_map(|rule| rule.include.iter().chain(&rule.exclude)),
        )
        .cloned()
        .collect();
    Ok(Snapshot {
        sha: revision.sha.clone(),
        parents: revision.parents.clone(),
        timestamp: revision.timestamp,
        date: super::utc_date(revision.timestamp),
        entries,
        findings,
        blobs: materialized.blobs.clone(),
        config_paths,
    })
}

fn structural_hard_silences(
    registries: &crate::exceptions::Registries,
    path: &str,
    hit: &crate::RuleHit<'_>,
) -> Result<bool, String> {
    let Some(hard) = hit.rule.budget.hard.filter(|hard| hit.actual > *hard) else {
        return Ok(false);
    };
    let _ = hard;
    let verdict = registries
        .verdict(
            Severity::Hard,
            path,
            &hit.rule.id,
            hit.rule.budget.unit,
            hit.actual,
        )
        .map_err(|error| error.to_string())?;
    Ok(matches!(verdict, Verdict::Silenced(entry) if entry.kind == Kind::Structural))
}

fn at(repository: &Repository<'_>, revision: &Revision, cause: impl Into<String>) -> HistoryError {
    repository.error_at(&revision.sha, cause)
}
