//! Crossref-backed literature discovery and verified local source capture.
//!
//! This module owns the literature-corpus contract: search results are
//! normalized and deduplicated by DOI, while selected works are captured as
//! immutable UTF-8 snapshots and registered with the source verifier.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::missions::MissionKind;
use crate::runtime::workspace_dir;
use crate::sources::{SourceManifestCheck, SourceSnapshot};
use crate::Env;

const SCHEMA_VERSION: u32 = 1;
const CORPUS_DIR: &str = ".happy-science/literature";
const CROSSREF_WORKS: &str = "https://api.crossref.org/works";
const MAX_CAPTURE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_SEARCH_BYTES: u64 = 10 * 1024 * 1024;
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiteratureWork {
    pub doi: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    pub landing_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abstract_text: Option<String>,
    pub full_text_urls: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiteratureSearchResult {
    pub provider: String,
    pub query: String,
    pub returned_at: u64,
    pub works: Vec<LiteratureWork>,
    pub duplicates_removed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotStatus {
    FullText,
    AbstractOnly,
    MetadataOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiteratureEntry {
    pub schema_version: u32,
    pub mission_id: String,
    pub canonical_id: String,
    pub work: LiteratureWork,
    pub snapshot_status: SnapshotStatus,
    pub retrieved_url: String,
    pub snapshot_path: String,
    pub sha256: String,
    pub added_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiteratureIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiteratureCorpusCheck {
    pub path: String,
    pub records: usize,
    pub entries: Vec<LiteratureEntry>,
    pub full_text_snapshots: usize,
    pub abstract_snapshots: usize,
    pub metadata_snapshots: usize,
    pub issues: Vec<LiteratureIssue>,
}

impl LiteratureCorpusCheck {
    pub fn valid(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiteratureImportResult {
    pub added: bool,
    pub entry: LiteratureEntry,
    pub corpus: LiteratureCorpusCheck,
    pub source_manifest: SourceManifestCheck,
}

#[derive(serde::Deserialize)]
struct CrossrefEnvelope {
    message: CrossrefMessage,
}

#[derive(serde::Deserialize)]
struct CrossrefMessage {
    #[serde(default)]
    items: Vec<CrossrefWork>,
}

#[derive(serde::Deserialize)]
struct CrossrefWork {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default)]
    author: Vec<CrossrefAuthor>,
    #[serde(default)]
    published: Option<CrossrefDate>,
    #[serde(rename = "published-print", default)]
    published_print: Option<CrossrefDate>,
    #[serde(rename = "published-online", default)]
    published_online: Option<CrossrefDate>,
    #[serde(rename = "container-title", default)]
    container_title: Vec<String>,
    publisher: Option<String>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    #[serde(default)]
    link: Vec<CrossrefLink>,
}

#[derive(serde::Deserialize)]
struct CrossrefAuthor {
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
}

#[derive(serde::Deserialize)]
struct CrossrefDate {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<u32>>,
}

#[derive(serde::Deserialize)]
struct CrossrefLink {
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "content-type")]
    content_type: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn corpus_path(mission_id: &str) -> String {
    format!("{CORPUS_DIR}/{mission_id}.jsonl")
}

fn full_corpus_path(root: &Path, mission_id: &str) -> PathBuf {
    root.join(corpus_path(mission_id))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn snapshot_path_valid(value: &str) -> bool {
    let path = Path::new(value);
    let components = path.components().collect::<Vec<_>>();
    !path.is_absolute()
        && components.len() >= 3
        && components[0] == Component::Normal(OsStr::new("evidence"))
        && components[1] == Component::Normal(OsStr::new("snapshots"))
        && components
            .iter()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn normalized_doi(value: &str) -> String {
    crate::sources::canonical_source_id(value)
        .trim()
        .to_ascii_lowercase()
}

fn clean_markup(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    let decoded = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn year_of(work: &CrossrefWork) -> Option<u32> {
    [
        &work.published,
        &work.published_print,
        &work.published_online,
    ]
    .into_iter()
    .flatten()
    .find_map(|date| {
        date.date_parts
            .first()
            .and_then(|parts| parts.first())
            .copied()
    })
}

fn author_name(author: CrossrefAuthor) -> Option<String> {
    if let Some(name) = author.name.filter(|name| !name.trim().is_empty()) {
        return Some(name.trim().to_owned());
    }
    let name = [author.given, author.family]
        .into_iter()
        .flatten()
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!name.is_empty()).then_some(name)
}

fn parse_crossref(body: &str, query: &str, limit: usize) -> Result<LiteratureSearchResult, String> {
    let envelope: CrossrefEnvelope = serde_json::from_str(body)
        .map_err(|error| format!("Crossref response was invalid: {error}"))?;
    let mut works = Vec::new();
    let mut seen = HashSet::new();
    let mut duplicates_removed = 0;
    for item in envelope.message.items {
        let year = year_of(&item);
        let Some(doi) = item.doi.map(|doi| normalized_doi(&doi)) else {
            continue;
        };
        if !crate::sources::source_id_valid(&doi) {
            continue;
        }
        if !seen.insert(doi.clone()) {
            duplicates_removed += 1;
            continue;
        }
        let title = item
            .title
            .first()
            .map(|title| clean_markup(title))
            .filter(|title| !title.is_empty());
        let Some(title) = title else { continue };
        let landing_url = item
            .url
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .unwrap_or_else(|| format!("https://doi.org/{doi}"));
        let mut full_text_urls = item
            .link
            .into_iter()
            .filter(|link| {
                !link
                    .content_type
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains("pdf")
            })
            .filter_map(|link| link.url)
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .collect::<Vec<_>>();
        full_text_urls.sort();
        full_text_urls.dedup();
        works.push(LiteratureWork {
            doi,
            title,
            authors: item.author.into_iter().filter_map(author_name).collect(),
            year,
            container_title: item
                .container_title
                .first()
                .map(|value| clean_markup(value))
                .filter(|value| !value.is_empty()),
            publisher: item
                .publisher
                .map(|value| clean_markup(&value))
                .filter(|value| !value.is_empty()),
            landing_url,
            abstract_text: item
                .abstract_text
                .map(|value| clean_markup(&value))
                .filter(|value| !value.is_empty()),
            full_text_urls,
        });
        if works.len() >= limit {
            break;
        }
    }
    Ok(LiteratureSearchResult {
        provider: "crossref".into(),
        query: query.into(),
        returned_at: now(),
        works,
        duplicates_removed,
    })
}

fn evidence_mission(root: &Path, mission_id: &str) -> Result<(), String> {
    if !crate::missions::valid_mission_id(mission_id) {
        return Err("a valid missionId is required".into());
    }
    let mission = crate::missions::mission_at(root, mission_id)?;
    if !mission.status.accepts_work() || mission.kind == MissionKind::StudyLaunch {
        return Err("literature capture requires an active evidence mission".into());
    }
    Ok(())
}

pub fn search(
    env: &Env,
    mission_id: &str,
    query: &str,
    limit: usize,
) -> Result<LiteratureSearchResult, String> {
    let root = workspace_dir(env)?;
    evidence_mission(&root, mission_id)?;
    let query = query.trim();
    if !(2..=300).contains(&query.chars().count()) {
        return Err("query must contain 2 to 300 characters".into());
    }
    let limit = limit.clamp(1, 25);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!(
            "HappyScience/{} (Crossref literature search)",
            env.version()
        ))
        .build()
        .map_err(|error| format!("literature client failed: {error}"))?;
    let response = client
        .get(CROSSREF_WORKS)
        .query(&[
            ("query.bibliographic", query),
            ("rows", &(limit * 2).to_string()),
        ])
        .send()
        .map_err(|error| format!("Crossref search failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Crossref search returned HTTP {}",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_SEARCH_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Crossref response could not be read: {error}"))?;
    if bytes.len() as u64 > MAX_SEARCH_BYTES {
        return Err("Crossref response exceeded the capture limit".into());
    }
    let body =
        String::from_utf8(bytes).map_err(|_| "Crossref response was not UTF-8".to_string())?;
    parse_crossref(&body, query, limit)
}

fn validate_work(work: &mut LiteratureWork) -> Result<(), String> {
    work.doi = normalized_doi(&work.doi);
    work.title = work.title.trim().to_owned();
    if !crate::sources::source_id_valid(&work.doi) {
        return Err("work DOI is invalid".into());
    }
    if work.title.is_empty() || work.title.chars().count() > 1_000 {
        return Err("work title must contain 1 to 1000 characters".into());
    }
    if !work.landing_url.starts_with("http://") && !work.landing_url.starts_with("https://") {
        return Err("work landingUrl must be HTTP(S)".into());
    }
    if work.authors.len() > 200 || work.full_text_urls.len() > 20 {
        return Err("work metadata exceeds capture limits".into());
    }
    if work
        .full_text_urls
        .iter()
        .any(|url| !url.starts_with("http://") && !url.starts_with("https://"))
    {
        return Err("fullTextUrls must contain only HTTP(S) URLs".into());
    }
    Ok(())
}

fn fetch_text(work: &LiteratureWork) -> Option<(String, String)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("HappyScience/0.5 (source snapshot)")
        .build()
        .ok()?;
    for url in &work.full_text_urls {
        let Ok(response) = client
            .get(url)
            .header(
                reqwest::header::ACCEPT,
                "text/plain, application/xml, text/xml, text/html, application/xhtml+xml",
            )
            .send()
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if content_type.contains("pdf") || content_type.contains("octet-stream") {
            continue;
        }
        let mut bytes = Vec::new();
        if response
            .take(MAX_CAPTURE_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.is_empty()
            || bytes.len() as u64 > MAX_CAPTURE_BYTES
        {
            continue;
        }
        let Ok(raw) = String::from_utf8(bytes) else {
            continue;
        };
        let text = if content_type.contains("html") || content_type.contains("xml") {
            clean_markup(&raw)
        } else {
            raw
        };
        if text.trim().chars().count() >= 200 {
            return Some((url.clone(), text));
        }
    }
    None
}

fn fallback_snapshot(work: &LiteratureWork) -> (SnapshotStatus, String, String) {
    if let Some(abstract_text) = work
        .abstract_text
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        return (
            SnapshotStatus::AbstractOnly,
            work.landing_url.clone(),
            abstract_text.trim().to_owned(),
        );
    }
    let metadata = format!(
        "Title: {}\nDOI: {}\nAuthors: {}\nYear: {}\nContainer: {}\nPublisher: {}\n",
        work.title,
        work.doi,
        work.authors.join("; "),
        work.year.map(|year| year.to_string()).unwrap_or_default(),
        work.container_title.as_deref().unwrap_or_default(),
        work.publisher.as_deref().unwrap_or_default(),
    );
    (
        SnapshotStatus::MetadataOnly,
        work.landing_url.clone(),
        metadata,
    )
}

pub(crate) fn check_at(root: &Path, mission_id: &str) -> LiteratureCorpusCheck {
    let path = corpus_path(mission_id);
    let file = match fs::File::open(root.join(&path)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LiteratureCorpusCheck {
                path,
                records: 0,
                entries: Vec::new(),
                full_text_snapshots: 0,
                abstract_snapshots: 0,
                metadata_snapshots: 0,
                issues: Vec::new(),
            }
        }
        Err(error) => {
            return LiteratureCorpusCheck {
                path,
                records: 0,
                entries: Vec::new(),
                full_text_snapshots: 0,
                abstract_snapshots: 0,
                metadata_snapshots: 0,
                issues: vec![LiteratureIssue {
                    line: 0,
                    message: format!("literature corpus could not be read: {error}"),
                }],
            }
        }
    };
    let mut entries = Vec::new();
    let mut issues = Vec::new();
    let mut ids = HashSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                issues.push(LiteratureIssue {
                    line: line_number,
                    message: format!("literature row could not be read: {error}"),
                });
                continue;
            }
        };
        let entry = match serde_json::from_str::<LiteratureEntry>(&line) {
            Ok(entry) => entry,
            Err(error) => {
                issues.push(LiteratureIssue {
                    line: line_number,
                    message: format!("invalid literature row: {error}"),
                });
                continue;
            }
        };
        if entry.schema_version != SCHEMA_VERSION
            || entry.mission_id != mission_id
            || entry.canonical_id != normalized_doi(&entry.work.doi)
            || !sha256_valid(&entry.sha256)
            || !snapshot_path_valid(&entry.snapshot_path)
        {
            issues.push(LiteratureIssue {
                line: line_number,
                message: "literature row violates the v1 corpus contract".into(),
            });
            continue;
        }
        if !ids.insert(entry.canonical_id.clone()) {
            issues.push(LiteratureIssue {
                line: line_number,
                message: format!("duplicate literature identity: {}", entry.canonical_id),
            });
            continue;
        }
        let snapshot = root.join(&entry.snapshot_path);
        let inside_workspace = fs::canonicalize(root)
            .ok()
            .zip(fs::canonicalize(&snapshot).ok())
            .is_some_and(|(workspace, snapshot)| snapshot.starts_with(workspace));
        match inside_workspace.then(|| fs::read(&snapshot)).transpose() {
            Ok(Some(bytes)) if !bytes.is_empty() && sha256(&bytes) == entry.sha256 => {}
            _ => {
                issues.push(LiteratureIssue {
                    line: line_number,
                    message: format!(
                        "snapshot is missing or has changed: {}",
                        entry.snapshot_path
                    ),
                });
                continue;
            }
        }
        entries.push(entry);
    }
    LiteratureCorpusCheck {
        path,
        records: entries.len(),
        full_text_snapshots: entries
            .iter()
            .filter(|entry| entry.snapshot_status == SnapshotStatus::FullText)
            .count(),
        abstract_snapshots: entries
            .iter()
            .filter(|entry| entry.snapshot_status == SnapshotStatus::AbstractOnly)
            .count(),
        metadata_snapshots: entries
            .iter()
            .filter(|entry| entry.snapshot_status == SnapshotStatus::MetadataOnly)
            .count(),
        entries,
        issues,
    }
}

pub fn capture(
    env: &Env,
    mission_id: &str,
    mut work: LiteratureWork,
) -> Result<LiteratureImportResult, String> {
    let root = workspace_dir(env)?;
    capture_at(&root, mission_id, &mut work)
}

fn capture_at(
    root: &Path,
    mission_id: &str,
    work: &mut LiteratureWork,
) -> Result<LiteratureImportResult, String> {
    evidence_mission(root, mission_id)?;
    validate_work(work)?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "literature corpus lock poisoned")?;
    let current = check_at(root, mission_id);
    if !current.valid() {
        return Err("the literature corpus is invalid and must be repaired before capture".into());
    }
    let canonical_id = normalized_doi(&work.doi);
    if let Some(entry) = current
        .entries
        .iter()
        .find(|entry| entry.canonical_id == canonical_id)
        .cloned()
    {
        let source_manifest =
            crate::sources::check_manifest_at(root, &crate::sources::manifest_path(mission_id))
                .check;
        return Ok(LiteratureImportResult {
            added: false,
            entry,
            corpus: current,
            source_manifest,
        });
    }

    let (snapshot_status, retrieved_url, text) = match fetch_text(work) {
        Some((url, text)) => (SnapshotStatus::FullText, url, text),
        None => fallback_snapshot(work),
    };
    let hash = sha256(text.as_bytes());
    let snapshot_path = format!("evidence/snapshots/crossref-{}.txt", &hash[..16]);
    let snapshot_full = root.join(&snapshot_path);
    fs::create_dir_all(snapshot_full.parent().expect("snapshot has a parent"))
        .map_err(|error| format!("snapshot directory failed: {error}"))?;
    if snapshot_full.exists() {
        let existing = fs::read(&snapshot_full)
            .map_err(|error| format!("existing snapshot could not be read: {error}"))?;
        if sha256(&existing) != hash {
            return Err("content-addressed snapshot collision".into());
        }
    } else {
        fs::write(&snapshot_full, text.as_bytes())
            .map_err(|error| format!("snapshot could not be written: {error}"))?;
    }
    let source_manifest = crate::sources::append_snapshot_at(
        root,
        mission_id,
        SourceSnapshot {
            schema_version: crate::sources::SCHEMA_VERSION,
            source_id: canonical_id.clone(),
            title: work.title.clone(),
            retrieved_url: retrieved_url.clone(),
            retrieved_at: now(),
            snapshot_path: snapshot_path.clone(),
            sha256: hash.clone(),
        },
    )?;
    let entry = LiteratureEntry {
        schema_version: SCHEMA_VERSION,
        mission_id: mission_id.into(),
        canonical_id,
        work: work.clone(),
        snapshot_status,
        retrieved_url,
        snapshot_path,
        sha256: hash,
        added_at: now(),
    };
    let path = full_corpus_path(root, mission_id);
    fs::create_dir_all(path.parent().expect("literature corpus has a parent"))
        .map_err(|error| format!("literature corpus directory failed: {error}"))?;
    let line = serde_json::to_string(&entry)
        .map_err(|error| format!("literature entry serialize failed: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("literature corpus open failed: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("literature corpus write failed: {error}"))?;
    Ok(LiteratureImportResult {
        added: true,
        entry,
        corpus: check_at(root, mission_id),
        source_manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::missions::RigorLevel;

    fn root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("happy-science-literature-{tag}-{}", now()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn mission(root: &Path) -> String {
        let plan = crate::missions::plan_mission_at(
            root,
            MissionKind::EvidenceSprint,
            RigorLevel::Research,
        )
        .unwrap();
        crate::missions::start_mission_at(root, &plan.mission.mission_id, "ses_1").unwrap();
        plan.mission.mission_id
    }

    #[test]
    fn normalizes_and_deduplicates_crossref_results() {
        let body = r#"{"message":{"items":[{"DOI":"10.1000/Test","title":["<b>Study A</b>"],"author":[{"given":"Ada","family":"Lovelace"}],"published":{"date-parts":[[2024]]},"URL":"https://doi.org/10.1000/Test"},{"DOI":"10.1000/test","title":["Duplicate"],"URL":"https://doi.org/10.1000/test"}]}}"#;
        let result = parse_crossref(body, "study", 10).unwrap();
        assert_eq!(result.works.len(), 1);
        assert_eq!(result.duplicates_removed, 1);
        assert_eq!(result.works[0].doi, "10.1000/test");
        assert_eq!(result.works[0].title, "Study A");
        assert_eq!(result.works[0].authors, vec!["Ada Lovelace"]);
    }

    #[test]
    fn captures_an_abstract_once_and_registers_a_verified_snapshot() {
        let root = root("capture");
        let mission_id = mission(&root);
        let mut work = LiteratureWork {
            doi: "https://doi.org/10.1000/example".into(),
            title: "Example Study".into(),
            authors: vec!["Researcher A".into()],
            year: Some(2024),
            container_title: Some("Journal".into()),
            publisher: None,
            landing_url: "https://doi.org/10.1000/example".into(),
            abstract_text: Some(
                "A sufficiently detailed abstract used for the immutable local source snapshot."
                    .into(),
            ),
            full_text_urls: Vec::new(),
        };
        let first = capture_at(&root, &mission_id, &mut work).unwrap();
        assert!(first.added);
        assert_eq!(first.entry.snapshot_status, SnapshotStatus::AbstractOnly);
        assert_eq!(first.corpus.records, 1);
        assert_eq!(first.source_manifest.verified_snapshots, 1);
        let duplicate = capture_at(&root, &mission_id, &mut work).unwrap();
        assert!(!duplicate.added);
        assert_eq!(duplicate.corpus.records, 1);
        assert_eq!(
            fs::read_to_string(root.join(duplicate.corpus.path))
                .unwrap()
                .lines()
                .count(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }
}
