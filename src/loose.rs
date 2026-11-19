//! The `loose ceilings:` section of `audit --stale-exceptions`
//! (§FS-003-exceptions.7): exact-path entries whose ceiling has stopped saying
//! something true about the file under it — it stands far above that file, or it
//! sits exactly on it — and the one remedy each carries
//! (§FS-004-check-audit.2).

use std::collections::HashMap;

use crate::audit::Measured;
use crate::cli::Loaded;
use crate::entry;
use crate::exceptions::{EntrySite, MatchKind};
use crate::json::Json;
use crate::{Severity, Unit};

/// One entry accepting far more of a file that is still there, or accepting
/// exactly what that file measures. Stale is the other half: an entry accepting
/// a file that is gone (§FS-003-exceptions.7).
pub(crate) struct Loose {
    pub site: EntrySite,
    pub severity: Severity,
    pub unit: Unit,
    pub accepted: u64,
    pub actual: u64,
    pub limit: u64,
    /// What to do about the gap: the first of the cases §FS-004-check-audit.2
    /// lists that applies (§DF-010-stated-ceilings-are-exact.2).
    pub remedy: Remedy,
    /// The file no longer crosses that limit, so the entry silences nothing and
    /// lowering it is not the remedy — removing it is.
    pub silences_nothing: bool,
    /// The ceiling equals the measurement: slack that has run out rather than
    /// slack that drifted, so the entry passes today and fails on the next
    /// unrelated commit (§FS-003-exceptions.7).
    pub no_headroom: bool,
}

/// What closes the gap between an entry's ceiling and the file under it: the
/// first of the cases §FS-004-check-audit.2 lists that applies, once the entry
/// is known to still silence something.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Remedy {
    /// The ceiling `exception retune` would write, never below the limit the
    /// entry exists to accept (§FS-004-check-audit.2 case 4).
    RetuneTo(u64),
    /// The step lands a soft ceiling on the hard limit, which `retune` refuses
    /// (§DF-010-stated-ceilings-are-exact.2), so the remedy is the stated form:
    /// the floor a ceiling has to clear, up to, excluding, that limit
    /// (§FS-004-check-audit.2 case 2).
    StateWithin { floor: u64, hard: u64 },
    /// The measured form would write the number already recorded and report that
    /// it changed nothing, so the stated form carries the value instead
    /// (§FS-004-check-audit.2 case 3).
    StateAt(u64),
    /// Case 2 with an empty range: every ceiling above the measurement is at or
    /// above the hard limit, so no soft ceiling grants headroom and only the
    /// hard registry accepts the file.
    HardRegistryOnly { hard: u64 },
}

/// Entries whose ceiling has drifted more than one bump step above their file,
/// or whose file has grown up to it exactly (§FS-003-exceptions.7). Exact paths
/// only: a glob's ceiling is a policy for a class of files, so lowering it to
/// today's largest member breaks the next one.
pub(crate) fn entries(loaded: &Loaded, files: &[Measured<'_>]) -> Vec<Loose> {
    // Indexed once: a linear scan per entry is a full pass over the repository
    // for every exception, which is the shape §GOAL-001-fast-feedback rules out.
    let by_path: HashMap<&str, &Measured<'_>> = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut loose = Vec::new();
    for exception in loaded.registries.all() {
        if exception.match_kind != MatchKind::Exact {
            continue;
        }
        let Some(file) = by_path.get(exception.path.as_str()) else {
            continue;
        };
        // The measurement to compare is the one this entry can silence: the
        // effective rule for its unit, among the rules it lists.
        let Some(hit) = file.hits.iter().find(|hit| {
            hit.rule.budget.unit == exception.max_unit && exception.applies_to_rule(&hit.rule.id)
        }) else {
            continue;
        };

        let step = loaded.config.exceptions.bump.step(exception.max_unit);
        let Some(no_headroom) = half(exception.max_value, hit.actual, step) else {
            continue;
        };
        let limit = match exception.severity {
            Severity::Soft => hit.rule.budget.soft,
            Severity::Hard => hit.rule.budget.hard,
        }
        .unwrap_or(0);
        // An entry without headroom already accepts its own measurement, so the
        // smallest ceiling that grants any is the step's next multiple strictly
        // above it (§FS-003-exceptions.7).
        let base = if no_headroom {
            hit.actual.saturating_add(1)
        } else {
            hit.actual
        };
        let quantized = entry::quantize(base, step).max(limit);
        // The value `retune` would write, unless it is one `retune` refuses
        // (§DF-010-stated-ceilings-are-exact.2). The twin is resolved the way the
        // command resolves it, so the report and the command cannot disagree.
        let refused = hit.rule.budget.hard.filter(|hard| {
            exception.severity == Severity::Soft
                && hit.actual < *hard
                && quantized >= *hard
                && !entry::has_deferred_hard_twin(
                    &loaded.registries,
                    &exception.path,
                    exception.match_kind,
                    std::slice::from_ref(&hit.rule.id),
                    exception.max_unit,
                )
        });
        // The least a stated ceiling may be and still silence something — one
        // above the measurement where the entry already accepts it.
        let floor = base.max(limit);
        loose.push(Loose {
            site: exception.site(),
            severity: exception.severity,
            unit: exception.max_unit,
            accepted: exception.max_value,
            actual: hit.actual,
            limit,
            remedy: match refused {
                Some(hard) if floor < hard => Remedy::StateWithin { floor, hard },
                // The hard limit sits directly above the measurement, so the
                // range is empty and no soft ceiling helps.
                Some(hard) => Remedy::HardRegistryOnly { hard },
                // The measurement is already a step multiple, so `retune` would
                // answer `already accepts {N}` and write nothing.
                None if no_headroom && entry::quantize(hit.actual, step) == hit.actual => {
                    Remedy::StateAt(quantized)
                }
                None => Remedy::RetuneTo(quantized),
            },
            silences_nothing: hit.actual < limit,
            no_headroom,
        });
    }
    loose
}

/// Which half of §FS-003-exceptions.7 a ceiling is — `true` for no headroom,
/// `false` for loose — or `None` for neither. Slack inside one step is the
/// quantization a fresh entry is written with (§DF-006-quantized-ceilings), and
/// a ceiling below its file has stopped silencing, which `check` reports itself.
fn half(accepted: u64, actual: u64, step: u64) -> Option<bool> {
    match accepted.checked_sub(actual) {
        Some(0) => Some(true),
        Some(slack) if slack > step => Some(false),
        _ => None,
    }
}

/// The line the text report prints for one entry, advice last
/// (§FS-004-check-audit.2).
pub(crate) fn text_line(item: &Loose) -> String {
    let Loose {
        site,
        unit,
        accepted,
        actual,
        ..
    } = item;
    let advice = if item.silences_nothing {
        // Lowering is not the remedy here, so the line names the command that is
        // (§FS-003-exceptions.7, §FS-009-exception-remove). An entry its file has
        // fallen below is finished, not short of room, so it takes no prefix.
        format!(
            "silences nothing now; the {} limit is {} — remove it with `fissile exception remove`",
            item.severity, item.limit
        )
    } else {
        let remedy = match item.remedy {
            Remedy::RetuneTo(to) => format!("retune to {to}"),
            Remedy::StateAt(max) => format!("retune with --max {max} --unit {unit}"),
            // `retune` refuses the measured form on the hard limit, so the line
            // names the one it accepts and the range that keeps it under
            // (§DF-010-stated-ceilings-are-exact.2).
            Remedy::StateWithin { floor, hard } => {
                format!("retune with --max <N> --unit {unit}, {floor} <= N < {hard}")
            }
            // A hook or CI prints this line, so it names both routes left, the hard
            // one as a person's at a terminal (§FS-004-check-audit.2,
            // §DF-008-hard-severity-needs-a-terminal.1).
            Remedy::HardRegistryOnly { hard } => format!(
                "no soft ceiling under the {hard}-{} hard limit grants any — split the file, \
                 or have a person accept it in the hard registry at a terminal with \
                 `fissile exception add --severity hard`",
                unit.singular()
            ),
        };
        if item.no_headroom {
            format!("no headroom; {remedy}")
        } else {
            remedy
        }
    };
    format!("{site} accepts {accepted} {unit}, now {actual} — {advice}")
}

/// The `loose` array of `audit --format json` (§FS-004-check-audit.2).
pub(crate) fn json(items: &[Loose]) -> Json {
    Json::Array(items.iter().map(record).collect())
}

fn record(item: &Loose) -> Json {
    // `severity` and `limit` travel with the record: the text line states both,
    // and a consumer must not have to re-derive them by matching the registry
    // filename against the config.
    Json::Object(vec![
        ("registry", Json::str(item.site.registry.clone())),
        ("path", Json::str(item.site.path.clone())),
        ("severity", Json::str(item.severity.to_string())),
        ("unit", Json::str(item.unit.to_string())),
        ("accepted", Json::UInt(item.accepted)),
        ("actual", Json::UInt(item.actual)),
        ("limit", Json::UInt(item.limit)),
        // Which half of §FS-003-exceptions.7 the record is, so a consumer reads
        // it without parsing the line. `src/json.rs` models no boolean, so it is
        // encoded the way `silences_nothing` already is.
        ("no_headroom", Json::UInt(u64::from(item.no_headroom))),
        (
            "retune_to",
            match item.remedy {
                Remedy::RetuneTo(to) => Json::UInt(to),
                _ => Json::Null,
            },
        ),
        // The remedy a `null` `retune_to` stands for, so a consumer has the
        // ceiling or the range the text line prints rather than an absence
        // (§FS-004-check-audit.2). The empty range has neither.
        (
            "stated_range",
            match item.remedy {
                Remedy::StateWithin { floor, hard } => Json::Object(vec![
                    ("min", Json::UInt(floor)),
                    ("max_excluded", Json::UInt(hard)),
                ]),
                Remedy::StateAt(max) => Json::Object(vec![("min", Json::UInt(max))]),
                _ => Json::Null,
            },
        ),
        (
            "silences_nothing",
            Json::UInt(u64::from(item.silences_nothing)),
        ),
    ])
}
