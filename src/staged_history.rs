//! Git-history proof for bounded staged soft debt (§FS-004-check-audit.1.4).
//!
//! History stays command-side: the public checker remains a snapshot engine.
//! Each distinct path uses one rename-following log, while every historical
//! blob is read through one shared `git cat-file --batch` process.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::config::Tokens;
use crate::{FileMeasurement, Glob, Unit};

/// One standing staged soft finding whose history can affect this commit.
#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub path: PathBuf,
    pub rule_id: String,
    pub unit: Unit,
    pub soft_limit: u64,
    pub edit_limit: u64,
    pub include: Vec<Glob>,
    pub exclude: Vec<Glob>,
    pub count_blank_lines: bool,
    pub count_comment_lines: bool,
}

impl Candidate {
    fn applies_to(&self, path: &str) -> bool {
        self.include.iter().any(|glob| glob.matches(path))
            && !self.exclude.iter().any(|glob| glob.matches(path))
    }
}

/// Provenance attached to a command finding, never to the public overflow.
#[derive(Clone, Debug)]
pub(crate) struct SoftEdit {
    pub path: PathBuf,
    pub rule_id: String,
    pub unit: Unit,
    pub count: u64,
    pub limit: u64,
    pub history_complete: bool,
}

impl SoftEdit {
    pub(crate) fn promoted(&self) -> bool {
        self.history_complete && self.count >= self.limit
    }
}

#[derive(Clone, Debug)]
struct Record {
    commit: String,
    blob: Option<String>,
    path: String,
    establishes_absence: bool,
    provable: bool,
    graph_has_merge: bool,
}

#[derive(Clone, Debug)]
enum Seed {
    Path(String),
    Absent,
}

/// Derive every candidate independently, failing open for paths whose identity,
/// log, object, or measurement cannot be proved. Errors are deliberately not
/// returned: history may never hide findings or unrelated real command errors.
pub(crate) fn derive(root: &Path, tokens: &Tokens, candidates: &[Candidate]) -> Vec<SoftEdit> {
    let mut results: Vec<SoftEdit> = candidates
        .iter()
        .map(|candidate| SoftEdit {
            path: candidate.path.clone(),
            rule_id: candidate.rule_id.clone(),
            unit: candidate.unit,
            count: 1,
            limit: candidate.edit_limit,
            history_complete: false,
        })
        .collect();
    if candidates.is_empty() {
        return results;
    }

    let Ok(seeds) = staged_seeds(root) else {
        return results;
    };
    let Ok(shallow) = shallow_commits(root) else {
        return results;
    };
    let mut logs: BTreeMap<String, Vec<Record>> = BTreeMap::new();
    let mut log_failed = HashSet::new();

    for candidate in candidates {
        let current = candidate.path.to_string_lossy().replace('\\', "/");
        match seeds.get(&current) {
            Some(Seed::Absent) => {}
            Some(Seed::Path(path)) => {
                if !logs.contains_key(path) && !log_failed.contains(path) {
                    match file_log(root, path) {
                        Ok(records) => {
                            logs.insert(path.clone(), records);
                        }
                        Err(_) => {
                            log_failed.insert(path.clone());
                        }
                    }
                }
            }
            None => {
                log_failed.insert(current);
            }
        }
    }

    let mut measurements = HistoryMeasurer::new(root, tokens);

    for (candidate, result) in candidates.iter().zip(&mut results) {
        let current = candidate.path.to_string_lossy().replace('\\', "/");
        let Some(seed) = seeds.get(&current) else {
            continue;
        };
        let Seed::Path(seed_path) = seed else {
            result.history_complete = true;
            continue;
        };
        let Some(records) = logs.get(seed_path) else {
            continue;
        };

        for (index, record) in records.iter().enumerate() {
            if !record.provable {
                break;
            }
            // A shallow boundary's apparent add is a synthetic root diff. Its
            // parent is unavailable, so neither the edit nor a reset is proven.
            if shallow.contains(&record.commit) {
                break;
            }
            if !candidate.applies_to(&record.path) {
                result.history_complete = boundary_closes_graph(root, records, index);
                break;
            }
            let Some(blob) = &record.blob else {
                result.history_complete = boundary_closes_graph(root, records, index);
                break;
            };
            let Ok(Some(measurement)) = measurements.measure(blob, &record.path) else {
                break;
            };
            let Some(actual) = measured_value(candidate, &measurement) else {
                break;
            };
            if actual <= candidate.soft_limit {
                result.history_complete = boundary_closes_graph(root, records, index);
                break;
            }
            result.count += 1;
            if record.establishes_absence {
                result.history_complete = boundary_closes_graph(root, records, index);
                break;
            }
        }
    }

    results
}

/// A reset on one side of a merge is not a boundary for a parallel line. The
/// later log records may be skipped only when every one is an ancestor of the
/// boundary commit; otherwise the DAG is deliberately left incomplete.
fn boundary_closes_graph(root: &Path, records: &[Record], boundary: usize) -> bool {
    if !records[boundary].graph_has_merge {
        return true;
    }
    let is_ancestor = |ancestor: &str, descendant: &str| {
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    let boundary_commit = &records[boundary].commit;
    records[..boundary]
        .iter()
        .all(|newer| is_ancestor(boundary_commit, &newer.commit))
        && records[boundary + 1..]
            .iter()
            .all(|older| is_ancestor(&older.commit, boundary_commit))
}

fn measured_value(candidate: &Candidate, measurement: &FileMeasurement) -> Option<u64> {
    match candidate.unit {
        Unit::Bytes => Some(measurement.bytes),
        Unit::Lines => measurement
            .lines
            .map(|lines| lines.counted(candidate.count_blank_lines, candidate.count_comment_lines)),
        Unit::Tokens => measurement.tokens,
    }
}

/// The staged diff establishes the current path's predecessor, including a
/// rename's old side. Adds and copies establish absence for this file identity.
fn staged_seeds(root: &Path) -> io::Result<HashMap<String, Seed>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "--find-renames",
            "--diff-filter=ACMR",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git diff --cached failed"));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let mut seeds = HashMap::new();
    while let Some(status) = fields.next() {
        let Some(first) = fields.next() else { break };
        match status.as_bytes().first().copied() {
            Some(b'R') => {
                let Some(new) = fields.next() else { break };
                seeds.insert(new.to_owned(), Seed::Path(first.to_owned()));
            }
            Some(b'A' | b'C') => {
                if status.starts_with('C') {
                    let Some(new) = fields.next() else { break };
                    seeds.insert(new.to_owned(), Seed::Absent);
                } else {
                    seeds.insert(first.to_owned(), Seed::Absent);
                }
            }
            Some(b'M') => {
                seeds.insert(first.to_owned(), Seed::Path(first.to_owned()));
            }
            _ => {}
        }
    }
    Ok(seeds)
}

/// Complete reachable history in topological order. `-m` exposes one merge diff
/// per changed parent; [`parse_log`] uses those parent views to distinguish an
/// importing merge from a resolution that introduced a new version.
fn file_log(root: &Path, path: &str) -> io::Result<Vec<Record>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "--full-history",
            "--topo-order",
            "--follow",
            "--root",
            "-m",
            "--find-renames",
            "--no-abbrev",
            "--format=format:%x1e%H%x00%P%x00",
            "--raw",
            "-z",
            "--",
            path,
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git log --follow failed"));
    }
    Ok(parse_log(&output.stdout))
}

fn parse_log(output: &[u8]) -> Vec<Record> {
    #[derive(Clone)]
    struct Entry {
        blob: Option<String>,
        path: String,
        establishes_absence: bool,
    }

    struct Commit {
        id: String,
        parent_count: usize,
        entries: Vec<Entry>,
    }

    let mut commits: Vec<Commit> = Vec::new();
    for chunk in output
        .split(|byte| *byte == 0x1e)
        .filter(|part| !part.is_empty())
    {
        let Some(header_end) = chunk.iter().position(|byte| *byte == 0) else {
            continue;
        };
        let commit = String::from_utf8_lossy(&chunk[..header_end])
            .trim()
            .to_owned();
        let after_commit = &chunk[header_end + 1..];
        let Some(parents_end) = after_commit.iter().position(|byte| *byte == 0) else {
            continue;
        };
        let parent_count = String::from_utf8_lossy(&after_commit[..parents_end])
            .split_whitespace()
            .count();
        let fields: Vec<String> = after_commit[parents_end + 1..]
            .split(|byte| *byte == 0)
            .map(|field| String::from_utf8_lossy(field).trim().to_owned())
            .filter(|field| !field.is_empty())
            .collect();
        let mut entries = Vec::new();
        let mut index = 0;
        while index < fields.len() {
            if !fields[index].starts_with(':') {
                index += 1;
                continue;
            }
            let meta: Vec<&str> = fields[index].split_whitespace().collect();
            if meta.len() < 5 {
                index += 1;
                continue;
            }
            let status = meta[4];
            let path_offset = usize::from(status.starts_with('R') || status.starts_with('C')) + 1;
            let Some(path) = fields.get(index + path_offset) else {
                break;
            };
            let blob = (!status.starts_with('D') && !meta[3].bytes().all(|byte| byte == b'0'))
                .then(|| meta[3].to_owned());
            entries.push(Entry {
                blob,
                path: path.clone(),
                establishes_absence: status.starts_with('A'),
            });
            index += path_offset + 1;
        }
        if let Some(existing) = commits.iter_mut().find(|item| item.id == commit) {
            existing.entries.extend(entries);
        } else {
            commits.push(Commit {
                id: commit,
                parent_count,
                entries,
            });
        }
    }

    let graph_has_merge = commits.iter().any(|commit| commit.parent_count > 1);
    commits
        .into_iter()
        .filter_map(|commit| {
            if commit.entries.is_empty() {
                return None;
            }
            // A merge missing a parent diff has the same file version as that
            // parent. It imports the branch edits but is not a second edit.
            if commit.parent_count > 1 && commit.entries.len() < commit.parent_count {
                return None;
            }
            let first = &commit.entries[0];
            let same_result = commit.entries.iter().all(|entry| {
                entry.blob == first.blob
                    && entry.path == first.path
                    && entry.establishes_absence == first.establishes_absence
            });
            Some(Record {
                commit: commit.id,
                blob: first.blob.clone(),
                path: first.path.clone(),
                establishes_absence: first.establishes_absence,
                provable: (commit.parent_count <= 1 && commit.entries.len() == 1)
                    || (commit.entries.len() == commit.parent_count && same_result),
                graph_has_merge,
            })
        })
        .collect()
}

fn shallow_commits(root: &Path) -> io::Result<HashSet<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "shallow",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git rev-parse --git-path shallow failed"));
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(error) => Err(error),
    }
}

/// Lazy shared `cat-file` session. A history version is requested only when the
/// newest-to-oldest walk reaches it, and the cache shares work across rules.
struct HistoryMeasurer<'a> {
    root: &'a Path,
    tokens: &'a Tokens,
    process: Option<CatFile>,
    cache: HashMap<(String, String), Option<FileMeasurement>>,
}

struct CatFile {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl<'a> HistoryMeasurer<'a> {
    fn new(root: &'a Path, tokens: &'a Tokens) -> Self {
        Self {
            root,
            tokens,
            process: None,
            cache: HashMap::new(),
        }
    }

    fn measure(&mut self, blob: &str, path: &str) -> io::Result<Option<FileMeasurement>> {
        let key = (blob.to_owned(), path.to_owned());
        if let Some(measurement) = self.cache.get(&key) {
            return Ok(measurement.clone());
        }
        if self.process.is_none() {
            self.process = Some(CatFile::spawn(self.root)?);
        }
        let content = self
            .process
            .as_mut()
            .expect("cat-file process initialized")
            .read(blob)?;
        let measurement = match content {
            Some(content) => {
                crate::scan::measure_history_blob(self.root, path, &content, self.tokens).ok()
            }
            None => None,
        };
        self.cache.insert(key, measurement.clone());
        Ok(measurement)
    }
}

impl CatFile {
    fn spawn(root: &Path) -> io::Result<Self> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child.stdin.take().expect("piped git stdin");
        let output = BufReader::new(child.stdout.take().expect("piped git stdout"));
        Ok(Self {
            child,
            input,
            output,
        })
    }

    fn read(&mut self, object: &str) -> io::Result<Option<Vec<u8>>> {
        writeln!(self.input, "{object}")?;
        self.input.flush()?;
        let mut header = String::new();
        if self.output.read_line(&mut header)? == 0 {
            return Err(io::Error::other("git cat-file ended before its response"));
        }
        let fields: Vec<&str> = header.split_whitespace().collect();
        if fields.last() == Some(&"missing") {
            return Ok(None);
        }
        if fields.len() < 3 {
            return Err(io::Error::other("invalid git cat-file header"));
        }
        let size: usize = fields[2]
            .parse()
            .map_err(|_| io::Error::other("invalid git cat-file size"))?;
        let mut content = vec![0; size];
        self.output.read_exact(&mut content)?;
        let mut newline = [0];
        self.output.read_exact(&mut newline)?;
        Ok(Some(content))
    }
}

impl Drop for CatFile {
    fn drop(&mut self) {
        // The process owns no state after its requested blobs were read; never
        // turn cleanup trouble into an error after its evidence was consumed.
        let _ = self.input.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
