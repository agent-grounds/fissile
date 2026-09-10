//! Stable records and renderers for the audit history section
//! (§FS-004-check-audit.2.1).

use crate::Unit;
use crate::exceptions::{Exception, Kind, MatchKind};
use crate::json::Json;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Address {
    pub severity: String,
    pub registry: String,
    pub path: String,
    pub match_kind: String,
    pub unit: String,
    pub rules: Vec<String>,
}

impl Address {
    pub fn from_exception(entry: &Exception) -> Self {
        Self {
            severity: entry.severity.as_str().to_owned(),
            registry: entry.registry.clone(),
            path: entry.path.clone(),
            match_kind: match entry.match_kind {
                MatchKind::Exact => "exact",
                MatchKind::Glob => "glob",
            }
            .to_owned(),
            unit: entry.max_unit.to_string(),
            rules: entry.rules.clone(),
        }
    }

    pub fn text(&self) -> String {
        format!(
            "{} {}: {} [match={}; rules={}; unit={}]",
            self.severity,
            self.registry,
            self.path,
            self.match_kind,
            self.rules.join(","),
            self.unit
        )
    }

    pub fn json_fields(&self) -> Vec<(&'static str, Json)> {
        vec![
            ("registry", Json::str(self.registry.clone())),
            ("severity", Json::str(self.severity.clone())),
            ("path", Json::str(self.path.clone())),
            ("match", Json::str(self.match_kind.clone())),
            (
                "rules",
                Json::Array(self.rules.iter().cloned().map(Json::str).collect()),
            ),
            ("unit", Json::str(self.unit.clone())),
        ]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EntryState {
    pub address: Address,
    pub value: u64,
    pub kind: Kind,
    /// Non-identity evidence used only to disambiguate several byte-identical
    /// rename candidates. Changing it never resets continuity.
    pub rename_hint: String,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FindingAddress {
    pub path: String,
    pub rule: String,
    pub unit: String,
}

#[derive(Clone, Debug)]
pub(crate) struct FindingState {
    pub address: FindingAddress,
    pub actual: u64,
    pub limit: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub sha: String,
    pub timestamp: i64,
    pub date: String,
    pub entries: Vec<EntryState>,
    pub findings: Vec<FindingState>,
    pub blobs: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Ceiling {
    pub address: Address,
    pub value: u64,
}

impl Ceiling {
    fn json(&self) -> Json {
        let mut fields = self.address.json_fields();
        fields.push(("value", Json::UInt(self.value)));
        Json::Object(fields)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CeilingChange {
    pub address: Address,
    pub old_value: u64,
    pub new_value: u64,
}

impl CeilingChange {
    fn json(&self) -> Json {
        let mut fields = self.address.json_fields();
        fields.push(("old_value", Json::UInt(self.old_value)));
        fields.push(("new_value", Json::UInt(self.new_value)));
        Json::Object(fields)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MovementKind {
    ExactFile,
    Directory,
}

impl MovementKind {
    pub fn name(self) -> &'static str {
        match self {
            MovementKind::ExactFile => "exact-file",
            MovementKind::Directory => "directory",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Renamed {
    pub kind: MovementKind,
    pub old: Address,
    pub new: Address,
}

impl Renamed {
    fn json(&self) -> Json {
        Json::Object(vec![
            ("kind", Json::str(self.kind.name())),
            ("old", Json::Object(self.old.json_fields())),
            ("new", Json::Object(self.new.json_fields())),
        ])
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DeferredAge {
    pub entry: Ceiling,
    pub first_seen_commit: String,
    pub first_seen_date: String,
    pub age_days: u64,
}

impl DeferredAge {
    fn json(&self) -> Json {
        let mut fields = self.entry.address.json_fields();
        fields.push(("value", Json::UInt(self.entry.value)));
        fields.extend(age_fields(self));
        Json::Object(fields)
    }
}

fn age_fields(age: &DeferredAge) -> Vec<(&'static str, Json)> {
    vec![
        (
            "first_seen_commit",
            Json::str(age.first_seen_commit.clone()),
        ),
        ("first_seen_date", Json::str(age.first_seen_date.clone())),
        ("age_days", Json::UInt(age.age_days)),
    ]
}

#[derive(Clone, Debug)]
pub(crate) struct FindingAge {
    pub finding: FindingState,
    pub first_seen_commit: String,
    pub first_seen_date: String,
    pub age_days: u64,
}

impl FindingAge {
    fn json(&self) -> Json {
        Json::Object(vec![
            ("severity", Json::str("soft")),
            ("path", Json::str(self.finding.address.path.clone())),
            ("rule", Json::str(self.finding.address.rule.clone())),
            ("unit", Json::str(self.finding.address.unit.clone())),
            ("actual", Json::UInt(self.finding.actual)),
            ("limit", Json::UInt(self.finding.limit)),
            (
                "first_seen_commit",
                Json::str(self.first_seen_commit.clone()),
            ),
            ("first_seen_date", Json::str(self.first_seen_date.clone())),
            ("age_days", Json::UInt(self.age_days)),
        ])
    }
}

#[derive(Clone, Debug)]
pub(crate) struct History {
    pub from: String,
    pub to: String,
    pub added: Vec<Ceiling>,
    pub retired: Vec<Ceiling>,
    pub raised: Vec<CeilingChange>,
    pub lowered: Vec<CeilingChange>,
    pub renamed: Vec<Renamed>,
    pub deferred_ages: Vec<DeferredAge>,
    pub soft_finding_ages: Vec<FindingAge>,
}

impl History {
    pub fn render_text(&self) -> String {
        let mut lines = vec![
            format!("history {}..{}:", self.from, self.to),
            format!(
                "  exceptions: +{} -{}; ceilings: {} raised, {} lowered; renames: {}",
                self.added.len(),
                self.retired.len(),
                self.raised.len(),
                self.lowered.len(),
                self.renamed.len()
            ),
        ];
        for item in &self.added {
            lines.push(format!("  added: {} = {}", item.address.text(), item.value));
        }
        for item in &self.retired {
            lines.push(format!(
                "  retired: {} = {}",
                item.address.text(),
                item.value
            ));
        }
        for item in &self.raised {
            lines.push(format!(
                "  raised: {} {} -> {}",
                item.address.text(),
                item.old_value,
                item.new_value
            ));
        }
        for item in &self.lowered {
            lines.push(format!(
                "  lowered: {} {} -> {}",
                item.address.text(),
                item.old_value,
                item.new_value
            ));
        }
        for item in &self.renamed {
            lines.push(format!(
                "  renamed: {} {} {}: {} -> {} [match={}; rules={}; unit={}]",
                item.kind.name(),
                item.new.severity,
                item.new.registry,
                item.old.path,
                item.new.path,
                item.new.match_kind,
                item.new.rules.join(","),
                item.new.unit
            ));
        }
        for item in &self.deferred_ages {
            lines.push(format!(
                "  deferred age: {} = {}; first {} {}; {} days",
                item.entry.address.text(),
                item.entry.value,
                item.first_seen_commit,
                item.first_seen_date,
                item.age_days
            ));
        }
        for item in &self.soft_finding_ages {
            lines.push(format!(
                "  soft finding age: {} [rule={}; unit={}] {} > {}; first {} {}; {} days",
                item.finding.address.path,
                item.finding.address.rule,
                item.finding.address.unit,
                item.finding.actual,
                item.finding.limit,
                item.first_seen_commit,
                item.first_seen_date,
                item.age_days
            ));
        }
        lines.join("\n")
    }

    pub fn json(&self) -> Json {
        Json::Object(vec![
            ("from", Json::str(self.from.clone())),
            ("to", Json::str(self.to.clone())),
            ("counts", self.counts_json()),
            (
                "added",
                Json::Array(self.added.iter().map(Ceiling::json).collect()),
            ),
            (
                "retired",
                Json::Array(self.retired.iter().map(Ceiling::json).collect()),
            ),
            (
                "raised",
                Json::Array(self.raised.iter().map(CeilingChange::json).collect()),
            ),
            (
                "lowered",
                Json::Array(self.lowered.iter().map(CeilingChange::json).collect()),
            ),
            (
                "renamed",
                Json::Array(self.renamed.iter().map(Renamed::json).collect()),
            ),
            (
                "deferred_ages",
                Json::Array(self.deferred_ages.iter().map(DeferredAge::json).collect()),
            ),
            (
                "soft_finding_ages",
                Json::Array(
                    self.soft_finding_ages
                        .iter()
                        .map(FindingAge::json)
                        .collect(),
                ),
            ),
        ])
    }

    fn counts_json(&self) -> Json {
        Json::Object(vec![
            ("added", Json::UInt(self.added.len() as u64)),
            ("retired", Json::UInt(self.retired.len() as u64)),
            ("raised", Json::UInt(self.raised.len() as u64)),
            ("lowered", Json::UInt(self.lowered.len() as u64)),
            ("renamed", Json::UInt(self.renamed.len() as u64)),
            ("deferred_ages", Json::UInt(self.deferred_ages.len() as u64)),
            (
                "soft_finding_ages",
                Json::UInt(self.soft_finding_ages.len() as u64),
            ),
        ])
    }
}

pub(crate) fn finding_address(path: &str, rule: &str, unit: Unit) -> FindingAddress {
    FindingAddress {
        path: path.to_owned(),
        rule: rule.to_owned(),
        unit: unit.to_string(),
    }
}
