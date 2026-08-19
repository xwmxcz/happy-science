//! Append-only, mission-scoped research decision records.
//!
//! Decisions are authored through the Happy Science kernel, not by the agent,
//! and remain immutable. A later decision may explicitly supersede an earlier
//! one, preserving the reasoning trail instead of rewriting history.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::workspace_dir;
use crate::Env;

const SCHEMA_VERSION: u32 = 1;
const STORE_DIR: &str = ".happy-science/decisions";
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchDecision {
    pub schema_version: u32,
    pub decision_id: String,
    pub mission_id: String,
    pub title: String,
    pub choice: String,
    pub rationale: String,
    pub alternatives: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    pub decided_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionLogIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionLogCheck {
    pub path: String,
    pub records: usize,
    pub decisions: Vec<ResearchDecision>,
    pub issues: Vec<DecisionLogIssue>,
}

impl DecisionLogCheck {
    pub fn valid(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewResearchDecision {
    pub title: String,
    pub choice: String,
    pub rationale: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub impact: Option<String>,
    #[serde(default)]
    pub supersedes: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn decision_id() -> String {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    format!(
        "hsd_{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn valid_decision_id(value: &str) -> bool {
    value.strip_prefix("hsd_").is_some_and(|suffix| {
        suffix.len() == 16
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

pub fn log_path(mission_id: &str) -> String {
    format!("{STORE_DIR}/{mission_id}.jsonl")
}

fn full_path(root: &Path, mission_id: &str) -> PathBuf {
    root.join(log_path(mission_id))
}

fn bounded(value: &str, name: &str, max: usize) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(format!("{name} must contain 1 to {max} characters"));
    }
    Ok(value.to_owned())
}

fn optional_bounded(
    value: Option<String>,
    name: &str,
    max: usize,
) -> Result<Option<String>, String> {
    value.map(|value| bounded(&value, name, max)).transpose()
}

pub(crate) fn check_at(root: &Path, mission_id: &str) -> DecisionLogCheck {
    let path = log_path(mission_id);
    let file = match fs::File::open(root.join(&path)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return DecisionLogCheck {
                path,
                records: 0,
                decisions: Vec::new(),
                issues: Vec::new(),
            }
        }
        Err(error) => {
            return DecisionLogCheck {
                path,
                records: 0,
                decisions: Vec::new(),
                issues: vec![DecisionLogIssue {
                    line: 0,
                    message: format!("decision log could not be read: {error}"),
                }],
            }
        }
    };
    let mut decisions = Vec::new();
    let mut issues = Vec::new();
    let mut ids = HashSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                issues.push(DecisionLogIssue {
                    line: line_number,
                    message: format!("decision row could not be read: {error}"),
                });
                continue;
            }
        };
        let decision = match serde_json::from_str::<ResearchDecision>(&line) {
            Ok(decision) => decision,
            Err(error) => {
                issues.push(DecisionLogIssue {
                    line: line_number,
                    message: format!("invalid decision row: {error}"),
                });
                continue;
            }
        };
        let invalid = decision.schema_version != SCHEMA_VERSION
            || decision.mission_id != mission_id
            || !valid_decision_id(&decision.decision_id)
            || decision.title.trim().is_empty()
            || decision.choice.trim().is_empty()
            || decision.rationale.trim().is_empty();
        if invalid {
            issues.push(DecisionLogIssue {
                line: line_number,
                message: "decision row violates the v1 mission contract".into(),
            });
            continue;
        }
        if !ids.insert(decision.decision_id.clone()) {
            issues.push(DecisionLogIssue {
                line: line_number,
                message: format!("duplicate decisionId: {}", decision.decision_id),
            });
            continue;
        }
        if let Some(supersedes) = &decision.supersedes {
            if !ids.contains(supersedes) {
                issues.push(DecisionLogIssue {
                    line: line_number,
                    message: format!(
                        "supersedes does not reference an earlier decision: {supersedes}"
                    ),
                });
                continue;
            }
        }
        decisions.push(decision);
    }
    DecisionLogCheck {
        path,
        records: decisions.len(),
        decisions,
        issues,
    }
}

pub fn record(
    env: &Env,
    mission_id: &str,
    input: NewResearchDecision,
) -> Result<DecisionLogCheck, String> {
    let root = workspace_dir(env)?;
    record_at(&root, mission_id, input)
}

pub(crate) fn record_at(
    root: &Path,
    mission_id: &str,
    input: NewResearchDecision,
) -> Result<DecisionLogCheck, String> {
    if !crate::missions::valid_mission_id(mission_id) {
        return Err("a valid missionId is required".into());
    }
    let mission = crate::missions::mission_at(root, mission_id)?;
    if !mission.status.accepts_work() {
        return Err("only an active mission can receive research decisions".into());
    }
    let title = bounded(&input.title, "title", 160)?;
    let choice = bounded(&input.choice, "choice", 400)?;
    let rationale = bounded(&input.rationale, "rationale", 2_000)?;
    if input.alternatives.len() > 8 {
        return Err("alternatives may contain at most 8 entries".into());
    }
    let alternatives = input
        .alternatives
        .iter()
        .map(|value| bounded(value, "each alternative", 300))
        .collect::<Result<Vec<_>, _>>()?;
    let unique = alternatives.iter().collect::<HashSet<_>>();
    if unique.len() != alternatives.len() {
        return Err("alternatives must be unique".into());
    }
    let impact = optional_bounded(input.impact, "impact", 1_000)?;
    let supersedes = optional_bounded(input.supersedes, "supersedes", 20)?;

    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "decision log lock poisoned")?;
    let current = check_at(root, mission_id);
    if !current.valid() {
        return Err(
            "the existing decision log is invalid and must be repaired before appending".into(),
        );
    }
    if let Some(previous) = &supersedes {
        if !current
            .decisions
            .iter()
            .any(|decision| &decision.decision_id == previous)
        {
            return Err("supersedes must reference an existing decision in this mission".into());
        }
    }
    let decision = ResearchDecision {
        schema_version: SCHEMA_VERSION,
        decision_id: decision_id(),
        mission_id: mission_id.to_owned(),
        title,
        choice,
        rationale,
        alternatives,
        impact,
        supersedes,
        decided_at: now(),
    };
    let path = full_path(root, mission_id);
    fs::create_dir_all(path.parent().expect("decision store has a parent"))
        .map_err(|error| format!("decision log directory failed: {error}"))?;
    let line = serde_json::to_string(&decision)
        .map_err(|error| format!("decision serialize failed: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("decision log open failed: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("decision log write failed: {error}"))?;
    Ok(check_at(root, mission_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::missions::{MissionKind, RigorLevel};

    fn root(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("happy-science-decisions-{tag}-{}", decision_id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn input(title: &str) -> NewResearchDecision {
        NewResearchDecision {
            title: title.into(),
            choice: "Use the conservative estimator".into(),
            rationale: "It matches the registered estimand and tolerates missingness.".into(),
            alternatives: vec!["Complete-case estimator".into()],
            impact: Some("The confidence interval may be wider.".into()),
            supersedes: None,
        }
    }

    #[test]
    fn appends_immutable_mission_decisions_and_allows_explicit_supersession() {
        let root = root("append");
        let plan =
            crate::missions::plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Research)
                .unwrap();
        crate::missions::start_mission_at(&root, &plan.mission.mission_id, "ses_1").unwrap();

        let first = record_at(&root, &plan.mission.mission_id, input("Primary estimator")).unwrap();
        assert_eq!(first.records, 1);
        let mut replacement = input("Revised estimator");
        replacement.supersedes = Some(first.decisions[0].decision_id.clone());
        let second = record_at(&root, &plan.mission.mission_id, replacement).unwrap();
        assert_eq!(second.records, 2);
        assert_eq!(
            second.decisions[1].supersedes.as_deref(),
            Some(first.decisions[0].decision_id.as_str())
        );
        assert_eq!(
            fs::read_to_string(root.join(second.path))
                .unwrap()
                .lines()
                .count(),
            2
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_weak_or_out_of_mission_decisions() {
        let root = root("validation");
        let plan =
            crate::missions::plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Research)
                .unwrap();
        crate::missions::start_mission_at(&root, &plan.mission.mission_id, "ses_1").unwrap();
        let mut invalid = input(" ");
        invalid.alternatives = vec!["same".into(), "same".into()];
        assert!(record_at(&root, &plan.mission.mission_id, invalid).is_err());
        assert!(record_at(&root, "../../escape", input("Choice")).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
