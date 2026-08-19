//! Happy Science mission kernel: owns scientific task contracts and their persisted lifecycle.
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::capabilities::ResearchCapability;
use crate::runtime::workspace_dir;
use crate::Env;

const STORE_DIR: &str = ".happy-science";
const STORE_FILE: &str = "missions.jsonl";
pub const CONTRACT_VERSION: u32 = 5;
const MIGRATABLE_CONTRACT_VERSION: u32 = 4;
static STORE_LOCK: Mutex<()> = Mutex::new(());
const PROTOCOL_PATH: &str = "research/protocol.md";
const PROTOCOL_APPROVAL_TITLE: &str = "Protocol approval before outcomes";
const PROTOCOL_APPROVAL_PREFIX: &str = "Approved protocol SHA-256 ";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissionKind {
    StudyLaunch,
    EvidenceSprint,
    ReproductionChallenge,
    ManuscriptStressTest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RigorLevel {
    Explore,
    Research,
    Publication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissionStatus {
    Planned,
    Running,
    WaitingForInput,
    WaitingForApproval,
    Paused,
    Interrupted,
    ReviewReady,
    Completed,
    Failed,
    Cancelled,
}

impl MissionStatus {
    pub const fn accepts_work(self) -> bool {
        matches!(
            self,
            Self::Running | Self::WaitingForInput | Self::WaitingForApproval | Self::ReviewReady
        )
    }

    const fn checkable(self) -> bool {
        self.accepts_work() || matches!(self, Self::Paused | Self::Interrupted)
    }

    const fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissionAction {
    WaitForInput,
    WaitForApproval,
    Pause,
    Interrupt,
    Resume,
    Fail,
    Cancel,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateStatus {
    Pending,
    Passed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityGate {
    pub key: String,
    pub status: GateStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissionActionOwner {
    Agent,
    Researcher,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionNextAction {
    pub key: String,
    pub owner: MissionActionOwner,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionRunIntegrity {
    pub runs_checked: usize,
    pub attention_runs: usize,
    pub aligned_runs: usize,
    pub no_plan_runs: usize,
    pub findings: Vec<crate::research_integrity::RunIntegrityFinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionRecord {
    pub contract_version: u32,
    pub mission_id: String,
    pub kind: MissionKind,
    pub rigor: RigorLevel,
    pub status: MissionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub deliverables: Vec<String>,
    pub quality_gates: Vec<QualityGate>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionPlan {
    pub mission: MissionRecord,
    pub prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionCheck {
    pub mission: MissionRecord,
    pub ready_for_review: bool,
    pub missing_deliverables: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ledger: Option<crate::evidence::EvidenceLedgerCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_manifest: Option<crate::sources::SourceManifestCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_review: Option<crate::adjudication::EvidenceReviewCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_passports: Option<Vec<crate::claim_passport::ClaimPassport>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literature_corpus: Option<crate::literature::LiteratureCorpusCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<ProtocolCheck>,
    pub decision_log: crate::decisions::DecisionLogCheck,
    pub issues: Vec<String>,
    pub next_actions: Vec<MissionNextAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_integrity: Option<MissionRunIntegrity>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolCheck {
    pub path: String,
    pub present: bool,
    pub complete: bool,
    pub unresolved_items: usize,
    pub unchecked_items: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub approved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceDecisionResult {
    pub review: crate::adjudication::EvidenceReviewCheck,
    pub claim_passports: Vec<crate::claim_passport::ClaimPassport>,
}

struct MissionContract {
    base_prompt: &'static str,
    capabilities: &'static [ResearchCapability],
    deliverables: &'static [&'static str],
    gates: &'static [&'static str],
    evidence_ledger: bool,
}

fn contract(kind: MissionKind) -> MissionContract {
    match kind {
        MissionKind::StudyLaunch => MissionContract {
            base_prompt: "Turn my research question into a decision-ready, preregistration-style protocol before running any analysis. Ask for missing population or sample, study design, exposure or intervention, outcomes, confounders, and practical constraints. Define hypotheses, estimands, variables, inclusion and exclusion rules, primary and secondary endpoints, the analysis model, missing-data and multiplicity handling, power or sensitivity assumptions, stopping rules, risks, and a decision log. Mark every unresolved field exactly `[TBD]` and remove each marker only after it is resolved. Do not inspect outcomes or run the analysis until Happy Science records my approval of the current protocol.",
            capabilities: &[],
            deliverables: &[PROTOCOL_PATH],
            gates: &["protocol-complete", "approval-before-outcomes"],
            evidence_ledger: false,
        },
        MissionKind::EvidenceSprint => MissionContract {
            base_prompt: "Build an auditable literature evidence map for the research question I provide. Ask for a missing question, date range, databases, or inclusion and exclusion criteria before searching. Use the required bundled capabilities listed below; never invent or silently repair a citation. Separate reported evidence, uncertainty, and inference, and report every source that cannot be verified.",
            capabilities: &[
                ResearchCapability::LiteratureSurvey,
                ResearchCapability::TraceabilityReview,
            ],
            deliverables: &["literature/search_log.md", "literature/evidence.csv", "literature/synthesis.md"],
            gates: &["claim-evidence-valid"],
            evidence_ledger: true,
        },
        MissionKind::ReproductionChallenge => MissionContract {
            base_prompt: "Reproduce one quantitative claim from the paper, report, repository, or data in this workspace. Ask which claim to target if ambiguous and never overwrite original files. Record the claim, source paths, input hashes, environment, and commands. Run an independent implementation, compare the original and reproduced values against an explicit tolerance, and never declare success when an input or method detail is unavailable.",
            capabilities: &[],
            deliverables: &["reproduction/manifest.json", "reproduction/comparison.csv", "reproduction/report.md"],
            gates: &["claim-evidence-valid"],
            evidence_ledger: true,
        },
        MissionKind::ManuscriptStressTest => MissionContract {
            base_prompt: "Red-team the report or manuscript in this workspace. Ask which document to audit if ambiguous. Resolve every citation, flag each number with no traceable source, check figures against the code that generated them, rank material integrity findings, and propose a repair for every material issue.",
            capabilities: &[
                ResearchCapability::TraceabilityReview,
                ResearchCapability::IntegrityAudit,
            ],
            deliverables: &["audit/findings.md", "audit/traceability.csv", "audit/repair_plan.md"],
            gates: &["claim-evidence-valid"],
            evidence_ledger: true,
        },
    }
}

fn rigor_contract(rigor: RigorLevel) -> &'static str {
    match rigor {
        RigorLevel::Explore => "Time-box the work for orientation. Clearly mark every unverified source, assumption, and provisional conclusion, then name the evidence needed for a stronger answer.",
        RigorLevel::Research => "Verify sources and citations, execute analysis reproducibly, preserve source-to-claim traceability, and stop for approval at consequential decisions.",
        RigorLevel::Publication => "Meet the research-grade standard, then perform an independent second-pass review of citations, statistics, figures, and claim traceability. Remove or explicitly qualify every unsupported claim.",
    }
}

/** The mission kernel owns the ordered gate contract. Only requirements with a
 * deterministic checker belong here; qualitative rigor remains prompt policy. */
fn quality_gate_keys(kind: MissionKind) -> Vec<&'static str> {
    let mission = contract(kind);
    mission
        .gates
        .iter()
        .copied()
        .chain(std::iter::once("deliverables-present"))
        .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn mission_id() -> String {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    format!(
        "hsm_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

pub(crate) fn valid_mission_id(value: &str) -> bool {
    value.strip_prefix("hsm_").is_some_and(|suffix| {
        suffix.len() == 16
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn store_path(root: &Path) -> PathBuf {
    root.join(STORE_DIR).join(STORE_FILE)
}

fn append(root: &Path, record: &MissionRecord) -> Result<(), String> {
    let path = store_path(root);
    let dir = path.parent().expect("mission store has a parent");
    fs::create_dir_all(dir).map_err(|e| format!("mission store directory failed: {e}"))?;
    let line =
        serde_json::to_string(record).map_err(|e| format!("mission serialize failed: {e}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("mission store open failed: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("mission store write failed: {e}"))
}

fn latest_at(root: &Path) -> Result<Vec<MissionRecord>, String> {
    let path = store_path(root);
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("mission store read failed: {e}")),
    };
    let mut records = HashMap::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        let Ok(mut record) = serde_json::from_str::<MissionRecord>(&line) else {
            continue;
        };
        if record.contract_version == MIGRATABLE_CONTRACT_VERSION {
            record.contract_version = CONTRACT_VERSION;
        }
        if record.contract_version == CONTRACT_VERSION && valid_mission_id(&record.mission_id) {
            records.insert(record.mission_id.clone(), record);
        }
    }
    let mut values: Vec<_> = records.into_values().collect();
    values.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.mission_id.cmp(&a.mission_id))
    });
    Ok(values)
}

pub(crate) fn mission_at(root: &Path, mission_id: &str) -> Result<MissionRecord, String> {
    latest_at(root)?
        .into_iter()
        .find(|record| record.mission_id == mission_id)
        .ok_or_else(|| format!("unknown mission: {mission_id}"))
}

fn compile_prompt(record: &MissionRecord) -> String {
    let c = contract(record.kind);
    let rigor = rigor_contract(record.rigor);
    let deliverables = record
        .deliverables
        .iter()
        .map(|path| format!("- {path}"))
        .collect::<Vec<_>>()
        .join("\n");
    let gates = record
        .quality_gates
        .iter()
        .map(|gate| format!("- {}", gate.key))
        .collect::<Vec<_>>()
        .join("\n");
    let capabilities = if c.capabilities.is_empty() {
        String::new()
    } else {
        let rows = c
            .capabilities
            .iter()
            .map(|capability| {
                let spec = capability.spec();
                format!("- `{}` — {}", spec.skill_name, spec.purpose)
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\n\nRequired bundled capabilities:\n{rows}\nRead each named skill before using it. If a capability is unavailable, stop and report the missing skill instead of silently substituting an unverified workflow."
        )
    };
    let ledger = if c.evidence_ledger {
        let ledger_path = crate::evidence::ledger_path(&record.mission_id);
        let manifest_path = crate::sources::manifest_path(&record.mission_id);
        format!(
            "\n\nClaim–Evidence Graph contract:\nWrite `{}` as JSONL: one JSON object per line and no Markdown fences. Every material claim needs one or more evidence rows. Preserve conflicting evidence instead of averaging it away. Reuse each claimId with exactly the same claim text and each source.id with exactly the same source title; Happy Science derives the contestation graph from those stable identities. Use this exact schema:\n{{\"schemaVersion\":1,\"evidenceId\":\"ev_unique\",\"claimId\":\"cl_stable\",\"claim\":\"the material claim\",\"stance\":\"supports|contradicts|qualifies\",\"source\":{{\"id\":\"DOI or HTTP(S) URL\",\"title\":\"source title\",\"locator\":\"page, section, figure, or table\",\"quote\":\"exact supporting excerpt\"}}}}\n\nSource Snapshot contract:\nFor every source, save the retrieved UTF-8 source text under `evidence/snapshots/`. Copy each evidence quote exactly from that snapshot. Write `{}` as JSONL using this exact schema:\n{{\"schemaVersion\":1,\"sourceId\":\"same DOI or URL as the evidence row\",\"title\":\"same source title as the evidence row\",\"retrievedUrl\":\"final HTTP(S) retrieval URL\",\"retrievedAt\":1700000000,\"snapshotPath\":\"evidence/snapshots/source.txt\",\"sha256\":\"64 lowercase hexadecimal characters\"}}\nThe kernel recomputes every snapshot hash and rejects any quote that is not an exact substring of its snapshot.",
            ledger_path, manifest_path
        )
    } else {
        String::new()
    };
    let decision_log = crate::decisions::log_path(&record.mission_id);
    format!(
        "Happy Science mission `{}` (contract v{}).\n\n{}{}\n\nRigor standard:\n{}\n\nRequired deliverables:\n{}\n\nQuality gates — do not report the mission complete until each gate has evidence:\n{}{}\n\nResearch Decision Log:\nConsequential choices are recorded by the researcher through Happy Science at `{}`. Treat that append-only log as authoritative; never edit or replace it from the agent runtime.",
        record.mission_id,
        record.contract_version,
        c.base_prompt,
        capabilities,
        rigor,
        deliverables,
        gates,
        ledger,
        decision_log
    )
}

pub fn plan_mission(
    env: &Env,
    kind: MissionKind,
    rigor: RigorLevel,
) -> Result<MissionPlan, String> {
    let root = workspace_dir(env)?;
    plan_mission_at(&root, kind, rigor)
}

pub(crate) fn plan_mission_at(
    root: &Path,
    kind: MissionKind,
    rigor: RigorLevel,
) -> Result<MissionPlan, String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "mission store lock poisoned")?;
    let c = contract(kind);
    let created = now();
    let mission_id = mission_id();
    let mut deliverables = c
        .deliverables
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    if c.evidence_ledger {
        deliverables.push(crate::evidence::ledger_path(&mission_id));
        deliverables.push(crate::sources::manifest_path(&mission_id));
    }
    let mission = MissionRecord {
        contract_version: CONTRACT_VERSION,
        mission_id,
        kind,
        rigor,
        status: MissionStatus::Planned,
        status_reason: None,
        session_id: None,
        deliverables,
        quality_gates: quality_gate_keys(kind)
            .into_iter()
            .map(|key| QualityGate {
                key: key.to_owned(),
                status: GateStatus::Pending,
            })
            .collect(),
        created_at: created,
        updated_at: created,
    };
    append(root, &mission)?;
    let prompt = compile_prompt(&mission);
    Ok(MissionPlan { mission, prompt })
}

pub fn start_mission(
    env: &Env,
    mission_id: &str,
    session_id: &str,
) -> Result<MissionRecord, String> {
    let root = workspace_dir(env)?;
    start_mission_at(&root, mission_id, session_id)
}

pub(crate) fn start_mission_at(
    root: &Path,
    mission_id: &str,
    session_id: &str,
) -> Result<MissionRecord, String> {
    if !valid_mission_id(mission_id) || session_id.trim().is_empty() {
        return Err("a valid missionId and sessionId are required".into());
    }
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "mission store lock poisoned")?;
    let mut record = latest_at(root)?
        .into_iter()
        .find(|record| record.mission_id == mission_id)
        .ok_or_else(|| format!("unknown mission: {mission_id}"))?;
    if record.status != MissionStatus::Planned || record.session_id.is_some() {
        return Err("only a planned mission can start".into());
    }
    record.status = MissionStatus::Running;
    record.status_reason = None;
    record.session_id = Some(session_id.trim().to_owned());
    record.updated_at = now();
    append(root, &record)?;
    Ok(record)
}

fn transition_allowed(status: MissionStatus, action: MissionAction) -> bool {
    if status.terminal() {
        return false;
    }
    match action {
        MissionAction::WaitForInput | MissionAction::WaitForApproval => {
            status == MissionStatus::Running
        }
        MissionAction::Pause | MissionAction::Interrupt => matches!(
            status,
            MissionStatus::Running
                | MissionStatus::WaitingForInput
                | MissionStatus::WaitingForApproval
                | MissionStatus::ReviewReady
        ),
        MissionAction::Resume => matches!(
            status,
            MissionStatus::WaitingForInput
                | MissionStatus::WaitingForApproval
                | MissionStatus::Paused
                | MissionStatus::Interrupted
                | MissionStatus::ReviewReady
        ),
        MissionAction::Fail => status != MissionStatus::Planned,
        MissionAction::Cancel => true,
        MissionAction::Complete => status == MissionStatus::ReviewReady,
    }
}

fn action_status(action: MissionAction) -> MissionStatus {
    match action {
        MissionAction::WaitForInput => MissionStatus::WaitingForInput,
        MissionAction::WaitForApproval => MissionStatus::WaitingForApproval,
        MissionAction::Pause => MissionStatus::Paused,
        MissionAction::Interrupt => MissionStatus::Interrupted,
        MissionAction::Resume => MissionStatus::Running,
        MissionAction::Fail => MissionStatus::Failed,
        MissionAction::Cancel => MissionStatus::Cancelled,
        MissionAction::Complete => MissionStatus::Completed,
    }
}

pub fn transition_mission(
    env: &Env,
    mission_id: &str,
    action: MissionAction,
    reason: Option<&str>,
) -> Result<MissionRecord, String> {
    if !valid_mission_id(mission_id) {
        return Err("a valid missionId is required".into());
    }
    let root = workspace_dir(env)?;
    if action == MissionAction::Complete && !check_mission_at(&root, mission_id)?.ready_for_review {
        return Err("a mission can complete only after every quality gate passes".into());
    }
    transition_mission_at(&root, mission_id, action, reason)
}

fn transition_mission_at(
    root: &Path,
    mission_id: &str,
    action: MissionAction,
    reason: Option<&str>,
) -> Result<MissionRecord, String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "mission store lock poisoned")?;
    let mut record = latest_at(root)?
        .into_iter()
        .find(|record| record.mission_id == mission_id)
        .ok_or_else(|| format!("unknown mission: {mission_id}"))?;
    if !transition_allowed(record.status, action) {
        return Err(format!(
            "mission action {action:?} is not allowed from status {:?}",
            record.status
        ));
    }
    record.status = action_status(action);
    record.status_reason = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    record.updated_at = now();
    append(root, &record)?;
    Ok(record)
}

pub fn list_missions(env: &Env) -> Result<Vec<MissionRecord>, String> {
    let root = workspace_dir(env)?;
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "mission store lock poisoned")?;
    latest_at(&root)
}

/// Run the deterministic gate owned by the kernel. Semantic gates remain
/// pending until a later reviewer records evidence; missing or empty required
/// artifacts can never be waved through by the executor's prose response.
pub fn check_mission(env: &Env, mission_id: &str) -> Result<MissionCheck, String> {
    if !valid_mission_id(mission_id) {
        return Err("a valid missionId is required".into());
    }
    let root = workspace_dir(env)?;
    check_mission_at(&root, mission_id)
}

pub fn decide_evidence(
    env: &Env,
    mission_id: &str,
    evidence_id: &str,
    verdict: crate::adjudication::EvidenceVerdict,
    note: &str,
) -> Result<EvidenceDecisionResult, String> {
    if !valid_mission_id(mission_id) {
        return Err("a valid missionId is required".into());
    }
    let root = workspace_dir(env)?;
    decide_evidence_at(&root, mission_id, evidence_id, verdict, note)
}

fn decide_evidence_at(
    root: &Path,
    mission_id: &str,
    evidence_id: &str,
    verdict: crate::adjudication::EvidenceVerdict,
    note: &str,
) -> Result<EvidenceDecisionResult, String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "mission store lock poisoned")?;
    let record = latest_at(root)?
        .into_iter()
        .find(|record| record.mission_id == mission_id)
        .ok_or_else(|| format!("unknown mission: {mission_id}"))?;
    if !record.status.accepts_work() || !contract(record.kind).evidence_ledger {
        return Err("only an active evidence mission can be reviewed".into());
    }
    let (ledger, sources) = crate::evidence::check_bundle_at(
        root,
        &crate::evidence::ledger_path(&record.mission_id),
        &crate::sources::manifest_path(&record.mission_id),
    );
    let evidence_ids = ledger
        .entries
        .iter()
        .map(|entry| entry.evidence_id.clone())
        .collect::<HashSet<_>>();
    let review = crate::adjudication::decide_at(
        root,
        mission_id,
        &evidence_ids,
        evidence_id,
        verdict,
        note,
    )?;
    let claim_passports = crate::claim_passport::build(&ledger, &sources, &review);
    Ok(EvidenceDecisionResult {
        review,
        claim_passports,
    })
}

fn protocol_check_at(
    root: &Path,
    decision_log: &crate::decisions::DecisionLogCheck,
) -> ProtocolCheck {
    let bytes = match fs::read(root.join(PROTOCOL_PATH)) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => {
            return ProtocolCheck {
                path: PROTOCOL_PATH.into(),
                present: false,
                complete: false,
                unresolved_items: 0,
                unchecked_items: 0,
                sha256: None,
                approved: false,
            }
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    let unresolved_items = text.match_indices("[TBD").count();
    let unchecked_items = text
        .lines()
        .filter(|line| line.trim_start().starts_with("- [ ]"))
        .count();
    // A protocol is a substantive preregistration artifact, not a placeholder
    // file that happens to satisfy deliverables-present.
    let complete = bytes.len() >= 800 && unresolved_items == 0 && unchecked_items == 0;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let expected_choice = format!("{PROTOCOL_APPROVAL_PREFIX}{sha256}");
    let approved = complete
        && decision_log.decisions.iter().any(|decision| {
            decision.title == PROTOCOL_APPROVAL_TITLE && decision.choice == expected_choice
        });
    ProtocolCheck {
        path: PROTOCOL_PATH.into(),
        present: true,
        complete,
        unresolved_items,
        unchecked_items,
        sha256: Some(sha256),
        approved,
    }
}

fn set_gate(record: &mut MissionRecord, key: &str, passed: bool) -> Result<(), String> {
    let gate = record
        .quality_gates
        .iter_mut()
        .find(|gate| gate.key == key)
        .ok_or_else(|| format!("mission contract is missing the {key} gate"))?;
    gate.status = if passed {
        GateStatus::Passed
    } else {
        GateStatus::Pending
    };
    Ok(())
}

fn next_actions(
    record: &MissionRecord,
    missing_deliverables: &[String],
    protocol: Option<&ProtocolCheck>,
    evidence_review: Option<&crate::adjudication::EvidenceReviewCheck>,
    run_integrity: Option<&MissionRunIntegrity>,
    issues: &[String],
) -> Vec<MissionNextAction> {
    let action = |key: &str, owner, target: Option<String>| MissionNextAction {
        key: key.into(),
        owner,
        target,
    };
    match record.status {
        MissionStatus::Paused | MissionStatus::Interrupted => {
            return vec![action(
                "resume-mission",
                MissionActionOwner::Researcher,
                None,
            )]
        }
        MissionStatus::WaitingForInput => {
            return vec![action(
                "answer-question",
                MissionActionOwner::Researcher,
                None,
            )]
        }
        MissionStatus::WaitingForApproval => {
            return vec![action(
                "review-approval",
                MissionActionOwner::Researcher,
                None,
            )]
        }
        MissionStatus::ReviewReady => {
            return vec![action(
                "complete-mission",
                MissionActionOwner::Researcher,
                None,
            )]
        }
        MissionStatus::Planned => {
            return vec![action(
                "start-mission",
                MissionActionOwner::Researcher,
                None,
            )]
        }
        MissionStatus::Completed | MissionStatus::Failed | MissionStatus::Cancelled => {
            return Vec::new()
        }
        MissionStatus::Running => {}
    }

    let mut actions = Vec::new();
    if let Some(protocol) = protocol {
        if !protocol.present {
            actions.push(action(
                "create-deliverable",
                MissionActionOwner::Agent,
                Some(protocol.path.clone()),
            ));
        } else if !protocol.complete {
            actions.push(action(
                "resolve-protocol",
                MissionActionOwner::Agent,
                Some(protocol.path.clone()),
            ));
        } else if !protocol.approved {
            actions.push(action(
                "approve-protocol",
                MissionActionOwner::Researcher,
                Some(protocol.path.clone()),
            ));
        }
    }
    for path in missing_deliverables.iter().take(3) {
        if !actions
            .iter()
            .any(|item| item.target.as_ref() == Some(path))
        {
            actions.push(action(
                "create-deliverable",
                MissionActionOwner::Agent,
                Some(path.clone()),
            ));
        }
    }
    if let Some(review) = evidence_review {
        if !review.unreviewed_evidence_ids.is_empty() {
            actions.push(action(
                "review-evidence",
                MissionActionOwner::Researcher,
                Some(review.unreviewed_evidence_ids.len().to_string()),
            ));
        }
    }
    if let Some(integrity) = run_integrity {
        if integrity.attention_runs > 0 {
            actions.push(action(
                "review-run-integrity",
                MissionActionOwner::Researcher,
                Some(integrity.findings.len().to_string()),
            ));
        }
    }
    if actions.is_empty() && !issues.is_empty() {
        actions.push(action(
            "repair-evidence",
            MissionActionOwner::Agent,
            issues.first().cloned(),
        ));
    }
    actions
}

fn mission_run_integrity(root: &Path, session_id: Option<&str>) -> Option<MissionRunIntegrity> {
    let session_id = session_id?;
    let runs = crate::runs::read_runs(root)
        .into_iter()
        .filter(|run| run.session_id.as_deref() == Some(session_id))
        .collect::<Vec<_>>();
    if runs.is_empty() {
        return None;
    }
    let mut summary = MissionRunIntegrity {
        runs_checked: 0,
        attention_runs: 0,
        aligned_runs: 0,
        no_plan_runs: 0,
        findings: Vec::new(),
    };
    for integrity in runs.into_iter().filter_map(|run| run.integrity) {
        summary.runs_checked += 1;
        match integrity.status.as_str() {
            "attention" => summary.attention_runs += 1,
            "aligned" => summary.aligned_runs += 1,
            "no-plan" => summary.no_plan_runs += 1,
            _ => {}
        }
        summary.findings.extend(integrity.findings);
    }
    (summary.runs_checked > 0).then_some(summary)
}

fn check_mission_at(root: &Path, mission_id: &str) -> Result<MissionCheck, String> {
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "mission store lock poisoned")?;
    let mut record = latest_at(root)?
        .into_iter()
        .find(|record| record.mission_id == mission_id)
        .ok_or_else(|| format!("unknown mission: {mission_id}"))?;
    if !record.status.checkable() {
        return Err("only an active, paused, or interrupted mission can be checked".into());
    }
    // Contract v4 removes non-applicable execution gates from Study Launch.
    // Rebuild from the owner on every check so persisted v3 missions migrate
    // instead of remaining permanently stuck on decorative pending gates.
    let prior = record
        .quality_gates
        .iter()
        .map(|gate| (gate.key.clone(), gate.status))
        .collect::<HashMap<_, _>>();
    record.contract_version = CONTRACT_VERSION;
    record.quality_gates = quality_gate_keys(record.kind)
        .into_iter()
        .map(|key| QualityGate {
            key: key.into(),
            status: prior.get(key).copied().unwrap_or(GateStatus::Pending),
        })
        .collect();
    let missing_deliverables = record
        .deliverables
        .iter()
        .filter(|relative| {
            fs::metadata(root.join(relative))
                .map(|metadata| !metadata.is_file() || metadata.len() == 0)
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();
    let (evidence_ledger, source_manifest) = if contract(record.kind).evidence_ledger {
        let path = crate::evidence::ledger_path(&record.mission_id);
        let manifest_path = crate::sources::manifest_path(&record.mission_id);
        let (ledger, sources) = crate::evidence::check_bundle_at(root, &path, &manifest_path);
        (Some(ledger), Some(sources))
    } else {
        (None, None)
    };
    let evidence_valid = evidence_ledger
        .as_ref()
        .map(crate::evidence::EvidenceLedgerCheck::valid)
        .unwrap_or(true);
    let sources_valid = source_manifest
        .as_ref()
        .map(crate::sources::SourceManifestCheck::valid)
        .unwrap_or(true);
    let evidence_review = if let Some(ledger) = &evidence_ledger {
        let evidence_ids = ledger
            .entries
            .iter()
            .map(|entry| entry.evidence_id.clone())
            .collect::<HashSet<_>>();
        Some(crate::adjudication::check_at(
            root,
            mission_id,
            &evidence_ids,
        )?)
    } else {
        None
    };
    let claim_passports = match (&evidence_ledger, &source_manifest, &evidence_review) {
        (Some(ledger), Some(sources), Some(review)) => {
            Some(crate::claim_passport::build(ledger, sources, review))
        }
        _ => None,
    };
    let literature_corpus = contract(record.kind)
        .evidence_ledger
        .then(|| crate::literature::check_at(root, mission_id));
    let literature_valid = literature_corpus
        .as_ref()
        .map(crate::literature::LiteratureCorpusCheck::valid)
        .unwrap_or(true);
    let decision_log = crate::decisions::check_at(root, mission_id);
    let run_integrity = mission_run_integrity(root, record.session_id.as_deref());
    let protocol =
        (record.kind == MissionKind::StudyLaunch).then(|| protocol_check_at(root, &decision_log));
    set_gate(
        &mut record,
        "deliverables-present",
        missing_deliverables.is_empty(),
    )?;
    if let Some(protocol) = &protocol {
        set_gate(&mut record, "protocol-complete", protocol.complete)?;
        set_gate(&mut record, "approval-before-outcomes", protocol.approved)?;
    }
    if let Some(ledger) = &evidence_ledger {
        set_gate(
            &mut record,
            "claim-evidence-valid",
            ledger.valid() && sources_valid,
        )?;
    }
    let mut issues = missing_deliverables
        .iter()
        .map(|path| format!("missing required artifact: {path}"))
        .collect::<Vec<_>>();
    if let Some(ledger) = &evidence_ledger {
        issues.extend(
            ledger
                .issues
                .iter()
                .map(|issue| format!("{}:{}: {}", ledger.path, issue.line, issue.message)),
        );
    }
    if let Some(sources) = &source_manifest {
        issues.extend(
            sources
                .issues
                .iter()
                .map(|issue| format!("{}:{}: {}", sources.path, issue.line, issue.message)),
        );
    }
    if let Some(corpus) = &literature_corpus {
        issues.extend(
            corpus
                .issues
                .iter()
                .map(|issue| format!("{}:{}: {}", corpus.path, issue.line, issue.message)),
        );
    }
    issues.extend(
        decision_log
            .issues
            .iter()
            .map(|issue| format!("{}:{}: {}", decision_log.path, issue.line, issue.message)),
    );
    if let Some(protocol) = &protocol {
        if protocol.present && !protocol.complete {
            issues.push(format!(
                "protocol is incomplete: {} unresolved TBD markers and {} unchecked items",
                protocol.unresolved_items, protocol.unchecked_items
            ));
        }
        if protocol.complete && !protocol.approved {
            issues.push("the current protocol has not been approved before outcomes".into());
        }
    }
    let ready_for_review = missing_deliverables.is_empty()
        && evidence_valid
        && sources_valid
        && literature_valid
        && decision_log.valid()
        && record
            .quality_gates
            .iter()
            .all(|gate| gate.status == GateStatus::Passed);
    if ready_for_review
        && matches!(
            record.status,
            MissionStatus::Running
                | MissionStatus::WaitingForInput
                | MissionStatus::WaitingForApproval
        )
    {
        record.status = MissionStatus::ReviewReady;
        record.status_reason = Some("All deterministic quality gates passed".into());
    } else if !ready_for_review && record.status == MissionStatus::ReviewReady {
        record.status = MissionStatus::Running;
        record.status_reason = Some("A quality gate changed and requires more work".into());
    }
    let next_actions = next_actions(
        &record,
        &missing_deliverables,
        protocol.as_ref(),
        evidence_review.as_ref(),
        run_integrity.as_ref(),
        &issues,
    );
    record.updated_at = now();
    append(root, &record)?;
    Ok(MissionCheck {
        mission: record,
        ready_for_review,
        missing_deliverables,
        evidence_ledger,
        source_manifest,
        evidence_review,
        claim_passports,
        literature_corpus,
        protocol,
        decision_log,
        issues,
        next_actions,
        run_integrity,
    })
}

pub fn approve_protocol(env: &Env, mission_id: &str) -> Result<MissionCheck, String> {
    let root = workspace_dir(env)?;
    approve_protocol_at(&root, mission_id)
}

fn approve_protocol_at(root: &Path, mission_id: &str) -> Result<MissionCheck, String> {
    let checked = check_mission_at(root, mission_id)?;
    if checked.mission.kind != MissionKind::StudyLaunch {
        return Err("only a Study Launch mission has a protocol approval gate".into());
    }
    let protocol = checked
        .protocol
        .as_ref()
        .ok_or("the mission has no protocol")?;
    if !protocol.complete {
        return Err("resolve every protocol TBD and unchecked item before approval".into());
    }
    if protocol.approved {
        return Ok(checked);
    }
    let sha256 = protocol
        .sha256
        .as_deref()
        .ok_or("the protocol could not be hashed")?;
    crate::decisions::record_at(
        root,
        mission_id,
        crate::decisions::NewResearchDecision {
            title: PROTOCOL_APPROVAL_TITLE.into(),
            choice: format!("{PROTOCOL_APPROVAL_PREFIX}{sha256}"),
            rationale: "The researcher approved this exact protocol in Happy Science before outcome inspection or analysis.".into(),
            alternatives: vec!["Request protocol revisions".into()],
            impact: Some("Authorizes analysis only under the approved protocol; any protocol edit invalidates this approval.".into()),
            supersedes: None,
        },
    )?;
    check_mission_at(root, mission_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn root(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("happy-science-mission-{name}-{}", mission_id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn kernel_compiles_a_persisted_publication_contract() {
        let root = root("publication");
        let plan =
            plan_mission_at(&root, MissionKind::EvidenceSprint, RigorLevel::Publication).unwrap();

        assert_eq!(plan.mission.status, MissionStatus::Planned);
        assert!(plan
            .mission
            .deliverables
            .contains(&"literature/evidence.csv".into()));
        assert!(plan
            .mission
            .deliverables
            .contains(&crate::evidence::ledger_path(&plan.mission.mission_id)));
        assert!(plan
            .mission
            .deliverables
            .contains(&crate::sources::manifest_path(&plan.mission.mission_id)));
        assert_eq!(
            plan.mission
                .quality_gates
                .iter()
                .map(|gate| gate.key.as_str())
                .collect::<Vec<_>>(),
            vec!["claim-evidence-valid", "deliverables-present"]
        );
        assert!(plan.prompt.contains("Happy Science mission"));
        assert!(plan.prompt.contains("`literature-survey`"));
        assert!(plan.prompt.contains("`traceability-review`"));
        assert!(!plan.prompt.contains("literature-review"));
        assert!(!plan.prompt.contains("citation-reviewer"));
        assert!(plan.prompt.contains("do not report the mission complete"));
        assert!(plan.prompt.contains("literature/evidence.csv"));
        assert!(plan.prompt.contains("Claim–Evidence Graph contract"));
        assert!(plan.prompt.contains("same claim text"));
        assert!(plan.prompt.contains("Source Snapshot contract"));
        assert!(plan.prompt.contains("snapshot hash"));
        assert!(plan.prompt.contains("contradicts|qualifies"));
        assert!(plan.prompt.contains("Research Decision Log"));
        assert!(plan.prompt.contains("never edit or replace"));

        let records = latest_at(&root).unwrap();
        assert_eq!(records, vec![plan.mission]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_json_is_ignored_and_latest_state_wins() {
        let root = root("lifecycle");
        let plan = plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Research).unwrap();
        let path = store_path(&root);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"not json\n")
            .unwrap();

        let mut running = plan.mission.clone();
        running.status = MissionStatus::Running;
        running.session_id = Some("ses_1".into());
        running.updated_at += 1;
        append(&root, &running).unwrap();

        assert_eq!(latest_at(&root).unwrap(), vec![running]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_a_planned_mission_can_start() {
        let root = root("start");
        let plan = plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Research).unwrap();

        assert_eq!(
            start_mission_at(&root, "../../escape", "ses_1").unwrap_err(),
            "a valid missionId and sessionId are required"
        );

        let running = start_mission_at(&root, &plan.mission.mission_id, "ses_1").unwrap();
        assert_eq!(running.status, MissionStatus::Running);
        assert_eq!(running.session_id.as_deref(), Some("ses_1"));
        assert_eq!(latest_at(&root).unwrap(), vec![running]);
        assert_eq!(
            start_mission_at(&root, &plan.mission.mission_id, "ses_2").unwrap_err(),
            "only a planned mission can start"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mission_state_machine_preserves_wait_pause_interrupt_and_terminal_states() {
        let root = root("state-machine");
        let plan = plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Research).unwrap();
        assert!(
            transition_mission_at(&root, &plan.mission.mission_id, MissionAction::Pause, None)
                .is_err()
        );

        start_mission_at(&root, &plan.mission.mission_id, "ses_1").unwrap();
        let waiting = transition_mission_at(
            &root,
            &plan.mission.mission_id,
            MissionAction::WaitForInput,
            Some("Need the primary endpoint"),
        )
        .unwrap();
        assert_eq!(waiting.status, MissionStatus::WaitingForInput);
        assert_eq!(
            waiting.status_reason.as_deref(),
            Some("Need the primary endpoint")
        );

        let resumed =
            transition_mission_at(&root, &plan.mission.mission_id, MissionAction::Resume, None)
                .unwrap();
        assert_eq!(resumed.status, MissionStatus::Running);
        assert_eq!(resumed.status_reason, None);

        let paused = transition_mission_at(
            &root,
            &plan.mission.mission_id,
            MissionAction::Pause,
            Some("Researcher paused the mission"),
        )
        .unwrap();
        assert_eq!(paused.status, MissionStatus::Paused);
        assert!(check_mission_at(&root, &plan.mission.mission_id).is_ok());

        transition_mission_at(&root, &plan.mission.mission_id, MissionAction::Resume, None)
            .unwrap();
        let interrupted = transition_mission_at(
            &root,
            &plan.mission.mission_id,
            MissionAction::Interrupt,
            Some("Runtime restarted"),
        )
        .unwrap();
        assert_eq!(interrupted.status, MissionStatus::Interrupted);

        transition_mission_at(&root, &plan.mission.mission_id, MissionAction::Resume, None)
            .unwrap();
        let failed = transition_mission_at(
            &root,
            &plan.mission.mission_id,
            MissionAction::Fail,
            Some("Executor exited"),
        )
        .unwrap();
        assert_eq!(failed.status, MissionStatus::Failed);
        assert!(transition_mission_at(
            &root,
            &plan.mission.mission_id,
            MissionAction::Resume,
            None
        )
        .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn contract_v4_records_migrate_once_to_the_current_state_contract() {
        let root = root("contract-migration");
        let plan = plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Research).unwrap();
        let mut legacy = plan.mission.clone();
        legacy.contract_version = MIGRATABLE_CONTRACT_VERSION;
        legacy.updated_at += 1;
        append(&root, &legacy).unwrap();

        let migrated = mission_at(&root, &legacy.mission_id).unwrap();
        assert_eq!(migrated.contract_version, CONTRACT_VERSION);
        assert_eq!(migrated.status, MissionStatus::Planned);
        assert_eq!(migrated.status_reason, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mission_check_surfaces_plan_deviations_from_its_session_runs() {
        let root = root("run-integrity");
        let plan = plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Research).unwrap();
        start_mission_at(&root, &plan.mission.mission_id, "ses_integrity").unwrap();
        fs::create_dir_all(root.join(".openscience")).unwrap();
        fs::write(
            root.join(".openscience/runs.jsonl"),
            concat!(
                "{\"runId\":\"run_1\",\"ts\":1,\"sessionId\":\"ses_integrity\",",
                "\"command\":\"python analysis.py\",\"status\":\"ok\",",
                "\"integrity\":{\"schemaVersion\":1,\"status\":\"attention\",",
                "\"planPaths\":[\"research/protocol.md\"],\"findings\":[{",
                "\"kind\":\"plan-deviation\",\"level\":\"material\",\"tag\":\"stats · prereg\",",
                "\"title\":\"Unregistered predictor\",\"evidence\":\"formula adds treatment:age\",",
                "\"path\":\"analysis.py\",\"line\":12}]}}\n"
            ),
        )
        .unwrap();

        let checked = check_mission_at(&root, &plan.mission.mission_id).unwrap();
        let integrity = checked.run_integrity.unwrap();
        assert_eq!(integrity.runs_checked, 1);
        assert_eq!(integrity.attention_runs, 1);
        assert_eq!(integrity.findings[0].kind, "plan-deviation");
        assert!(checked
            .next_actions
            .iter()
            .any(|action| action.key == "review-run-integrity"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn artifact_gate_requires_every_non_empty_deliverable() {
        let root = root("gate");
        let plan = plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Explore).unwrap();
        start_mission_at(&root, &plan.mission.mission_id, "ses_1").unwrap();

        let blocked = check_mission_at(&root, &plan.mission.mission_id).unwrap();
        assert!(!blocked.ready_for_review);
        assert_eq!(blocked.missing_deliverables.len(), 1);
        assert_eq!(blocked.next_actions[0].key, "create-deliverable");
        assert_eq!(blocked.next_actions[0].owner, MissionActionOwner::Agent);

        for path in &plan.mission.deliverables {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "evidence").unwrap();
        }
        let present = check_mission_at(&root, &plan.mission.mission_id).unwrap();
        assert!(!present.ready_for_review);
        assert!(present.missing_deliverables.is_empty());
        assert!(!present.protocol.as_ref().unwrap().complete);
        assert_eq!(
            present
                .mission
                .quality_gates
                .iter()
                .find(|gate| gate.key == "deliverables-present")
                .unwrap()
                .status,
            GateStatus::Passed
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn protocol_approval_is_hash_bound_and_invalidated_by_edits() {
        let root = root("protocol-approval");
        let plan = plan_mission_at(&root, MissionKind::StudyLaunch, RigorLevel::Research).unwrap();
        start_mission_at(&root, &plan.mission.mission_id, "ses_1").unwrap();
        fs::create_dir_all(root.join("research")).unwrap();
        fs::write(
            root.join(PROTOCOL_PATH),
            format!("# Locked protocol\n\n{}", "Finalized design. ".repeat(80)),
        )
        .unwrap();

        let awaiting = check_mission_at(&root, &plan.mission.mission_id).unwrap();
        assert!(awaiting.protocol.as_ref().unwrap().complete);
        assert!(!awaiting.protocol.as_ref().unwrap().approved);
        assert!(!awaiting.ready_for_review);
        assert_eq!(awaiting.next_actions[0].key, "approve-protocol");
        assert_eq!(
            awaiting.next_actions[0].owner,
            MissionActionOwner::Researcher
        );
        assert_eq!(awaiting.mission.contract_version, CONTRACT_VERSION);
        assert_eq!(awaiting.mission.quality_gates.len(), 3);

        let approved = approve_protocol_at(&root, &plan.mission.mission_id).unwrap();
        assert!(approved.protocol.as_ref().unwrap().approved);
        assert!(approved.ready_for_review);
        assert_eq!(approved.mission.status, MissionStatus::ReviewReady);
        assert_eq!(approved.next_actions[0].key, "complete-mission");
        assert!(approved
            .mission
            .quality_gates
            .iter()
            .all(|gate| gate.status == GateStatus::Passed));

        fs::write(
            root.join(PROTOCOL_PATH),
            format!(
                "# Revised protocol\n\n{}",
                "Finalized revision. ".repeat(80)
            ),
        )
        .unwrap();
        let invalidated = check_mission_at(&root, &plan.mission.mission_id).unwrap();
        assert!(invalidated.protocol.as_ref().unwrap().complete);
        assert!(!invalidated.protocol.as_ref().unwrap().approved);
        assert!(!invalidated.ready_for_review);
        assert_eq!(invalidated.mission.status, MissionStatus::Running);

        let reapproved = approve_protocol_at(&root, &plan.mission.mission_id).unwrap();
        assert!(reapproved.protocol.as_ref().unwrap().approved);
        assert_eq!(reapproved.decision_log.records, 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn evidence_mission_requires_a_valid_evidence_bundle() {
        let root = root("evidence-gate");
        let plan =
            plan_mission_at(&root, MissionKind::EvidenceSprint, RigorLevel::Research).unwrap();
        start_mission_at(&root, &plan.mission.mission_id, "ses_1").unwrap();
        for path in &plan.mission.deliverables {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "not valid evidence").unwrap();
        }

        let invalid = check_mission_at(&root, &plan.mission.mission_id).unwrap();
        assert!(!invalid.ready_for_review);
        assert!(invalid.missing_deliverables.is_empty());
        assert!(!invalid.evidence_ledger.as_ref().unwrap().valid());
        assert!(!invalid.source_manifest.as_ref().unwrap().valid());

        let snapshot = "The observed result was bounded.";
        let snapshot_path = "evidence/snapshots/example-study.txt";
        fs::create_dir_all(root.join("evidence/snapshots")).unwrap();
        fs::write(root.join(snapshot_path), snapshot).unwrap();
        fs::write(
            root.join(crate::evidence::ledger_path(&plan.mission.mission_id)),
            r#"{"schemaVersion":1,"evidenceId":"ev_1","claimId":"cl_1","claim":"The result is bounded.","stance":"qualifies","source":{"id":"10.1000/example","title":"Example Study","locator":"p. 7","quote":"The observed result was bounded."}}"#,
        )
        .unwrap();
        let manifest = serde_json::json!({
            "schemaVersion": 1,
            "sourceId": "10.1000/example",
            "title": "Example Study",
            "retrievedUrl": "https://doi.org/10.1000/example",
            "retrievedAt": 1_700_000_000_u64,
            "snapshotPath": snapshot_path,
            "sha256": format!("{:x}", Sha256::digest(snapshot.as_bytes())),
        });
        fs::write(
            root.join(crate::sources::manifest_path(&plan.mission.mission_id)),
            manifest.to_string(),
        )
        .unwrap();
        let ready = check_mission_at(&root, &plan.mission.mission_id).unwrap();
        assert!(ready.ready_for_review);
        assert!(ready.issues.is_empty());
        assert_eq!(ready.evidence_ledger.as_ref().unwrap().qualifies, 1);
        assert_eq!(ready.source_manifest.as_ref().unwrap().quote_matches, 1);
        assert_eq!(
            ready
                .evidence_review
                .as_ref()
                .unwrap()
                .unreviewed_evidence_ids,
            ["ev_1"]
        );
        assert_eq!(
            ready.claim_passports.as_ref().unwrap()[0].status,
            crate::claim_passport::ClaimStatus::ReviewPending
        );
        let reviewed = decide_evidence_at(
            &root,
            &plan.mission.mission_id,
            "ev_1",
            crate::adjudication::EvidenceVerdict::Accepted,
            "",
        )
        .unwrap();
        assert!(reviewed.review.complete());
        assert_eq!(reviewed.review.accepted, 1);
        assert_eq!(
            reviewed.claim_passports[0].status,
            crate::claim_passport::ClaimStatus::Qualified
        );
        assert_eq!(
            ready
                .evidence_ledger
                .as_ref()
                .unwrap()
                .qualified_only_claim_ids,
            vec!["cl_1"]
        );
        assert_eq!(
            ready
                .mission
                .quality_gates
                .iter()
                .find(|gate| gate.key == "claim-evidence-valid")
                .unwrap()
                .status,
            GateStatus::Passed
        );

        let sealed_state = check_mission_at(&root, &plan.mission.mission_id).unwrap();
        let release = crate::release_package::create_at(&root, &sealed_state).unwrap();
        assert_eq!(release.payload_files, 7);
        assert_eq!(release.claim_passports, 1);
        assert!(root.join(&release.path).is_file());

        let _ = fs::remove_dir_all(root);
    }
}
