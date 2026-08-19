//! Derives deterministic, fingerprinted scientific claim states from evidence, sources, and human review.
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimStatus {
    ReviewPending,
    Supported,
    Contested,
    Contradicted,
    Qualified,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimPassport {
    pub schema_version: u32,
    pub claim_id: String,
    pub claim: String,
    pub status: ClaimStatus,
    pub supports: usize,
    pub contradicts: usize,
    pub qualifies: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub needs_review: usize,
    pub unreviewed: usize,
    pub source_count: usize,
    pub verified_sources: usize,
    pub fingerprint: String,
}

fn stance_name(stance: crate::evidence::EvidenceStance) -> &'static str {
    match stance {
        crate::evidence::EvidenceStance::Supports => "supports",
        crate::evidence::EvidenceStance::Contradicts => "contradicts",
        crate::evidence::EvidenceStance::Qualifies => "qualifies",
    }
}

fn verdict_name(verdict: crate::adjudication::EvidenceVerdict) -> &'static str {
    match verdict {
        crate::adjudication::EvidenceVerdict::Accepted => "accepted",
        crate::adjudication::EvidenceVerdict::Rejected => "rejected",
        crate::adjudication::EvidenceVerdict::NeedsReview => "needs-review",
    }
}

fn fingerprint_field(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

pub fn build(
    ledger: &crate::evidence::EvidenceLedgerCheck,
    sources: &crate::sources::SourceManifestCheck,
    review: &crate::adjudication::EvidenceReviewCheck,
) -> Vec<ClaimPassport> {
    let decisions = review
        .decisions
        .iter()
        .map(|decision| (decision.evidence_id.as_str(), decision))
        .collect::<HashMap<_, _>>();
    let verified_source_ids = sources
        .verified_source_ids
        .iter()
        .map(|source_id| crate::sources::canonical_source_id(source_id))
        .collect::<HashSet<_>>();
    let source_hashes = sources
        .entries
        .iter()
        .map(|source| {
            (
                crate::sources::canonical_source_id(&source.source_id),
                source.sha256.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut groups = BTreeMap::<&str, Vec<&crate::evidence::EvidenceRecord>>::new();
    for entry in &ledger.entries {
        groups.entry(&entry.claim_id).or_default().push(entry);
    }

    groups
        .into_iter()
        .map(|(claim_id, mut entries)| {
            entries.sort_by(|a, b| a.evidence_id.cmp(&b.evidence_id));
            let claim = entries
                .first()
                .map(|entry| entry.claim.as_str())
                .unwrap_or_default();
            let mut supports = 0;
            let mut contradicts = 0;
            let mut qualifies = 0;
            let mut accepted = 0;
            let mut rejected = 0;
            let mut needs_review = 0;
            let mut unreviewed = 0;
            let mut accepted_supports = 0;
            let mut accepted_contradicts = 0;
            let mut accepted_qualifies = 0;
            let mut source_ids = HashSet::new();
            let mut hasher = Sha256::new();
            fingerprint_field(&mut hasher, &SCHEMA_VERSION.to_string());
            fingerprint_field(&mut hasher, claim_id);
            fingerprint_field(&mut hasher, claim);

            for entry in entries {
                match entry.stance {
                    crate::evidence::EvidenceStance::Supports => supports += 1,
                    crate::evidence::EvidenceStance::Contradicts => contradicts += 1,
                    crate::evidence::EvidenceStance::Qualifies => qualifies += 1,
                }
                let source_id = crate::sources::canonical_source_id(&entry.source.id);
                source_ids.insert(source_id.clone());
                let decision = decisions.get(entry.evidence_id.as_str()).copied();
                match decision.map(|decision| decision.verdict) {
                    Some(crate::adjudication::EvidenceVerdict::Accepted) => {
                        accepted += 1;
                        match entry.stance {
                            crate::evidence::EvidenceStance::Supports => accepted_supports += 1,
                            crate::evidence::EvidenceStance::Contradicts => {
                                accepted_contradicts += 1
                            }
                            crate::evidence::EvidenceStance::Qualifies => accepted_qualifies += 1,
                        }
                    }
                    Some(crate::adjudication::EvidenceVerdict::Rejected) => rejected += 1,
                    Some(crate::adjudication::EvidenceVerdict::NeedsReview) => needs_review += 1,
                    None => unreviewed += 1,
                }
                fingerprint_field(&mut hasher, &entry.evidence_id);
                fingerprint_field(&mut hasher, stance_name(entry.stance));
                fingerprint_field(&mut hasher, &source_id);
                fingerprint_field(&mut hasher, &entry.source.title);
                fingerprint_field(&mut hasher, &entry.source.locator);
                fingerprint_field(&mut hasher, &entry.source.quote);
                fingerprint_field(
                    &mut hasher,
                    source_hashes.get(&source_id).copied().unwrap_or_default(),
                );
                if let Some(decision) = decision {
                    fingerprint_field(&mut hasher, verdict_name(decision.verdict));
                    fingerprint_field(&mut hasher, &decision.note);
                } else {
                    fingerprint_field(&mut hasher, "unreviewed");
                }
            }

            let status = if unreviewed > 0 || needs_review > 0 {
                ClaimStatus::ReviewPending
            } else if accepted_supports > 0 && accepted_contradicts > 0 {
                ClaimStatus::Contested
            } else if accepted_contradicts > 0 {
                ClaimStatus::Contradicted
            } else if accepted_supports > 0 {
                ClaimStatus::Supported
            } else if accepted_qualifies > 0 {
                ClaimStatus::Qualified
            } else {
                ClaimStatus::Unsupported
            };
            let verified_sources = source_ids
                .iter()
                .filter(|source_id| verified_source_ids.contains(*source_id))
                .count();
            ClaimPassport {
                schema_version: SCHEMA_VERSION,
                claim_id: claim_id.to_owned(),
                claim: claim.to_owned(),
                status,
                supports,
                contradicts,
                qualifies,
                accepted,
                rejected,
                needs_review,
                unreviewed,
                source_count: source_ids.len(),
                verified_sources,
                fingerprint: format!("{:x}", hasher.finalize()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        evidence_id: &str,
        stance: crate::evidence::EvidenceStance,
        source_id: &str,
    ) -> crate::evidence::EvidenceRecord {
        crate::evidence::EvidenceRecord {
            schema_version: 1,
            evidence_id: evidence_id.into(),
            claim_id: "cl_1".into(),
            claim: "The intervention changes the primary outcome.".into(),
            stance,
            source: crate::evidence::EvidenceSource {
                id: source_id.into(),
                title: format!("Study {evidence_id}"),
                locator: "p. 4".into(),
                quote: format!("Excerpt {evidence_id}"),
            },
        }
    }

    fn ledger() -> crate::evidence::EvidenceLedgerCheck {
        crate::evidence::EvidenceLedgerCheck {
            path: "evidence/test.claims.jsonl".into(),
            entries: vec![
                entry(
                    "ev_1",
                    crate::evidence::EvidenceStance::Supports,
                    "10.1000/a",
                ),
                entry(
                    "ev_2",
                    crate::evidence::EvidenceStance::Contradicts,
                    "10.1000/b",
                ),
            ],
            records: 2,
            claims: 1,
            sources: 2,
            supports: 1,
            contradicts: 1,
            qualifies: 0,
            contested_claim_ids: vec!["cl_1".into()],
            qualified_only_claim_ids: Vec::new(),
            issues: Vec::new(),
        }
    }

    fn sources() -> crate::sources::SourceManifestCheck {
        crate::sources::SourceManifestCheck {
            path: "evidence/test.sources.jsonl".into(),
            entries: ["a", "b"]
                .into_iter()
                .map(|suffix| crate::sources::SourceSnapshot {
                    schema_version: 1,
                    source_id: format!("10.1000/{suffix}"),
                    title: format!("Study ev_{suffix}"),
                    retrieved_url: format!("https://doi.org/10.1000/{suffix}"),
                    retrieved_at: 1,
                    snapshot_path: format!("evidence/snapshots/{suffix}.txt"),
                    sha256: suffix.repeat(64),
                })
                .collect(),
            records: 2,
            verified_snapshots: 2,
            verified_source_ids: vec!["10.1000/a".into(), "10.1000/b".into()],
            quote_matches: 2,
            issues: Vec::new(),
        }
    }

    fn review(
        decisions: Vec<crate::adjudication::EvidenceDecision>,
    ) -> crate::adjudication::EvidenceReviewCheck {
        crate::adjudication::EvidenceReviewCheck {
            path: "evidence/test.reviews.jsonl".into(),
            records: decisions.len(),
            decisions,
            accepted: 0,
            rejected: 0,
            needs_review: 0,
            unreviewed_evidence_ids: Vec::new(),
            issues: Vec::new(),
        }
    }

    fn decision(
        evidence_id: &str,
        verdict: crate::adjudication::EvidenceVerdict,
    ) -> crate::adjudication::EvidenceDecision {
        crate::adjudication::EvidenceDecision {
            schema_version: 1,
            mission_id: "hsm_test".into(),
            evidence_id: evidence_id.into(),
            verdict,
            note: if verdict == crate::adjudication::EvidenceVerdict::Accepted {
                String::new()
            } else {
                "Reviewer rationale".into()
            },
            decided_at: 1,
        }
    }

    #[test]
    fn derives_reviewed_claim_states_without_an_ai_score() {
        let pending = build(&ledger(), &sources(), &review(Vec::new()));
        assert_eq!(pending[0].status, ClaimStatus::ReviewPending);
        assert_eq!(pending[0].unreviewed, 2);
        assert_eq!(pending[0].verified_sources, 2);

        let contested = build(
            &ledger(),
            &sources(),
            &review(vec![
                decision("ev_1", crate::adjudication::EvidenceVerdict::Accepted),
                decision("ev_2", crate::adjudication::EvidenceVerdict::Accepted),
            ]),
        );
        assert_eq!(contested[0].status, ClaimStatus::Contested);
        assert_ne!(pending[0].fingerprint, contested[0].fingerprint);

        let supported = build(
            &ledger(),
            &sources(),
            &review(vec![
                decision("ev_1", crate::adjudication::EvidenceVerdict::Accepted),
                decision("ev_2", crate::adjudication::EvidenceVerdict::Rejected),
            ]),
        );
        assert_eq!(supported[0].status, ClaimStatus::Supported);
        assert_eq!(supported[0].fingerprint.len(), 64);

        let contradicted = build(
            &ledger(),
            &sources(),
            &review(vec![
                decision("ev_1", crate::adjudication::EvidenceVerdict::Rejected),
                decision("ev_2", crate::adjudication::EvidenceVerdict::Accepted),
            ]),
        );
        assert_eq!(contradicted[0].status, ClaimStatus::Contradicted);

        let unsupported = build(
            &ledger(),
            &sources(),
            &review(vec![
                decision("ev_1", crate::adjudication::EvidenceVerdict::Rejected),
                decision("ev_2", crate::adjudication::EvidenceVerdict::Rejected),
            ]),
        );
        assert_eq!(unsupported[0].status, ClaimStatus::Unsupported);

        let mut qualified_ledger = ledger();
        qualified_ledger.entries.truncate(1);
        qualified_ledger.entries[0].stance = crate::evidence::EvidenceStance::Qualifies;
        let qualified = build(
            &qualified_ledger,
            &sources(),
            &review(vec![decision(
                "ev_1",
                crate::adjudication::EvidenceVerdict::Accepted,
            )]),
        );
        assert_eq!(qualified[0].status, ClaimStatus::Qualified);
    }

    #[test]
    fn fingerprint_is_stable_for_the_same_scientific_state() {
        let review = review(vec![
            decision("ev_1", crate::adjudication::EvidenceVerdict::Accepted),
            decision("ev_2", crate::adjudication::EvidenceVerdict::Rejected),
        ]);
        assert_eq!(
            build(&ledger(), &sources(), &review),
            build(&ledger(), &sources(), &review)
        );
    }
}
