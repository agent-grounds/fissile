//! `fissile audit` (§FS-004-check-audit.2): the whole-repo inventory and
//! migration surface. Beyond current overflows it can report the largest files
//! per unit, stale exceptions, and rule coverage gaps.

use std::path::PathBuf;

use crate::cli::{self, CommandError, Format, Loaded};
use crate::exceptions::{EntrySite, Exception, KindCounts, KindPathCounts};
use crate::history::History;
use crate::json::Json;
use crate::loose::{self, Loose};
use crate::report::{self, EvalError, Outcome};
use crate::{FileMeasurement, RuleHit, Selector, Unit, scan};

/// Inputs to an `audit` run.
#[derive(Clone, Debug, Default)]
pub struct AuditOptions {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub format: Option<Format>,
    /// Force plain text output regardless of `[output].color` (§FS-001-config.6).
    pub no_color: bool,
    pub top: Option<usize>,
    pub stale_exceptions: bool,
    pub rule_coverage: bool,
    /// An ancestral Git range whose committed debt history is appended to the
    /// audit (§FS-004-check-audit.2.1).
    pub history: Option<String>,
    /// The sections `--only` named, or `None` for the whole report
    /// (§FS-004-check-audit.2). A set: the order they were named in and a name
    /// repeated do not reach the output, which renders in canonical order.
    pub only: Option<Vec<Section>>,
}

impl AuditOptions {
    /// Add the sections one `--only` value names, or report the first name that
    /// is not one (§FS-004-check-audit.2). A second `--only` adds to the
    /// selection rather than replacing it, and a repeated name selects its
    /// section once.
    pub fn select(&mut self, raw: &str) -> Result<(), String> {
        // Parsed whole before anything is added, so a bad name leaves the
        // selection as it was.
        let named = raw
            .split(',')
            .map(Section::parse)
            .collect::<Result<Vec<_>, _>>()?;
        let selected = self.only.get_or_insert_default();
        for section in named {
            if !selected.contains(&section) {
                selected.push(section);
            }
        }
        Ok(())
    }

    /// Whether `--only` named this section. Naming a section is the request to
    /// compute it, so the flag that would otherwise ask for it is not also
    /// required (§FS-004-check-audit.2).
    pub fn selects(&self, section: Section) -> bool {
        self.only
            .as_ref()
            .is_some_and(|only| only.contains(&section))
    }

    /// Whether the text report prints this section: every section, until
    /// `--only` narrows it (§FS-004-check-audit.2).
    fn prints(&self, section: Section) -> bool {
        self.only
            .as_ref()
            .is_none_or(|only| only.contains(&section))
    }
}

/// A section of the audit report, addressable by name through `--only`
/// (§FS-004-check-audit.2). The names and their order are the top-level keys of
/// `schema/audit.schema.json`, so the text surface and the JSON surface cannot
/// grow two vocabularies for one report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Findings,
    Silenced,
    Exceptions,
    Top,
    Stale,
    Loose,
    Coverage,
    History,
}

/// Every section in the canonical order `schema/audit.schema.json` declares
/// them, which is the order a report renders in whatever order `--only` named
/// them (§FS-004-check-audit.2).
pub const SECTIONS: [Section; 8] = [
    Section::Findings,
    Section::Silenced,
    Section::Exceptions,
    Section::Top,
    Section::Stale,
    Section::Loose,
    Section::Coverage,
    Section::History,
];

impl Section {
    /// The name `--only` takes for this section, which is the key
    /// `--format json` publishes it under (§FS-004-check-audit.2).
    pub fn name(self) -> &'static str {
        match self {
            Section::Findings => "findings",
            Section::Silenced => "silenced",
            Section::Exceptions => "exceptions",
            Section::Top => "top",
            Section::Stale => "stale",
            Section::Loose => "loose",
            Section::Coverage => "coverage",
            Section::History => "history",
        }
    }

    /// The section a `--only` name selects. An unknown or empty name is an
    /// error naming it and the whole valid set in canonical order: a silent
    /// empty report reads exactly like a section that had nothing to say
    /// (§FS-004-check-audit.2).
    pub fn parse(name: &str) -> Result<Self, String> {
        SECTIONS
            .into_iter()
            .find(|section| section.name() == name)
            .ok_or_else(|| {
                let bad = if name.is_empty() {
                    "empty --only section name".to_owned()
                } else {
                    format!("unknown --only section `{name}`")
                };
                let valid: Vec<&str> = SECTIONS.iter().map(|section| section.name()).collect();
                format!("{bad} (valid: {})", valid.join(", "))
            })
    }
}

pub struct Run {
    pub output: String,
    pub failed: bool,
    /// What stderr owes beside the findings: today, the deprecated config home
    /// this run was governed by (§FS-001-config.8.2).
    pub notes: Vec<String>,
    /// File-level measurement errors; force exit 2 (§FS-004-check-audit.5).
    pub errors: Vec<String>,
}

const UNITS: [Unit; 3] = [Unit::Bytes, Unit::Lines, Unit::Tokens];

/// The largest files per unit: one ranked `(value, path)` list per measurement
/// unit (§FS-004-check-audit.2).
type TopFiles = Vec<(Unit, Vec<(u64, String)>)>;

/// What `audit` reports beyond the standing findings (§FS-004-check-audit.2).
/// A `None` section was not requested; `kinds` is default-on and carries its own
/// emptiness.
struct Inventory {
    top: Option<TopFiles>,
    stale: Option<Vec<EntrySite>>,
    /// Ceilings standing more than one bump step above the file they accept, or
    /// sitting exactly on it (§FS-003-exceptions.7). Requested by the same flag
    /// as `stale`: both answer "which entries no longer say something true?".
    loose: Option<Vec<Loose>>,
    coverage: Option<Coverage>,
    kinds: KindCounts,
    kind_paths: KindPathCounts,
}

/// Runs the audit and renders the report in the format the options resolve to.
///
/// The selection is refused against JSON here as well as at the CLI, because
/// `findings`, `silenced` and `exceptions` are `required` in
/// `schema/audit.schema.json`: a filtered object would not validate against the
/// contract it claims to satisfy (§FS-004-check-audit.2). The CLI refuses the
/// flag pair with its usage screen; this refuses the same thing for a library
/// caller, and for a config whose `[output].format` is what makes the run JSON.
/// Accepting a selection and ignoring it is the one outcome the caller cannot
/// tell apart from a section that had nothing in it.
pub fn run(options: &AuditOptions) -> Result<Run, CommandError> {
    let loaded = cli::load(&options.root, options.config_path.as_deref())?;
    let format = options
        .format
        .unwrap_or_else(|| loaded.config.output.format.into());
    // A selection is a text-report request, whatever made the run JSON
    // (§FS-004-check-audit.2).
    if options.only.is_some() && format == Format::Json {
        return Err(CommandError::Usage(
            "--only selects sections of the text report and is not valid with --format json"
                .to_owned(),
        ));
    }
    if options.selects(Section::History) && options.history.is_none() {
        return Err(CommandError::Usage(
            "--only history requires --history <from>..<to>".to_owned(),
        ));
    }
    let files = scan::walk_scope(&loaded.root, &loaded.config.scan)?;

    let mut measured_files = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for rel in &files {
        // Skip what cannot be measured; the walk goes on (§FS-004-check-audit.5).
        match scan::measure_file_with_context(&loaded.root, rel, &loaded.config.tokens) {
            Ok(measured_file) => measured_files.push(measured_file),
            Err(error) => errors.push(scan::measure_error_line(rel, &error)),
        }
    }

    // The hits are read three times — findings, `--top`, loose ceilings — so the
    // walk evaluates each file once and the sections share the result.
    let mut hits = Vec::with_capacity(measured_files.len());
    let mut contexts = Vec::new();
    for measured_file in &measured_files {
        let measurement = &measured_file.measurement;
        let file_hits = loaded
            .checker
            .evaluate(measurement)
            .map_err(EvalError::from)?;
        contexts.extend(report::contexts_for_file(
            measurement,
            &file_hits,
            measured_file.utf8,
            &loaded.config.exceptions.bump,
        ));
        hits.push(file_hits);
    }

    let mut outcomes = Vec::new();
    for (measured_file, hits) in measured_files.iter().zip(&hits) {
        outcomes.extend(report::evaluate_hits(
            &loaded.registries,
            &measured_file.measurement,
            hits,
        )?);
    }

    let mut failed = report::has_hard_failure(&outcomes);

    // Naming a section is the request to compute it, so the flag that would
    // otherwise ask for it is not also required (§FS-004-check-audit.2).
    let registry_pass = options.stale_exceptions || options.selects(Section::Stale);
    let loose_pass = options.stale_exceptions || options.selects(Section::Loose);

    let stale = registry_pass.then(|| {
        // Registry plus the entry's own `path`: the list spans both registries,
        // and the same path can be stale in each (§FS-003-exceptions.4).
        let entries: Vec<EntrySite> = loaded
            .registries
            .stale(&files)
            .iter()
            .map(|entry| Exception::site(entry))
            .collect();
        if !entries.is_empty() && loaded.config.exceptions.stale.fails() {
            failed = true;
        }
        entries
    });
    // `ignore` suppresses the report entirely (§FS-003-exceptions.4).
    let stale = stale.filter(|_| loaded.config.exceptions.stale.reports());

    // Path and hits paired once, for the two sections that read them by file.
    let files: Vec<Measured<'_>> = measured_files
        .iter()
        .zip(&hits)
        .map(|(measurement, hits)| Measured {
            path: repo_path(&measurement.measurement),
            measurement: &measurement.measurement,
            hits,
        })
        .collect();

    let inventory = Inventory {
        top: options.top.map(|n| top_files(&files, n)),
        stale,
        // Unfiltered by `[exceptions].stale`, which governs stale entries only: a
        // loose ceiling accepts everything it accepted yesterday and breaks
        // nothing today (§FS-003-exceptions.7).
        loose: loose_pass.then(|| loose::entries(&loaded, &files)),
        coverage: (options.rule_coverage || options.selects(Section::Coverage))
            .then(|| coverage(&loaded, &measured_files)),
        // Default-on, no flag: the two numbers are what an inventory is for
        // (§FS-004-check-audit.2).
        kinds: loaded.registries.kind_counts(),
        kind_paths: loaded.registries.kind_path_counts(),
    };

    // History is deliberately computed only behind its explicit option. The
    // ordinary audit path above remains the current working-tree inventory
    // (§FS-004-check-audit.2.1).
    let history = options
        .history
        .as_deref()
        .map(|range| crate::history::evaluate(&loaded.root, range, options.config_path.as_deref()))
        .transpose()?;

    let output = match format {
        Format::Text => {
            let color = cli::use_color(loaded.config.output.color, options.no_color, format);
            render_text(TextReport {
                options,
                loaded: &loaded,
                outcomes: &outcomes,
                contexts: &contexts,
                inventory: &inventory,
                history: history.as_ref(),
                color,
                errors: &errors,
            })
        }
        Format::Json => render_json(&outcomes, &contexts, &inventory, history.as_ref()),
    };
    Ok(Run {
        output,
        failed,
        notes: cli::config_notes(&loaded.source),
        errors,
    })
}

/// The largest `n` measured files per unit (§FS-004-check-audit.2). A rule's own
/// value where one measures the file, so `--top` and a finding never disagree;
/// the default policy elsewhere, so an unruled file still ranks.
fn top_files(files: &[Measured<'_>], n: usize) -> TopFiles {
    UNITS
        .iter()
        .filter_map(|&unit| {
            let mut ranked: Vec<(u64, String)> = files
                .iter()
                .filter_map(|file| Some((file.value(unit)?, file.path.clone())))
                .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            ranked.truncate(n);
            (!ranked.is_empty()).then_some((unit, ranked))
        })
        .collect()
}

/// One scanned file with its repo-relative path and its rule hits, paired once
/// so the sections that read them do not each re-derive the path or re-scan the
/// list (§GOAL-001-fast-feedback).
pub(crate) struct Measured<'a> {
    pub path: String,
    measurement: &'a FileMeasurement,
    pub hits: &'a [RuleHit<'a>],
}

impl Measured<'_> {
    /// What this file counts in `unit`: the effective rule's value where one
    /// measures it there, and fissile's default arithmetic otherwise
    /// (§FS-001-config.3.1).
    fn value(&self, unit: Unit) -> Option<u64> {
        if let Some(hit) = self.hits.iter().find(|hit| hit.rule.budget.unit == unit) {
            return Some(hit.actual);
        }
        match unit {
            Unit::Bytes => Some(self.measurement.bytes),
            Unit::Lines => self
                .measurement
                .lines
                .map(|stats| stats.counted(false, true)),
            Unit::Tokens => self.measurement.tokens,
        }
    }
}

fn repo_path(measurement: &FileMeasurement) -> String {
    measurement.path.to_string_lossy().replace('\\', "/")
}

struct Coverage {
    unmatched_rules: Vec<String>,
    catch_all_only: Vec<String>,
    unused_messages: Vec<String>,
}

fn is_catch_all(selector: &Selector) -> bool {
    match selector {
        Selector::All => true,
        Selector::Glob(globs) => globs
            .iter()
            .all(|glob| matches!(glob.pattern(), "**/*" | "**")),
        _ => false,
    }
}

/// Rules matching no file, files reachable only through catch-all rules, and
/// messages no rule uses (§FS-004-check-audit.2).
fn coverage(loaded: &Loaded, measured_files: &[scan::MeasuredFile]) -> Coverage {
    let rules: Vec<_> = loaded.checker.scoped_rules().collect();

    let unmatched_rules = rules
        .iter()
        .filter(|scope| {
            !measured_files
                .iter()
                .any(|file| scope.applies_to(&file.measurement.path))
        })
        .map(|scope| scope.rule.id.clone())
        .collect();

    let catch_all_only = measured_files
        .iter()
        .filter(|file| {
            let measurement = &file.measurement;
            let matching: Vec<_> = rules
                .iter()
                .filter(|scope| scope.applies_to(&measurement.path))
                .collect();
            !matching.is_empty()
                && matching
                    .iter()
                    .all(|scope| is_catch_all(&scope.rule.selector))
        })
        .map(|file| file.measurement.path.to_string_lossy().replace('\\', "/"))
        .collect();

    let unused_messages = loaded
        .config
        .messages
        .iter()
        .filter(|message| {
            !loaded
                .config
                .rules
                .iter()
                .flat_map(|rule| rule.message_ids())
                .any(|id| id == message.id)
        })
        .map(|message| message.id.clone())
        .collect();

    Coverage {
        unmatched_rules,
        catch_all_only,
        unused_messages,
    }
}

/// Sections render in canonical order and only where the selection prints them
/// (§FS-004-check-audit.2). A section can be computed and left unprinted:
/// `--only` governs the screen, while exit status is computed from the whole
/// run.
struct TextReport<'a> {
    options: &'a AuditOptions,
    loaded: &'a Loaded,
    outcomes: &'a [Outcome],
    contexts: &'a [report::FindingContext],
    inventory: &'a Inventory,
    history: Option<&'a History>,
    color: bool,
    errors: &'a [String],
}

fn render_text(report: TextReport<'_>) -> String {
    let TextReport {
        options,
        loaded,
        outcomes,
        contexts,
        inventory,
        history,
        color,
        errors,
    } = report;
    let mut sections = Vec::new();

    if options.prints(Section::Findings) {
        // Each grouped block is its own section, so the blank-line separation is
        // the same between blocks as between audit sections
        // (§FS-004-check-audit.1).
        let reported = report::finding_blocks_with_context(outcomes, color, contexts);
        if reported.is_empty() {
            // The marker is the empty findings section's own output, so it
            // follows the selection (§FS-004-check-audit.2), and is withheld
            // when a file could not be measured (§FS-004-check-audit.5).
            if errors.is_empty() {
                sections.push(report::success_marker(&loaded.config.output.success, color));
            }
        } else {
            sections.extend(reported);
        }
    }

    if options.prints(Section::Silenced) {
        let silenced: Vec<String> = outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Outcome::Silenced {
                    overflow,
                    exception_max,
                } => Some(report::silenced_line(overflow, *exception_max)),
                Outcome::Reported(_) => None,
            })
            .collect();
        if !silenced.is_empty() {
            sections.push(silenced.join("\n"));
        }
    }

    // Omitted when there is nothing to inventory: a repository with no
    // exceptions pays no lines for the section (§FS-004-check-audit.2).
    let kinds = inventory.kinds;
    if options.prints(Section::Exceptions) && !kinds.is_empty() {
        let paths = inventory.kind_paths;
        sections.push(format!(
            "exceptions:\n  structural (never expires): {} entries across {} paths\n  deferred (carrying debt): {} entries across {} paths",
            kinds.structural,
            paths.structural,
            kinds.deferred,
            paths.deferred
        ));
    }

    if let Some(top) = printed(&inventory.top, Section::Top, options) {
        for (unit, ranked) in top {
            let mut lines = vec![format!("top {unit}:")];
            for (value, path) in ranked {
                lines.push(format!("  {value} {path}"));
            }
            sections.push(lines.join("\n"));
        }
    }

    if let Some(stale) = printed(&inventory.stale, Section::Stale, options) {
        let mut lines = vec!["stale exceptions:".to_owned()];
        if stale.is_empty() {
            lines.push("  none".to_owned());
        }
        for site in stale {
            lines.push(format!("  {site}"));
        }
        sections.push(lines.join("\n"));
    }

    if let Some(loose) = printed(&inventory.loose, Section::Loose, options) {
        let mut lines = vec!["loose ceilings:".to_owned()];
        if loose.is_empty() {
            lines.push("  none".to_owned());
        }
        for item in loose {
            lines.push(format!("  {}", loose::text_line(item)));
        }
        sections.push(lines.join("\n"));
    }

    if let Some(coverage) = printed(&inventory.coverage, Section::Coverage, options) {
        sections.push(render_coverage_text(coverage));
    }

    if options.prints(Section::History)
        && let Some(history) = history
    {
        sections.push(history.render_text());
    }

    sections.join("\n\n")
}

/// A computed section, when the selection prints it (§FS-004-check-audit.2).
fn printed<'a, T>(section: &'a Option<T>, name: Section, options: &AuditOptions) -> Option<&'a T> {
    section.as_ref().filter(|_| options.prints(name))
}

fn render_coverage_text(coverage: &Coverage) -> String {
    let mut lines = vec!["rule coverage:".to_owned()];
    lines.push(format!(
        "  rules matching no file: {}",
        join_or_none(&coverage.unmatched_rules)
    ));
    lines.push(format!(
        "  files only under catch-all: {}",
        join_or_none(&coverage.catch_all_only)
    ));
    lines.push(format!(
        "  unused messages: {}",
        join_or_none(&coverage.unused_messages)
    ));
    lines.join("\n")
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

fn render_json(
    outcomes: &[Outcome],
    contexts: &[report::FindingContext],
    inventory: &Inventory,
    history: Option<&History>,
) -> String {
    let findings: Vec<Json> = outcomes
        .iter()
        .filter(|outcome| outcome.is_reported())
        .map(|outcome| report::overflow_json_with_context(outcome, contexts))
        .collect();
    let silenced: Vec<Json> = outcomes
        .iter()
        .filter(|outcome| !outcome.is_reported())
        .map(report::overflow_json)
        .collect();

    // Unconditional, unlike the text section: a consumer should not have to tell
    // "no exceptions" from "this build does not report them"
    // (§FS-004-check-audit.2).
    let mut fields = vec![
        ("findings", Json::Array(findings)),
        ("silenced", Json::Array(silenced)),
        (
            "exceptions",
            Json::Object(vec![
                ("structural", Json::UInt(inventory.kinds.structural as u64)),
                ("deferred", Json::UInt(inventory.kinds.deferred as u64)),
                (
                    "structural_paths",
                    Json::UInt(inventory.kind_paths.structural as u64),
                ),
                (
                    "deferred_paths",
                    Json::UInt(inventory.kind_paths.deferred as u64),
                ),
            ]),
        ),
    ];

    if let Some(top) = &inventory.top {
        let groups: Vec<Json> = top
            .iter()
            .map(|(unit, ranked)| {
                let entries = ranked
                    .iter()
                    .map(|(value, path)| {
                        Json::Object(vec![
                            ("value", Json::UInt(*value)),
                            ("path", Json::str(path.clone())),
                        ])
                    })
                    .collect();
                Json::Object(vec![
                    ("unit", Json::str(unit.to_string())),
                    ("files", Json::Array(entries)),
                ])
            })
            .collect();
        fields.push(("top", Json::Array(groups)));
    }

    if let Some(stale) = &inventory.stale {
        let entries = stale
            .iter()
            .map(|site| {
                Json::Object(vec![
                    ("registry", Json::str(site.registry.clone())),
                    ("path", Json::str(site.path.clone())),
                ])
            })
            .collect();
        fields.push(("stale", Json::Array(entries)));
    }

    if let Some(items) = &inventory.loose {
        fields.push(("loose", loose::json(items)));
    }

    if let Some(coverage) = &inventory.coverage {
        fields.push((
            "coverage",
            Json::Object(vec![
                ("unmatched_rules", str_array(&coverage.unmatched_rules)),
                ("catch_all_only", str_array(&coverage.catch_all_only)),
                ("unused_messages", str_array(&coverage.unused_messages)),
            ]),
        ));
    }

    if let Some(history) = history {
        fields.push(("history", history.json()));
    }

    Json::Object(fields).render()
}

fn str_array(items: &[String]) -> Json {
    Json::Array(items.iter().map(|item| Json::str(item.clone())).collect())
}
