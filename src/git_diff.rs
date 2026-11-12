//! Literal, mode-aware raw Git differences shared by staged and base selection
//! (§FS-004-check-audit.1.6, §FS-004-check-audit.1.5).

/// One raw change, naming the destination for a rename or copy
/// (§FS-004-check-audit.1.6). Base selection also needs the resulting blob and
/// deletion status (§FS-004-check-audit.1.5).
pub(crate) struct Change {
    pub path: String,
    pub blob: String,
    pub status: u8,
    mode: String,
}

impl Change {
    /// Select content-bearing changes within the caller's status scope. A
    /// gitlink has no file content, wherever its commit is stored
    /// (§FS-004-check-audit.1.6, §FS-004-check-audit.1.5).
    pub fn selects_content(&self, statuses: &[u8]) -> bool {
        statuses.contains(&self.status) && self.mode != "160000"
    }
}

/// Parse `--raw -z` output without display unquoting or trimming paths. Renames
/// and copies consume both paths and select their destination
/// (§FS-004-check-audit.1.6); `--no-renames` base diffs use the same format with
/// one path per change (§FS-004-check-audit.1.5).
pub(crate) fn parse(raw: &[u8]) -> Vec<Change> {
    let text = String::from_utf8_lossy(raw);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let mut changes = Vec::new();
    while let Some(meta) = fields.next() {
        let Some(first) = fields.next() else { break };
        // `:<old mode> <new mode> <old blob> <new blob> <status>`
        let meta: Vec<&str> = meta.trim_start_matches(':').split_whitespace().collect();
        let [_, mode, _, blob, status] = meta[..] else {
            continue;
        };
        let Some(&status) = status.as_bytes().first() else {
            continue;
        };
        let path = if matches!(status, b'R' | b'C') {
            let Some(destination) = fields.next() else {
                break;
            };
            destination
        } else {
            first
        };
        changes.push(Change {
            path: path.to_owned(),
            blob: blob.to_owned(),
            status,
            mode: mode.to_owned(),
        });
    }
    changes
}
