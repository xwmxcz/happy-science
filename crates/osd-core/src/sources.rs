//! Owns source snapshot manifests and deterministic quote-to-snapshot verification.
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path};

const SNAPSHOT_ROOT: &str = "evidence/snapshots";
const MAX_SNAPSHOT_BYTES: u64 = 20 * 1024 * 1024;
pub const SCHEMA_VERSION: u32 = 1;

pub fn manifest_path(mission_id: &str) -> String {
    format!("evidence/{mission_id}.sources.jsonl")
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSnapshot {
    pub schema_version: u32,
    pub source_id: String,
    pub title: String,
    pub retrieved_url: String,
    pub retrieved_at: u64,
    pub snapshot_path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceManifestCheck {
    pub path: String,
    pub entries: Vec<SourceSnapshot>,
    pub records: usize,
    pub verified_snapshots: usize,
    pub verified_source_ids: Vec<String>,
    pub quote_matches: usize,
    pub issues: Vec<SourceIssue>,
}

impl SourceManifestCheck {
    pub fn valid(&self) -> bool {
        self.records > 0 && self.verified_snapshots == self.records && self.issues.is_empty()
    }
}

pub(crate) struct CheckedSources {
    pub check: SourceManifestCheck,
    snapshots: HashMap<String, CheckedSnapshot>,
}

struct CheckedSnapshot {
    title: String,
    text: String,
}

pub(crate) fn canonical_source_id(value: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    for prefix in [
        "doi:",
        "https://doi.org/",
        "http://doi.org/",
        "http://dx.doi.org/",
    ] {
        if let Some(doi) = lower.strip_prefix(prefix) {
            return doi.to_owned();
        }
    }
    if lower.starts_with("10.") {
        lower
    } else {
        trimmed.to_owned()
    }
}

pub(crate) fn source_id_valid(value: &str) -> bool {
    let canonical = canonical_source_id(value);
    let canonical_lower = canonical.to_ascii_lowercase();
    if canonical_lower.starts_with("http://") || canonical_lower.starts_with("https://") {
        return true;
    }
    let Some((prefix, suffix)) = canonical.split_once('/') else {
        return false;
    };
    let registrant = prefix.strip_prefix("10.").unwrap_or_default();
    (4..=9).contains(&registrant.len())
        && registrant.bytes().all(|byte| byte.is_ascii_digit())
        && !suffix.is_empty()
        && !suffix.chars().any(char::is_whitespace)
}

fn url_valid(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("https://") || value.starts_with("http://")
}

fn sha256_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn snapshot_path_valid(value: &str) -> bool {
    let path = Path::new(value);
    if path.is_absolute() {
        return false;
    }
    let components = path.components().collect::<Vec<_>>();
    components.len() >= 3
        && components[0] == Component::Normal(OsStr::new("evidence"))
        && components[1] == Component::Normal(OsStr::new("snapshots"))
        && components
            .iter()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn content_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn issue(check: &mut SourceManifestCheck, line: usize, message: impl Into<String>) {
    check.issues.push(SourceIssue {
        line,
        message: message.into(),
    });
}

pub(crate) fn check_manifest_at(root: &Path, relative: &str) -> CheckedSources {
    let mut checked = CheckedSources {
        check: SourceManifestCheck {
            path: relative.into(),
            entries: Vec::new(),
            records: 0,
            verified_snapshots: 0,
            verified_source_ids: Vec::new(),
            quote_matches: 0,
            issues: Vec::new(),
        },
        snapshots: HashMap::new(),
    };
    let file = match File::open(root.join(relative)) {
        Ok(file) => file,
        Err(error) => {
            issue(
                &mut checked.check,
                0,
                format!("source manifest cannot be read: {error}"),
            );
            return checked;
        }
    };
    let mut source_ids = HashSet::new();
    let mut snapshot_paths = HashSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        let line = match line {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => continue,
            Err(error) => {
                issue(
                    &mut checked.check,
                    line_number,
                    format!("line cannot be read: {error}"),
                );
                continue;
            }
        };
        let record = match serde_json::from_str::<SourceSnapshot>(&line) {
            Ok(record) => record,
            Err(error) => {
                issue(
                    &mut checked.check,
                    line_number,
                    format!("invalid source record: {error}"),
                );
                continue;
            }
        };
        checked.check.records += 1;
        checked.check.entries.push(record.clone());
        let source_id = canonical_source_id(&record.source_id);
        let title = record.title.trim();
        let snapshot_path = record.snapshot_path.trim();
        let mut record_valid = true;
        if record.schema_version != SCHEMA_VERSION {
            issue(
                &mut checked.check,
                line_number,
                format!("schemaVersion must be {SCHEMA_VERSION}"),
            );
            record_valid = false;
        }
        if !source_id_valid(&source_id) {
            issue(
                &mut checked.check,
                line_number,
                "sourceId must be a DOI or HTTP(S) URL",
            );
            record_valid = false;
        }
        if title.is_empty() {
            issue(&mut checked.check, line_number, "title is required");
            record_valid = false;
        }
        if !url_valid(&record.retrieved_url) {
            issue(
                &mut checked.check,
                line_number,
                "retrievedUrl must be an HTTP(S) URL",
            );
            record_valid = false;
        }
        if record.retrieved_at == 0 {
            issue(
                &mut checked.check,
                line_number,
                "retrievedAt must be a Unix timestamp",
            );
            record_valid = false;
        }
        if !snapshot_path_valid(snapshot_path) {
            issue(
                &mut checked.check,
                line_number,
                format!("snapshotPath must stay under {SNAPSHOT_ROOT}"),
            );
            record_valid = false;
        }
        if !sha256_valid(&record.sha256) {
            issue(
                &mut checked.check,
                line_number,
                "sha256 must be 64 lowercase hexadecimal characters",
            );
            record_valid = false;
        }
        if !source_id.is_empty() && !source_ids.insert(source_id.clone()) {
            issue(&mut checked.check, line_number, "sourceId must be unique");
            record_valid = false;
        }
        if !snapshot_path.is_empty() && !snapshot_paths.insert(snapshot_path.to_owned()) {
            issue(
                &mut checked.check,
                line_number,
                "snapshotPath must be unique",
            );
            record_valid = false;
        }
        if !record_valid {
            continue;
        }
        let full_path = root.join(snapshot_path);
        let canonical_root = match fs::canonicalize(root) {
            Ok(path) => path,
            Err(error) => {
                issue(
                    &mut checked.check,
                    line_number,
                    format!("workspace cannot be resolved: {error}"),
                );
                continue;
            }
        };
        let canonical_snapshot = match fs::canonicalize(&full_path) {
            Ok(path) if path.starts_with(&canonical_root) => path,
            Ok(_) => {
                issue(
                    &mut checked.check,
                    line_number,
                    "snapshot resolves outside the workspace",
                );
                continue;
            }
            Err(error) => {
                issue(
                    &mut checked.check,
                    line_number,
                    format!("snapshot cannot be resolved: {error}"),
                );
                continue;
            }
        };
        let metadata = match fs::metadata(&canonical_snapshot) {
            Ok(metadata) if metadata.is_file() && metadata.len() > 0 => metadata,
            Ok(_) => {
                issue(
                    &mut checked.check,
                    line_number,
                    "snapshot must be a non-empty file",
                );
                continue;
            }
            Err(error) => {
                issue(
                    &mut checked.check,
                    line_number,
                    format!("snapshot cannot be read: {error}"),
                );
                continue;
            }
        };
        if metadata.len() > MAX_SNAPSHOT_BYTES {
            issue(
                &mut checked.check,
                line_number,
                format!("snapshot exceeds {MAX_SNAPSHOT_BYTES} bytes"),
            );
            continue;
        }
        let bytes = match fs::read(&canonical_snapshot) {
            Ok(bytes) => bytes,
            Err(error) => {
                issue(
                    &mut checked.check,
                    line_number,
                    format!("snapshot cannot be read: {error}"),
                );
                continue;
            }
        };
        if content_sha256(&bytes) != record.sha256 {
            issue(
                &mut checked.check,
                line_number,
                "snapshot sha256 does not match",
            );
            continue;
        }
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                issue(
                    &mut checked.check,
                    line_number,
                    "snapshot must be UTF-8 text",
                );
                continue;
            }
        };
        checked.check.verified_snapshots += 1;
        checked
            .check
            .verified_source_ids
            .push(record.source_id.clone());
        checked.snapshots.insert(
            source_id,
            CheckedSnapshot {
                title: title.to_owned(),
                text,
            },
        );
    }
    if checked.check.records == 0 && checked.check.issues.is_empty() {
        issue(
            &mut checked.check,
            0,
            "source manifest must contain at least one source record",
        );
    }
    checked
}

/// Append a kernel-captured snapshot without allowing duplicate source
/// identities or a second implementation of the manifest contract.
pub(crate) fn append_snapshot_at(
    root: &Path,
    mission_id: &str,
    record: SourceSnapshot,
) -> Result<SourceManifestCheck, String> {
    let relative = manifest_path(mission_id);
    let full = root.join(&relative);
    if full.is_file() {
        let checked = check_manifest_at(root, &relative).check;
        if !checked.issues.is_empty() {
            return Err(
                "the source manifest is invalid and must be repaired before appending".into(),
            );
        }
        let canonical = canonical_source_id(&record.source_id);
        if let Some(existing) = checked
            .entries
            .iter()
            .find(|entry| canonical_source_id(&entry.source_id) == canonical)
        {
            if existing == &record {
                return Ok(checked);
            }
            return Err(format!(
                "source already exists with a different snapshot: {}",
                record.source_id
            ));
        }
    }
    fs::create_dir_all(full.parent().expect("source manifest has a parent"))
        .map_err(|error| format!("source manifest directory failed: {error}"))?;
    let line = serde_json::to_string(&record)
        .map_err(|error| format!("source snapshot serialize failed: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&full)
        .map_err(|error| format!("source manifest open failed: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("source manifest write failed: {error}"))?;
    let checked = check_manifest_at(root, &relative).check;
    if checked.issues.is_empty() {
        Ok(checked)
    } else {
        Err("captured source did not satisfy the source snapshot contract".into())
    }
}

pub(crate) fn verify_quote(
    checked: &mut CheckedSources,
    evidence_line: usize,
    source_id: &str,
    source_title: &str,
    quote: &str,
) {
    let canonical_id = canonical_source_id(source_id);
    let Some(snapshot) = checked.snapshots.get(&canonical_id) else {
        issue(
            &mut checked.check,
            evidence_line,
            format!("evidence source is missing from source manifest: {source_id}"),
        );
        return;
    };
    if snapshot.title != source_title.trim() {
        issue(
            &mut checked.check,
            evidence_line,
            format!("evidence source title does not match source manifest: {source_id}"),
        );
        return;
    }
    if !quote.trim().is_empty() && snapshot.text.contains(quote.trim()) {
        checked.check.quote_matches += 1;
    } else {
        issue(
            &mut checked.check,
            evidence_line,
            format!("evidence quote is not an exact snapshot excerpt: {source_id}"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "happy-science-sources-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(SNAPSHOT_ROOT)).unwrap();
        root
    }

    fn record(snapshot_path: &str, text: &str) -> String {
        serde_json::json!({
            "schemaVersion": SCHEMA_VERSION,
            "sourceId": "10.1000/test",
            "title": "Study A",
            "retrievedUrl": "https://doi.org/10.1000/test",
            "retrievedAt": 1_700_000_000_u64,
            "snapshotPath": snapshot_path,
            "sha256": content_sha256(text.as_bytes()),
        })
        .to_string()
    }

    #[test]
    fn verifies_snapshot_hash_and_exact_quote() {
        let root = root("valid");
        let text = "The observed effect was bounded in the primary analysis.";
        let snapshot_path = "evidence/snapshots/study-a.txt";
        fs::write(root.join(snapshot_path), text).unwrap();
        let manifest = manifest_path("hsm_test");
        fs::write(root.join(&manifest), record(snapshot_path, text)).unwrap();

        let mut checked = check_manifest_at(&root, &manifest);
        verify_quote(
            &mut checked,
            1,
            "10.1000/test",
            "Study A",
            "effect was bounded",
        );
        assert!(checked.check.valid());
        assert_eq!(checked.check.entries.len(), 1);
        assert_eq!(checked.check.entries[0].snapshot_path, snapshot_path);
        assert_eq!(checked.check.verified_source_ids, ["10.1000/test"]);
        assert_eq!(checked.check.quote_matches, 1);
        assert_eq!(
            canonical_source_id("https://doi.org/10.1000/TEST"),
            canonical_source_id("doi:10.1000/test")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_path_escape_hash_mismatch_and_missing_quote() {
        let root = root("invalid");
        let text = "Original source text.";
        let snapshot_path = "evidence/snapshots/study-a.txt";
        fs::write(root.join(snapshot_path), text).unwrap();
        let manifest = manifest_path("hsm_test");
        let bad_hash = record(snapshot_path, "different text");
        fs::write(root.join(&manifest), bad_hash).unwrap();

        let mut checked = check_manifest_at(&root, &manifest);
        verify_quote(
            &mut checked,
            1,
            "10.1000/test",
            "Study A",
            "fabricated excerpt",
        );
        assert!(!checked.check.valid());
        assert!(checked
            .check
            .issues
            .iter()
            .any(|item| item.message.contains("sha256")));
        assert!(snapshot_path_valid("evidence/snapshots/source.txt"));
        assert!(!snapshot_path_valid("../source.txt"));
        assert!(!snapshot_path_valid("evidence/snapshots/../../source.txt"));
        let _ = fs::remove_dir_all(root);
    }
}
