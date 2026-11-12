//! Named paths retain measurements while human findings explain working scan
//! exclusions per file, including in mixed guidance blocks (§FS-004-check-audit.1).
//! Ports #102's triage reproducer; JSON and automatic selection are controls.

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::base_support::{Work, commit, fissile, folded, stdout};
use super::git;

const HTML: [&str; 2] = ["funding/viewer/funding.html", "web/public/radar-sim.html"];
const PEERS: [&str; 2] = ["funding/viewer/funding.data", "web/public/radar-sim.data"];
const SIZES: [usize; 2] = [605999, 1211999];
const LIMITS: [usize; 2] = [524288, 1048576];
const CEILINGS: [usize; 2] = [606208, 1212416];
const MATCH: &str = "**/*.html";

struct Fixture {
    work: Work,
    messages: [String; 2],
}

impl Fixture {
    fn new(name: &str) -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .expect("HOME or USERPROFILE names scratch storage");
        let work = Work(PathBuf::from(home).join(format!(
            "ag/tmp/fissile-102-e2e-{}-{name}",
            std::process::id()
        )));
        fs::create_dir_all(work.path()).unwrap();
        git(work.path(), &["init", "-q", "-b", "main"]);
        let init = fissile(work.path(), &["init", "--no-hook", "--exceptions"]);
        assert_eq!(init.status.code(), Some(0), "{init:?}");
        let path = work.path().join(".agent-grounds/fissile.toml");
        let original = fs::read_to_string(&path).unwrap();
        let config: toml::Value = toml::from_str(&original).unwrap();
        let messages = ["large-file-soft", "large-file-hard"].map(|id| {
            config["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["id"].as_str() == Some(id))
                .unwrap()["text"]
                .as_str()
                .unwrap()
                .trim()
                .replace("{rule}", "large-file-default")
        });
        // Both decoys precede the actual match: `*` cannot span path segments,
        // and `.htm` cannot match `.html`. Reporting the first entry is wrong.
        let config = original.replacen(
            "exclude = [",
            "exclude = [\n  \"funding/*.html\",\n  \"web/public/*.htm\",\n  \"**/*.html\",",
            1,
        );
        assert_ne!(
            config, original,
            "insert scan exclusion into shipped config"
        );
        fs::write(path, config).unwrap();
        commit(work.path(), "config baseline");
        for (path, size) in HTML.iter().zip(SIZES) {
            let target = work.path().join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, vec![b'a'; size]).unwrap();
        }
        git(work.path(), &["add", "-A"]);
        Self { work, messages }
    }

    fn run(&self, args: &[&str], exit: i32) -> String {
        let output = fissile(self.work.path(), args);
        assert_eq!(output.status.code(), Some(exit), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
        stdout(&output)
    }

    fn check(&self, paths: &[&str], exit: i32, json: bool) -> String {
        let mut args = vec!["check"];
        args.extend_from_slice(paths);
        args.extend(if json {
            vec!["--format", "json"]
        } else {
            vec!["--no-color"]
        });
        self.run(&args, exit)
    }

    fn expected(&self, path: &str, tier: usize) -> Value {
        let severity = ["soft", "hard"][tier];
        json!({
            "path": path, "unit": "bytes", "actual": SIZES[tier],
            "limit": LIMITS[tier], "severity": severity,
            "rule_id": "large-file-default",
            "message_id": format!("large-file-{severity}"),
            "message": self.messages[tier],
            "exception_would_accept": CEILINGS[tier]
        })
    }

    fn assert_json(&self, paths: &[&str], exit: i32) {
        let actual: Value = serde_json::from_str(&self.check(paths, exit, true)).unwrap();
        let expected: Vec<_> = paths
            .iter()
            .map(|path| self.expected(path, usize::from(path.contains("radar-sim"))))
            .collect();
        assert_eq!(
            actual,
            json!(expected),
            "entire JSON records stay unchanged"
        );
    }

    fn assert_measure(&self) {
        let text = self.run(&["measure", HTML[0], HTML[1], "--no-color"], 0);
        assert!(!text.contains("[scan].exclude"), "{text}");
        let output = self.run(&["measure", HTML[0], HTML[1], "--format", "json"], 0);
        let actual: Value = serde_json::from_str(&output).unwrap();
        let expected: Vec<_> = (0..2)
            .map(|tier| {
                json!({
                    "path": HTML[tier], "unit": "bytes", "actual": SIZES[tier],
                    "rule_id": "large-file-default", "soft": LIMITS[0], "hard": LIMITS[1],
                    "headroom": LIMITS[1] as i64 - SIZES[tier] as i64, "headroom_to": "hard"
                })
            })
            .collect();
        assert_eq!(actual, json!(expected));
    }

    fn automatic_controls(&self) {
        assert_eq!(self.check(&["--staged"], 0, false), "ok\n");
        assert_eq!(self.run(&["audit", "--no-color"], 0), "ok\n");
        commit(self.work.path(), "excluded HTML");
        assert_eq!(self.check(&["--base", "HEAD~1"], 0, false), "ok\n");
    }

    fn add_peers(&self) {
        for (html, peer) in HTML.iter().zip(PEERS) {
            fs::copy(self.work.path().join(html), self.work.path().join(peer)).unwrap();
        }
        git(self.work.path(), &["add", "-A"]);
        // Ordinary peers prove that automatic modes really scan this scope.
        for args in [
            vec!["check", "--staged", "--no-color"],
            vec!["audit", "--no-color"],
        ] {
            let text = self.run(&args, 1);
            for path in HTML {
                assert!(!text.contains(path), "excluded {path}: {text}");
            }
            for path in PEERS {
                assert!(text.contains(path), "ordinary {path}: {text}");
            }
        }
        commit(self.work.path(), "ordinary peers");
        let base = self.check(&["--base", "HEAD~2"], 1, false);
        for path in HTML {
            assert!(!base.contains(path), "{base}");
        }
        for path in PEERS {
            assert!(base.contains(path), "{base}");
        }
    }
}

fn file_details(block: &str, path: &str) -> String {
    let marker = format!("{path}:");
    let mut lines = block
        .lines()
        .skip_while(|line| !line.trim_start().starts_with(&marker));
    let first = lines
        .next()
        .unwrap_or_else(|| panic!("missing {path}: {block}"));
    let rest = lines.take_while(|line| {
        !line.trim().is_empty()
            && !HTML
                .iter()
                .chain(PEERS.iter())
                .any(|other| line.trim_start().starts_with(&format!("{other}:")))
    });
    folded(
        &std::iter::once(first)
            .chain(rest)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn assert_context(details: &str, path: &str) {
    assert!(
        details.contains("[scan].exclude") && details.contains(MATCH),
        "missing actual matching [scan].exclude entry {MATCH:?} beside {path}: {details}"
    );
    assert!(
        details.contains("measured") && (details.contains("named") || details.contains("explicit")),
        "missing explanation that naming {path} caused measurement: {details}"
    );
    for mode in ["check --staged", "check --base", "audit"] {
        assert!(
            details.contains(mode),
            "missing skipped mode {mode} for {path}: {details}"
        );
    }
    assert!(
        details.contains("skip"),
        "automatic modes skip {path}: {details}"
    );
    for decoy in ["funding/*.html", "web/public/*.htm"] {
        assert!(
            !details.contains(decoy),
            "non-matching entry {decoy}: {details}"
        );
    }
}

fn named_finding(tier: usize) {
    let fixture = Fixture::new(["soft", "hard"][tier]);
    fixture.automatic_controls();
    fixture.assert_measure();
    let exit = tier as i32;
    fixture.assert_json(&[HTML[tier]], exit);
    let text = fixture.check(&[HTML[tier]], exit, false);
    assert_context(&file_details(&text, HTML[tier]), HTML[tier]);
}

#[test]
fn issue_102_soft_named_finding_explains_matching_exclusion() {
    named_finding(0);
}

#[test]
fn issue_102_hard_named_finding_explains_matching_exclusion() {
    named_finding(1);
}

#[test]
fn issue_102_mixed_blocks_attribute_context_only_to_excluded_files() {
    let fixture = Fixture::new("mixed");
    fixture.automatic_controls();
    fixture.add_peers();
    let paths = [HTML[0], HTML[1], PEERS[0], PEERS[1]];
    fixture.assert_json(&paths, 1);
    let ordinary = fixture.check(&PEERS, 1, false);
    let text = fixture.check(&paths, 1, false);
    let blocks: Vec<_> = text
        .split("\n\n")
        .filter(|block| block.starts_with("hard:") || block.starts_with("soft:"))
        .collect();
    assert_eq!(blocks.len(), 2, "group identity stays intact: {text}");
    // All compatibility assertions precede the new context assertions.
    for (block, tier) in blocks.iter().zip([1, 0]) {
        let severity = ["soft", "hard"][tier];
        assert_eq!(
            block.lines().next().unwrap(),
            format!(
                "{severity}: 2 files over the {}-byte budget [rule: large-file-default, message: large-file-{severity}]",
                LIMITS[tier]
            )
        );
        let detail_start = block
            .lines()
            .position(|line| line.contains(HTML[tier]) || line.contains(PEERS[tier]))
            .unwrap();
        let guidance = block
            .lines()
            .skip(1)
            .take(detail_start - 1)
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            folded(&guidance),
            folded(&fixture.messages[tier]),
            "shared guidance unchanged"
        );
        let peer = file_details(block, PEERS[tier]);
        assert!(
            !peer.contains("[scan].exclude") && !peer.contains(MATCH),
            "ordinary peer must not claim a scan exclusion: {peer}"
        );
        assert_eq!(
            peer,
            file_details(&ordinary, PEERS[tier]),
            "ordinary peer must not inherit exclusion context"
        );
        assert!(
            block.find(PEERS[tier]).unwrap() < block.find(HTML[tier]).unwrap(),
            "file order: {block}"
        );
    }
    for (block, tier) in blocks.iter().zip([1, 0]) {
        assert_context(&file_details(block, HTML[tier]), HTML[tier]);
    }
}
