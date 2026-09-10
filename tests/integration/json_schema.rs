//! The published JSON schema and the bytes `fissile` actually emits stay in
//! lockstep (§GOAL-003-friendly-output.1, §GOAL-004-token-thrift.1). A new or
//! renamed field that is not reflected in `schema/` fails here.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use fissile::Severity;
use fissile::Unit;
use fissile::audit::{self, AuditOptions};
use fissile::check::{self, CheckOptions};
use fissile::cli::Format;
use fissile::exception::{self, AddOptions, Rationale};
use fissile::exceptions::{Kind, MatchKind};
use fissile::limits::{self, LimitsOptions};
use fissile::measure::{self, MeasureOptions};

/// Required keys on every finding record (§FS-004-check-audit.1).
const REQUIRED: &[&str] = &[
    "path",
    "unit",
    "actual",
    "limit",
    "severity",
    "rule_id",
    "message_id",
    "message",
];
/// Extra keys only silenced `audit` records carry (§FS-003-exceptions.5). The
/// accepting entry is not named: it has no name (§DF-005-exception-identity).
const SILENCED_EXTRA: &[&str] = &["exception_max"];
/// The extra key a standing finding carries: the ceiling a plain `fissile
/// exception add` would write for the file (§FS-004-check-audit.1). Silenced
/// records do not carry it — the entry that already accepts the file reports
/// its own ceiling as `exception_max` instead.
const STANDING_EXTRA: &[&str] = &["exception_would_accept"];
/// Provenance carried only by staged soft findings (§FS-004-check-audit.1.4).
const STAGED_SOFT_EXTRA: &[&str] = &[
    "soft_edit_count",
    "soft_edit_limit",
    "soft_edit_history_complete",
    "promotion",
];

// Values in the fixture are free of `,` and `:` so a flat object splits cleanly.
const CONFIG: &str = r#"
fissile_config_version = 1
[scan]
include = ["src"]
exclude = []
respect_gitignore = false
[[messages]]
id = "m"
text = "Split the file."
[[rules]]
id = "rust"
include = ["src/**/*.rs"]
unit = "lines"
soft = 100
hard = 200
message = "m"
"#;

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema")
}

fn temp_repo() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fissile-schema-{}-{n}", std::process::id()));
    fs::create_dir_all(dir.join(".agent-grounds")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join(".agent-grounds/fissile.toml"), CONFIG).unwrap();
    let body: String = (0..250).map(|i| format!("fn f{i}() {{}}\n")).collect();
    fs::write(dir.join("src/big.rs"), body).unwrap();
    dir
}

/// Top-level keys of one flat JSON object whose values contain no `,` or `:`.
fn object_keys(object: &str) -> Vec<String> {
    let inner = object.trim().trim_start_matches('{').trim_end_matches('}');
    inner
        .split(',')
        .map(|pair| {
            let key = pair.split(':').next().expect("key before colon").trim();
            key.trim_matches('"').to_owned()
        })
        .collect()
}

/// Pull each `{...}` object out of a flat JSON array of flat objects.
fn array_objects(array: &str) -> Vec<String> {
    let inner = array.trim().trim_start_matches('[').trim_end_matches(']');
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner
        .split("},")
        .map(|chunk| {
            let chunk = chunk.trim();
            if chunk.ends_with('}') {
                chunk.to_owned()
            } else {
                format!("{chunk}}}")
            }
        })
        .collect()
}

#[test]
fn schema_declares_every_finding_field() {
    let finding = fs::read_to_string(schema_dir().join("finding.schema.json")).unwrap();
    for field in REQUIRED
        .iter()
        .chain(SILENCED_EXTRA)
        .chain(STANDING_EXTRA)
        .chain(STAGED_SOFT_EXTRA)
    {
        assert!(
            finding.contains(&format!("\"{field}\"")),
            "schema/finding.schema.json is missing field `{field}`"
        );
    }
    // The check/audit schemas reference the shared finding shape.
    let check = fs::read_to_string(schema_dir().join("check.schema.json")).unwrap();
    let audit = fs::read_to_string(schema_dir().join("audit.schema.json")).unwrap();
    assert!(check.contains("finding.schema.json"));
    assert!(audit.contains("finding.schema.json"));
}

#[test]
fn check_json_records_match_the_schema() {
    let root = temp_repo();
    let run = check::run(&CheckOptions {
        root,
        config_path: None,
        staged: false,
        format: Some(Format::Json),
        no_color: false,
        paths: Vec::new(),
    })
    .expect("check runs");

    let records = array_objects(&run.output);
    assert_eq!(records.len(), 1, "one hard record for the 250-line file");
    let keys = object_keys(&records[0]);
    let expected: Vec<String> = REQUIRED
        .iter()
        .chain(STANDING_EXTRA)
        .map(|field| (*field).to_owned())
        .collect();
    assert_eq!(
        sorted(&keys),
        sorted(&expected),
        "check record keys must be the required finding fields plus the ceiling"
    );
    // 250 lines against a 200-line hard limit, under the default 100-line step.
    assert!(
        run.output.contains("\"exception_would_accept\":300"),
        "{}",
        run.output
    );
    assert_schema_known(&keys);
}

#[test]
fn audit_silenced_records_carry_documented_exception_fields() {
    let root = temp_repo();
    exception::run(&AddOptions {
        root: root.clone(),
        config_path: None,
        path: "src/big.rs".to_owned(),
        severity: Severity::Hard,
        rules: vec!["rust".to_owned()],
        rationale: Rationale::Stated {
            kind: Kind::Deferred,
            reason: "no module owns the staged-blob reader yet".to_owned(),
            until: Some("the reader module lands".to_owned()),
        },
        match_kind: MatchKind::Exact,
        title: None,
        owner: None,
        issue: None,
        max: None,
        unit: None,
        interactive: true,
        force: false,
        dry_run: false,
    })
    .expect("exception add runs");

    let run = audit::run(&AuditOptions {
        root,
        config_path: None,
        format: Some(Format::Json),
        no_color: false,
        top: Some(1),
        stale_exceptions: true,
        rule_coverage: false,
        only: None,
    })
    .expect("audit runs");

    // Envelope keys present and documented.
    assert!(run.output.contains("\"findings\""));
    assert!(run.output.contains("\"silenced\""));
    assert!(run.output.contains("\"top\""));
    assert!(run.output.contains("\"stale\""));
    // Unconditional, so a consumer never has to tell "no exceptions" from "this
    // build does not report them" (§FS-004-check-audit.2).
    assert!(
        run.output
            .contains(
                "\"exceptions\":{\"structural\":0,\"deferred\":1,\"structural_paths\":0,\"deferred_paths\":1}"
            )
    );

    let audit_schema = fs::read_to_string(schema_dir().join("audit.schema.json")).unwrap();
    for field in [
        "structural",
        "deferred",
        "structural_paths",
        "deferred_paths",
    ] {
        assert!(
            audit_schema.contains(&format!("\"{field}\"")),
            "schema/audit.schema.json is missing `{field}`"
        );
    }

    // The silenced hard overflow carries the exception attribution fields.
    let silenced = extract_array(&run.output, "silenced");
    let records = array_objects(&silenced);
    assert_eq!(records.len(), 1, "the hard overflow is silenced once");
    let keys = object_keys(&records[0]);
    for field in REQUIRED.iter().chain(SILENCED_EXTRA) {
        assert!(
            keys.iter().any(|k| k == field),
            "silenced record missing `{field}`"
        );
    }
    // An entry already accepts this file, so there is no plain `add` to name a
    // ceiling for (§FS-004-check-audit.1).
    for field in STANDING_EXTRA {
        assert!(
            !keys.iter().any(|k| k == field),
            "silenced record carries `{field}`"
        );
    }
    assert_schema_known(&keys);
}

/// §FS-007-measure.2: every measure field is declared, and the signed headroom
/// says which side of the threshold the file is on without a second field.
#[test]
fn measure_records_match_the_published_schema() {
    let root = temp_repo();
    let run = measure::run(&MeasureOptions {
        root,
        config_path: None,
        staged: false,
        format: Some(Format::Json),
        no_color: false,
        paths: vec!["src/big.rs".to_owned()],
    })
    .expect("measure runs");

    let records = array_objects(&run.output);
    assert_eq!(records.len(), 1, "one line-rule record for the fixture");
    let keys = object_keys(&records[0]);
    let expected: Vec<String> = [
        "actual",
        "hard",
        "headroom",
        "headroom_to",
        "path",
        "rule_id",
        "soft",
        "unit",
    ]
    .iter()
    .map(|key| (*key).to_owned())
    .collect();
    assert_eq!(sorted(&keys), sorted(&expected));

    // 250 lines against a 200-line hard limit: negative headroom past the
    // highest threshold. -50: equality is accepted, so 200 is the last line
    // that clears it.
    assert!(run.output.contains("\"headroom\":-50"), "{}", run.output);
    assert!(run.output.contains("\"headroom_to\":\"hard\""));

    let schema = fs::read_to_string(schema_dir().join("measure.schema.json")).unwrap();
    for key in &keys {
        assert!(
            schema.contains(&format!("\"{key}\"")),
            "schema/measure.schema.json is missing `{key}`"
        );
    }
}

/// §FS-010-limits.4: every field of the rule inventory is declared, and the
/// envelope is an object keyed `rules` rather than a bare array.
#[test]
fn limits_records_match_the_published_schema() {
    let root = temp_repo();
    let run = limits::run(&LimitsOptions {
        root: root.clone(),
        config_path: None,
        format: Some(Format::Json),
        no_color: false,
    })
    .expect("limits runs");

    let rules = extract_array(&run.output, "rules");
    let records = array_objects(&rules);
    assert_eq!(records.len(), 1, "one record for the fixture's one rule");
    let keys = object_keys(&records[0]);
    let expected: Vec<String> = [
        "count_blank_lines",
        "count_comment_lines",
        "hard",
        "hard_message",
        "id",
        "include",
        "priority",
        "soft",
        "soft_edit_limit",
        "soft_message",
        "unit",
    ]
    .iter()
    .map(|key| (*key).to_owned())
    .collect();
    assert_eq!(sorted(&keys), sorted(&expected));

    // The config's values, not the tree's: no file was measured (§FS-010-limits.5).
    assert!(
        run.output
            .starts_with(r#"{"rules":[{"id":"rust","include":["src/**/*.rs"]"#),
        "{}",
        run.output
    );
    assert!(
        run.output
            .contains(r#""soft":100,"soft_edit_limit":5,"hard":200"#),
        "{}",
        run.output
    );

    let explicit = CONFIG.replace("soft = 100", "soft = 100\nsoft_edit_limit = 2");
    fs::write(root.join(".agent-grounds/fissile.toml"), explicit).unwrap();
    let explicit_run = limits::run(&LimitsOptions {
        root,
        config_path: None,
        format: Some(Format::Json),
        no_color: false,
    })
    .expect("limits accepts an explicit edit limit");
    assert!(
        explicit_run.output.contains(r#""soft_edit_limit":2"#),
        "{}",
        explicit_run.output
    );

    let schema = fs::read_to_string(schema_dir().join("limits.schema.json")).unwrap();
    for key in keys.iter().chain([&"rules".to_owned()]) {
        assert!(
            schema.contains(&format!("\"{key}\"")),
            "schema/limits.schema.json is missing `{key}`"
        );
    }
}

/// §FS-003-exceptions.7: a ceiling more than one bump step above its file is
/// reported with the value `exception retune` would write in its place.
#[test]
fn audit_reports_a_loose_ceiling_with_the_value_to_retune_to() {
    let root = temp_repo();
    exception::run(&AddOptions {
        root: root.clone(),
        config_path: None,
        path: "src/big.rs".to_owned(),
        severity: Severity::Hard,
        rules: vec!["rust".to_owned()],
        rationale: Rationale::Stated {
            kind: Kind::Deferred,
            reason: "no module owns the staged-blob reader yet".to_owned(),
            until: Some("the reader module lands".to_owned()),
        },
        match_kind: MatchKind::Exact,
        title: None,
        owner: None,
        issue: None,
        max: Some(500),
        unit: Some(Unit::Lines),
        interactive: true,
        force: false,
        dry_run: false,
    })
    .expect("exception add runs");

    let run = audit::run(&AuditOptions {
        root,
        config_path: None,
        format: Some(Format::Json),
        no_color: false,
        top: None,
        stale_exceptions: true,
        rule_coverage: false,
        only: None,
    })
    .expect("audit runs");

    let loose = extract_array(&run.output, "loose");
    let records = array_objects(&loose);
    assert_eq!(records.len(), 1, "the 500-line ceiling on a 250-line file");
    let keys = object_keys(&records[0]);
    let schema = fs::read_to_string(schema_dir().join("audit.schema.json")).unwrap();
    for key in &keys {
        assert!(
            schema.contains(&format!("\"{key}\"")),
            "schema/audit.schema.json is missing `{key}`"
        );
    }
    // Still over the hard limit, so the remedy is a lower ceiling, not removal.
    assert!(loose.contains("\"retune_to\":300"), "{loose}");
    // Exactly one of the two remedies is ever set (§DF-010-stated-ceilings-are-exact.2).
    assert!(loose.contains("\"stated_range\":null"), "{loose}");
    assert!(loose.contains("\"silences_nothing\":0"), "{loose}");
    // Both halves of the text line's advice are readable from the record: which
    // registry's limit was missed, and what that limit is.
    assert!(loose.contains("\"severity\":\"hard\""), "{loose}");
    assert!(loose.contains("\"limit\":200"), "{loose}");
    // Slack wider than a step, so this is the loose half of the section and not
    // the spent one (§FS-003-exceptions.7).
    assert!(loose.contains("\"no_headroom\":0"), "{loose}");
}

/// §FS-003-exceptions.7: a ceiling sitting exactly on the file it accepts has
/// no headroom, and the record says which half of the section it belongs to
/// without a consumer parsing the text line (§FS-004-check-audit.2).
#[test]
fn audit_flags_a_ceiling_with_no_headroom() {
    let root = temp_repo();
    exception::run(&AddOptions {
        root: root.clone(),
        config_path: None,
        path: "src/big.rs".to_owned(),
        severity: Severity::Hard,
        rules: vec!["rust".to_owned()],
        rationale: Rationale::Stated {
            kind: Kind::Deferred,
            reason: "no module owns the staged-blob reader yet".to_owned(),
            until: Some("the reader module lands".to_owned()),
        },
        match_kind: MatchKind::Exact,
        title: None,
        owner: None,
        issue: None,
        // The pinned ceiling §FS-005-exception-add.2 warns about: exactly what
        // the file measures today, so the next unrelated line fails the gate.
        max: Some(250),
        unit: Some(Unit::Lines),
        interactive: true,
        force: false,
        dry_run: false,
    })
    .expect("exception add runs");

    let run = audit::run(&AuditOptions {
        root,
        config_path: None,
        format: Some(Format::Json),
        no_color: false,
        top: None,
        stale_exceptions: true,
        rule_coverage: false,
        only: None,
    })
    .expect("audit runs");

    let loose = extract_array(&run.output, "loose");
    let records = array_objects(&loose);
    assert_eq!(records.len(), 1, "the 250-line ceiling on a 250-line file");
    let keys = object_keys(&records[0]);
    let schema = fs::read_to_string(schema_dir().join("audit.schema.json")).unwrap();
    for key in &keys {
        assert!(
            schema.contains(&format!("\"{key}\"")),
            "schema/audit.schema.json is missing `{key}`"
        );
    }
    assert!(loose.contains("\"no_headroom\":1"), "{loose}");
    // The step's next multiple strictly above the recorded ceiling: the smallest
    // round number that grants any headroom at all.
    assert!(loose.contains("\"retune_to\":300"), "{loose}");
    // A hard entry, so the hard-limit refusal never applies and the range form
    // stays unused (§DF-010-stated-ceilings-are-exact.2).
    assert!(loose.contains("\"stated_range\":null"), "{loose}");
    assert!(loose.contains("\"silences_nothing\":0"), "{loose}");
    assert!(loose.contains("\"accepted\":250"), "{loose}");
    assert!(loose.contains("\"actual\":250"), "{loose}");
}

/// Every emitted key must be a property the schema declares.
fn assert_schema_known(keys: &[String]) {
    let finding = fs::read_to_string(schema_dir().join("finding.schema.json")).unwrap();
    for key in keys {
        assert!(
            finding.contains(&format!("\"{key}\"")),
            "emitted field `{key}` is not declared in schema/finding.schema.json"
        );
    }
}

/// Pull the JSON array that follows `"<name>":` out of the audit envelope.
fn extract_array(envelope: &str, name: &str) -> String {
    let marker = format!("\"{name}\":");
    let start = envelope.find(&marker).expect("array present") + marker.len();
    let bytes = &envelope[start..];
    let open = bytes.find('[').expect("array open");
    let mut depth = 0;
    for (index, ch) in bytes[open..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return bytes[open..=open + index].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("unterminated array for {name}");
}

fn sorted(items: &[String]) -> Vec<String> {
    let mut out = items.to_vec();
    out.sort();
    out
}
