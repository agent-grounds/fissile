//! Git-history proof for bounded staged soft debt (§FS-004-check-audit.1.4).
//!
//! History stays command-side: the public checker remains a snapshot engine.
//! Each distinct path uses one rename-following log, while every historical
//! blob is read through one shared `git cat-file --batch` process.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::Tokens;
use crate::{FileMeasurement, Unit};

/// One standing staged soft finding whose history can affect this commit.
#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub path: PathBuf,
    pub rule_id: String,
    pub unit: Unit,
    pub soft_limit: u64,
    pub edit_limit: u64,
    pub count_blank_lines: bool,
    pub count_comment_lines: bool,
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

    let requests: BTreeSet<(String, String)> = logs
        .values()
        .flatten()
        .filter(|record| !shallow.contains(&record.commit))
        .filter_map(|record| {
            record
                .blob
                .as_ref()
                .map(|blob| (blob.clone(), record.path.clone()))
        })
        .collect();
    let measurements = batch_measure(root, tokens, &requests).unwrap_or_default();

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

        for record in records {
            // A shallow boundary's apparent add is a synthetic root diff. Its
            // parent is unavailable, so neither the edit nor a reset is proven.
            if shallow.contains(&record.commit) {
                break;
            }
            let Some(blob) = &record.blob else {
                result.history_complete = true;
                break;
            };
            let key = (blob.clone(), record.path.clone());
            let Some(Some(measurement)) = measurements.get(&key) else {
                break;
            };
            let Some(actual) = measured_value(candidate, measurement) else {
                break;
            };
            if actual <= candidate.soft_limit {
                result.history_complete = true;
                break;
            }
            result.count += 1;
            if record.establishes_absence {
                result.history_complete = true;
                break;
            }
        }
    }

    results
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

/// One first-parent line is the sequence of committed versions the staged
/// commit extends. `--follow` carries that identity through established renames.
fn file_log(root: &Path, path: &str) -> io::Result<Vec<Record>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "--first-parent",
            "--follow",
            "--root",
            "--no-abbrev",
            "--format=format:%x1e%H%x00",
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
    let mut records = Vec::new();
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
        let fields: Vec<String> = chunk[header_end + 1..]
            .split(|byte| *byte == 0)
            .map(|field| String::from_utf8_lossy(field).trim().to_owned())
            .filter(|field| !field.is_empty())
            .collect();
        let Some(meta_index) = fields.iter().position(|field| field.starts_with(':')) else {
            continue;
        };
        let meta: Vec<&str> = fields[meta_index].split_whitespace().collect();
        if meta.len() < 5 {
            continue;
        }
        let status = meta[4];
        let path_index = meta_index + 1;
        let path = if status.starts_with('R') || status.starts_with('C') {
            fields.get(path_index + 1)
        } else {
            fields.get(path_index)
        };
        let Some(path) = path else { continue };
        let blob = (!status.starts_with('D') && !meta[3].bytes().all(|byte| byte == b'0'))
            .then(|| meta[3].to_owned());
        records.push(Record {
            commit,
            blob,
            path: path.clone(),
            establishes_absence: status.starts_with('A'),
        });
    }
    records
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

/// Read and measure all needed historical objects through one Git process.
fn batch_measure(
    root: &Path,
    tokens: &Tokens,
    requests: &BTreeSet<(String, String)>,
) -> io::Result<HashMap<(String, String), Option<FileMeasurement>>> {
    if requests.is_empty() {
        return Ok(HashMap::new());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut input = child.stdin.take().expect("piped git stdin");
    let object_ids: Vec<String> = requests.iter().map(|(blob, _)| blob.clone()).collect();
    let writer = std::thread::spawn(move || -> io::Result<()> {
        for object in object_ids {
            writeln!(input, "{object}")?;
        }
        Ok(())
    });
    let stdout = child.stdout.take().expect("piped git stdout");
    let mut reader = BufReader::new(stdout);
    let mut measured = HashMap::new();

    for (blob, path) in requests {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let fields: Vec<&str> = header.split_whitespace().collect();
        if fields.last() == Some(&"missing") || fields.len() < 3 {
            measured.insert((blob.clone(), path.clone()), None);
            continue;
        }
        let size: usize = fields[2]
            .parse()
            .map_err(|_| io::Error::other("invalid git cat-file size"))?;
        let mut content = vec![0; size];
        reader.read_exact(&mut content)?;
        let mut newline = [0];
        reader.read_exact(&mut newline)?;
        let measurement = crate::scan::measure_history_blob(root, path, &content, tokens).ok();
        measured.insert((blob.clone(), path.clone()), measurement);
    }

    let write_result = writer
        .join()
        .map_err(|_| io::Error::other("git cat-file input writer panicked"))?;
    write_result?;
    if !child.wait()?.success() {
        return Err(io::Error::other("git cat-file --batch failed"));
    }
    Ok(measured)
}
