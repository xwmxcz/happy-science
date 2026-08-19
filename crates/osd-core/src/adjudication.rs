//! Owns append-only human decisions over claim–evidence relations without mutating source records.
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_NOTE_CHARS: usize = 2_000;
static REVIEW_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceVerdict {
    Accepted,
    Rejected,
    NeedsReview,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceDecision {
    pub schema_version: u32,
    pub mission_id: String,
    pub evidence_id: String,
    pub verdict: EvidenceVerdict,
    pub note: String,
    pub decided_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceReviewIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceReviewCheck {
    pub path: String,
    pub records: usize,
    pub decisions: Vec<EvidenceDecision>,
    pub accepted: usize,
    pub rejected: usize,
    pub needs_review: usize,
    pub unreviewed_evidence_ids: Vec<String>,
    pub issues: Vec<EvidenceReviewIssue>,
}

impl EvidenceReviewCheck {
    pub fn complete(&self) -> bool {
        self.unreviewed_evidence_ids.is_empty() && self.issues.is_empty()
    }
}

pub fn review_path(mission_id: &str) -> String {
    format!("evidence/{mission_id}.reviews.jsonl")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn decision_valid(verdict: EvidenceVerdict, note: &str) -> Result<(), String> {
    let note = note.trim();
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(format!("note must be at most {MAX_NOTE_CHARS} characters"));
    }
    if verdict != EvidenceVerdict::Accepted && note.is_empty() {
        return Err("a note is required when evidence is rejected or needs review".into());
    }
    Ok(())
}

fn empty_check(mission_id: &str) -> EvidenceReviewCheck {
    EvidenceReviewCheck {
        path: review_path(mission_id),
        records: 0,
        decisions: Vec::new(),
        accepted: 0,
        rejected: 0,
        needs_review: 0,
        unreviewed_evidence_ids: Vec::new(),
        issues: Vec::new(),
    }
}

fn read_at(root: &Path, mission_id: &str, evidence_ids: &HashSet<String>) -> EvidenceReviewCheck {
    let mut check = empty_check(mission_id);
    let file = match fs::File::open(root.join(&check.path)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            check.unreviewed_evidence_ids = evidence_ids.iter().cloned().collect();
            check.unreviewed_evidence_ids.sort();
            return check;
        }
        Err(error) => {
            check.issues.push(EvidenceReviewIssue {
                line: 0,
                message: format!("review log cannot be read: {error}"),
            });
            return check;
        }
    };
    let mut latest = HashMap::<String, EvidenceDecision>::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        let line = match line {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => continue,
            Err(error) => {
                check.issues.push(EvidenceReviewIssue {
                    line: line_number,
                    message: format!("line cannot be read: {error}"),
                });
                continue;
            }
        };
        let decision = match serde_json::from_str::<EvidenceDecision>(&line) {
            Ok(decision) => decision,
            Err(error) => {
                check.issues.push(EvidenceReviewIssue {
                    line: line_number,
                    message: format!("invalid review record: {error}"),
                });
                continue;
            }
        };
        check.records += 1;
        if decision.schema_version != SCHEMA_VERSION {
            check.issues.push(EvidenceReviewIssue {
                line: line_number,
                message: format!("schemaVersion must be {SCHEMA_VERSION}"),
            });
            continue;
        }
        if decision.mission_id != mission_id {
            check.issues.push(EvidenceReviewIssue {
                line: line_number,
                message: "missionId does not match the review log".into(),
            });
            continue;
        }
        if !evidence_ids.contains(&decision.evidence_id) {
            check.issues.push(EvidenceReviewIssue {
                line: line_number,
                message: format!("unknown evidenceId: {}", decision.evidence_id),
            });
            continue;
        }
        if decision.decided_at == 0 {
            check.issues.push(EvidenceReviewIssue {
                line: line_number,
                message: "decidedAt must be a Unix timestamp".into(),
            });
            continue;
        }
        if let Err(message) = decision_valid(decision.verdict, &decision.note) {
            check.issues.push(EvidenceReviewIssue {
                line: line_number,
                message,
            });
            continue;
        }
        latest.insert(decision.evidence_id.clone(), decision);
    }
    check.decisions = latest.into_values().collect();
    check
        .decisions
        .sort_by(|a, b| a.evidence_id.cmp(&b.evidence_id));
    for decision in &check.decisions {
        match decision.verdict {
            EvidenceVerdict::Accepted => check.accepted += 1,
            EvidenceVerdict::Rejected => check.rejected += 1,
            EvidenceVerdict::NeedsReview => check.needs_review += 1,
        }
    }
    check.unreviewed_evidence_ids = evidence_ids
        .iter()
        .filter(|evidence_id| !latest_contains(&check.decisions, evidence_id))
        .cloned()
        .collect();
    check.unreviewed_evidence_ids.sort();
    check
}

fn latest_contains(decisions: &[EvidenceDecision], evidence_id: &str) -> bool {
    decisions
        .binary_search_by_key(&evidence_id, |decision| decision.evidence_id.as_str())
        .is_ok()
}

pub(crate) fn check_at(
    root: &Path,
    mission_id: &str,
    evidence_ids: &HashSet<String>,
) -> Result<EvidenceReviewCheck, String> {
    let _guard = REVIEW_LOCK
        .lock()
        .map_err(|_| "evidence review lock poisoned")?;
    Ok(read_at(root, mission_id, evidence_ids))
}

pub(crate) fn decide_at(
    root: &Path,
    mission_id: &str,
    evidence_ids: &HashSet<String>,
    evidence_id: &str,
    verdict: EvidenceVerdict,
    note: &str,
) -> Result<EvidenceReviewCheck, String> {
    let evidence_id = evidence_id.trim();
    if !evidence_ids.contains(evidence_id) {
        return Err(format!("unknown evidenceId: {evidence_id}"));
    }
    decision_valid(verdict, note)?;
    let decision = EvidenceDecision {
        schema_version: SCHEMA_VERSION,
        mission_id: mission_id.to_owned(),
        evidence_id: evidence_id.to_owned(),
        verdict,
        note: note.trim().to_owned(),
        decided_at: now(),
    };
    let _guard = REVIEW_LOCK
        .lock()
        .map_err(|_| "evidence review lock poisoned")?;
    let path = root.join(review_path(mission_id));
    let parent = path.parent().expect("evidence review log has a parent");
    fs::create_dir_all(parent).map_err(|error| format!("review directory failed: {error}"))?;
    let line = serde_json::to_string(&decision)
        .map_err(|error| format!("review serialize failed: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("review log open failed: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("review log write failed: {error}"))?;
    Ok(read_at(root, mission_id, evidence_ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "happy-science-review-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn appends_decisions_and_keeps_only_the_latest_per_evidence() {
        let root = root("latest");
        let ids = HashSet::from(["ev_1".to_owned(), "ev_2".to_owned()]);
        let first = decide_at(
            &root,
            "hsm_test",
            &ids,
            "ev_1",
            EvidenceVerdict::NeedsReview,
            "Check the sample definition",
        )
        .unwrap();
        assert_eq!(first.needs_review, 1);
        assert_eq!(first.unreviewed_evidence_ids, ["ev_2"]);

        let latest = decide_at(
            &root,
            "hsm_test",
            &ids,
            "ev_1",
            EvidenceVerdict::Accepted,
            "",
        )
        .unwrap();
        assert_eq!(latest.records, 2);
        assert_eq!(latest.decisions.len(), 1);
        assert_eq!(latest.accepted, 1);
        assert!(latest.complete() == false);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unknown_evidence_and_unexplained_negative_decisions() {
        let root = root("invalid");
        let ids = HashSet::from(["ev_1".to_owned()]);
        let unknown = decide_at(
            &root,
            "hsm_test",
            &ids,
            "ev_missing",
            EvidenceVerdict::Accepted,
            "",
        )
        .unwrap_err();
        assert!(unknown.contains("unknown evidenceId"));
        let unexplained = decide_at(
            &root,
            "hsm_test",
            &ids,
            "ev_1",
            EvidenceVerdict::Rejected,
            "",
        )
        .unwrap_err();
        assert!(unexplained.contains("note is required"));
        assert!(!root.join(review_path("hsm_test")).exists());
        let _ = fs::remove_dir_all(root);
    }
}
