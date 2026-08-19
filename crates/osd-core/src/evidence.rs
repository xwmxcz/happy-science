//! Owns the local Claim–Evidence Ledger schema, validation, and contestation graph.
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

const LEDGER_DIR: &str = "evidence";
pub const SCHEMA_VERSION: u32 = 1;

pub fn ledger_path(mission_id: &str) -> String {
    format!("{LEDGER_DIR}/{mission_id}.claims.jsonl")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceStance {
    Supports,
    Contradicts,
    Qualifies,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceSource {
    pub id: String,
    pub title: String,
    pub locator: String,
    pub quote: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceRecord {
    pub schema_version: u32,
    pub evidence_id: String,
    pub claim_id: String,
    pub claim: String,
    pub stance: EvidenceStance,
    pub source: EvidenceSource,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLedgerCheck {
    pub path: String,
    pub entries: Vec<EvidenceRecord>,
    pub records: usize,
    pub claims: usize,
    pub sources: usize,
    pub supports: usize,
    pub contradicts: usize,
    pub qualifies: usize,
    pub contested_claim_ids: Vec<String>,
    pub qualified_only_claim_ids: Vec<String>,
    pub issues: Vec<EvidenceIssue>,
}

impl EvidenceLedgerCheck {
    pub fn valid(&self) -> bool {
        self.records > 0 && self.issues.is_empty()
    }
}

fn source_id_valid(value: &str) -> bool {
    crate::sources::source_id_valid(value)
}

fn required(line: usize, name: &str, value: &str, issues: &mut Vec<EvidenceIssue>) {
    if value.trim().is_empty() {
        issues.push(EvidenceIssue {
            line,
            message: format!("{name} is required"),
        });
    }
}

#[derive(Default)]
struct ClaimRelations {
    supports: bool,
    contradicts: bool,
    qualifies: bool,
}

#[cfg(test)]
pub(crate) fn check_ledger_at(root: &Path, relative: &str) -> EvidenceLedgerCheck {
    check_ledger_with_sources_at(root, relative, None)
}

pub(crate) fn check_bundle_at(
    root: &Path,
    ledger_relative: &str,
    manifest_relative: &str,
) -> (EvidenceLedgerCheck, crate::sources::SourceManifestCheck) {
    let mut sources = crate::sources::check_manifest_at(root, manifest_relative);
    let ledger = check_ledger_with_sources_at(root, ledger_relative, Some(&mut sources));
    (ledger, sources.check)
}

fn check_ledger_with_sources_at(
    root: &Path,
    relative: &str,
    mut sources: Option<&mut crate::sources::CheckedSources>,
) -> EvidenceLedgerCheck {
    let mut check = EvidenceLedgerCheck {
        path: relative.into(),
        entries: Vec::new(),
        records: 0,
        claims: 0,
        sources: 0,
        supports: 0,
        contradicts: 0,
        qualifies: 0,
        contested_claim_ids: Vec::new(),
        qualified_only_claim_ids: Vec::new(),
        issues: Vec::new(),
    };
    let file = match File::open(root.join(relative)) {
        Ok(file) => file,
        Err(error) => {
            check.issues.push(EvidenceIssue {
                line: 0,
                message: format!("ledger cannot be read: {error}"),
            });
            return check;
        }
    };
    let mut evidence_ids = HashSet::new();
    let mut claim_texts = HashMap::<String, String>::new();
    let mut source_titles = HashMap::<String, String>::new();
    let mut claim_relations = HashMap::<String, ClaimRelations>::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        let line = match line {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => continue,
            Err(error) => {
                check.issues.push(EvidenceIssue {
                    line: line_number,
                    message: format!("line cannot be read: {error}"),
                });
                continue;
            }
        };
        let record = match serde_json::from_str::<EvidenceRecord>(&line) {
            Ok(record) => record,
            Err(error) => {
                check.issues.push(EvidenceIssue {
                    line: line_number,
                    message: format!("invalid evidence record: {error}"),
                });
                continue;
            }
        };
        check.records += 1;
        check.entries.push(record.clone());
        match record.stance {
            EvidenceStance::Supports => check.supports += 1,
            EvidenceStance::Contradicts => check.contradicts += 1,
            EvidenceStance::Qualifies => check.qualifies += 1,
        }
        if record.schema_version != SCHEMA_VERSION {
            check.issues.push(EvidenceIssue {
                line: line_number,
                message: format!("schemaVersion must be {SCHEMA_VERSION}"),
            });
        }
        required(
            line_number,
            "evidenceId",
            &record.evidence_id,
            &mut check.issues,
        );
        required(line_number, "claimId", &record.claim_id, &mut check.issues);
        required(line_number, "claim", &record.claim, &mut check.issues);
        required(
            line_number,
            "source.title",
            &record.source.title,
            &mut check.issues,
        );
        required(
            line_number,
            "source.locator",
            &record.source.locator,
            &mut check.issues,
        );
        required(
            line_number,
            "source.quote",
            &record.source.quote,
            &mut check.issues,
        );
        if !source_id_valid(&record.source.id) {
            check.issues.push(EvidenceIssue {
                line: line_number,
                message: "source.id must be a DOI or HTTP(S) URL".into(),
            });
        }
        if !record.evidence_id.trim().is_empty() && !evidence_ids.insert(record.evidence_id) {
            check.issues.push(EvidenceIssue {
                line: line_number,
                message: "evidenceId must be unique".into(),
            });
        }
        let claim_id = record.claim_id.trim();
        let claim = record.claim.trim();
        if !claim_id.is_empty() {
            let relations = claim_relations.entry(claim_id.to_owned()).or_default();
            match record.stance {
                EvidenceStance::Supports => relations.supports = true,
                EvidenceStance::Contradicts => relations.contradicts = true,
                EvidenceStance::Qualifies => relations.qualifies = true,
            }
        }
        if !claim_id.is_empty() && !claim.is_empty() {
            if let Some(existing) = claim_texts.get(claim_id) {
                if existing != claim {
                    check.issues.push(EvidenceIssue {
                        line: line_number,
                        message: "claimId must always map to the same claim text".into(),
                    });
                }
            } else {
                claim_texts.insert(claim_id.to_owned(), claim.to_owned());
            }
        }
        let source_id = crate::sources::canonical_source_id(&record.source.id);
        let source_title = record.source.title.trim();
        if !source_id.is_empty() && !source_title.is_empty() {
            if let Some(existing) = source_titles.get(&source_id) {
                if existing != source_title {
                    check.issues.push(EvidenceIssue {
                        line: line_number,
                        message: "source.id must always map to the same source title".into(),
                    });
                }
            } else {
                source_titles.insert(source_id, source_title.to_owned());
            }
        }
        if let Some(checked) = sources.as_deref_mut() {
            crate::sources::verify_quote(
                checked,
                line_number,
                &record.source.id,
                &record.source.title,
                &record.source.quote,
            );
        }
    }
    if check.records == 0 && check.issues.is_empty() {
        check.issues.push(EvidenceIssue {
            line: 0,
            message: "ledger must contain at least one evidence record".into(),
        });
    }
    check.claims = claim_texts.len();
    check.sources = source_titles.len();
    check.contested_claim_ids = claim_relations
        .iter()
        .filter(|(_, relations)| relations.supports && relations.contradicts)
        .map(|(claim_id, _)| claim_id.clone())
        .collect();
    check.qualified_only_claim_ids = claim_relations
        .iter()
        .filter(|(_, relations)| {
            relations.qualifies && !relations.supports && !relations.contradicts
        })
        .map(|(claim_id, _)| claim_id.clone())
        .collect();
    check.contested_claim_ids.sort();
    check.qualified_only_claim_ids.sort();
    check
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "happy-science-evidence-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("evidence")).unwrap();
        root
    }

    #[test]
    fn accepts_support_contradiction_and_qualification_records() {
        let root = root("valid");
        let rows = [
            r#"{"schemaVersion":1,"evidenceId":"ev_1","claimId":"cl_1","claim":"A","stance":"supports","source":{"id":"10.1000/test","title":"Study A","locator":"p. 4","quote":"supporting excerpt"}}"#,
            r#"{"schemaVersion":1,"evidenceId":"ev_2","claimId":"cl_1","claim":"A","stance":"contradicts","source":{"id":"https://example.org/paper","title":"Study B","locator":"Table 2","quote":"contradicting excerpt"}}"#,
            r#"{"schemaVersion":1,"evidenceId":"ev_3","claimId":"cl_1","claim":"A","stance":"qualifies","source":{"id":"doi:10.1000/limit","title":"Study C","locator":"Discussion","quote":"qualifying excerpt"}}"#,
        ];
        let path = ledger_path("hsm_test");
        fs::write(root.join(&path), rows.join("\n")).unwrap();

        let check = check_ledger_at(&root, &path);
        assert!(check.valid());
        assert_eq!(
            (check.supports, check.contradicts, check.qualifies),
            (1, 1, 1)
        );
        assert_eq!((check.claims, check.sources), (1, 3));
        assert_eq!(check.entries.len(), 3);
        assert_eq!(check.entries[0].claim, "A");
        assert_eq!(check.contested_claim_ids, vec!["cl_1"]);
        assert!(check.qualified_only_claim_ids.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_untraceable_and_duplicate_evidence() {
        let root = root("invalid");
        let row = r#"{"schemaVersion":1,"evidenceId":"ev_1","claimId":"cl_1","claim":"A","stance":"supports","source":{"id":"paper 1","title":"Study","locator":"","quote":""}}"#;
        let path = ledger_path("hsm_test");
        fs::write(root.join(&path), format!("{row}\n{row}\n")).unwrap();

        let check = check_ledger_at(&root, &path);
        assert!(!check.valid());
        assert!(check
            .issues
            .iter()
            .any(|issue| issue.message.contains("DOI")));
        assert!(check
            .issues
            .iter()
            .any(|issue| issue.message.contains("unique")));
        assert!(check
            .issues
            .iter()
            .any(|issue| issue.message.contains("locator")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_identity_drift_and_marks_qualified_only_claims() {
        let root = root("identity-drift");
        let rows = [
            r#"{"schemaVersion":1,"evidenceId":"ev_1","claimId":"cl_1","claim":"Stable claim","stance":"qualifies","source":{"id":"10.1000/test","title":"Study A","locator":"p. 4","quote":"first excerpt"}}"#,
            r#"{"schemaVersion":1,"evidenceId":"ev_2","claimId":"cl_1","claim":"Changed claim","stance":"qualifies","source":{"id":"10.1000/test","title":"Different title","locator":"p. 5","quote":"second excerpt"}}"#,
        ];
        let path = ledger_path("hsm_test");
        fs::write(root.join(&path), rows.join("\n")).unwrap();

        let check = check_ledger_at(&root, &path);
        assert!(!check.valid());
        assert_eq!(check.qualified_only_claim_ids, vec!["cl_1"]);
        assert!(check
            .issues
            .iter()
            .any(|issue| issue.message.contains("same claim text")));
        assert!(check
            .issues
            .iter()
            .any(|issue| issue.message.contains("same source title")));
        let _ = fs::remove_dir_all(root);
    }
}
