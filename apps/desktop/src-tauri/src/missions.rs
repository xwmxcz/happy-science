// Desktop bridge to the headless Happy Science mission kernel. The contract,
// prompt compiler, validation, and persistence stay in osd-core so Tauri is
// only one of several clients.
use tauri::AppHandle;

use osd_core::adjudication::EvidenceVerdict;
use osd_core::decisions::{DecisionLogCheck, NewResearchDecision};
use osd_core::literature::{LiteratureImportResult, LiteratureSearchResult, LiteratureWork};
use osd_core::missions::EvidenceDecisionResult;
use osd_core::missions::{
    MissionAction, MissionCheck, MissionKind, MissionPlan, MissionRecord, RigorLevel,
};
use osd_core::release_package::{ReleaseImport, ReleaseVerification, ResearchRelease};

use crate::env_of;

#[tauri::command(async)]
pub fn plan_mission(
    app: AppHandle,
    kind: MissionKind,
    rigor: RigorLevel,
) -> Result<MissionPlan, String> {
    osd_core::missions::plan_mission(&env_of(&app), kind, rigor)
}

#[tauri::command(async)]
pub fn start_mission(
    app: AppHandle,
    mission_id: String,
    session_id: String,
) -> Result<MissionRecord, String> {
    osd_core::missions::start_mission(&env_of(&app), &mission_id, &session_id)
}

#[tauri::command(async)]
pub fn transition_mission(
    app: AppHandle,
    mission_id: String,
    action: MissionAction,
    reason: Option<String>,
) -> Result<MissionRecord, String> {
    osd_core::missions::transition_mission(&env_of(&app), &mission_id, action, reason.as_deref())
}

#[tauri::command(async)]
pub fn list_missions(app: AppHandle) -> Result<Vec<MissionRecord>, String> {
    osd_core::missions::list_missions(&env_of(&app))
}

#[tauri::command(async)]
pub fn check_mission(app: AppHandle, mission_id: String) -> Result<MissionCheck, String> {
    osd_core::missions::check_mission(&env_of(&app), &mission_id)
}

#[tauri::command(async)]
pub fn approve_protocol(app: AppHandle, mission_id: String) -> Result<MissionCheck, String> {
    osd_core::missions::approve_protocol(&env_of(&app), &mission_id)
}

#[tauri::command(async)]
pub fn decide_evidence(
    app: AppHandle,
    mission_id: String,
    evidence_id: String,
    verdict: EvidenceVerdict,
    note: String,
) -> Result<EvidenceDecisionResult, String> {
    osd_core::missions::decide_evidence(&env_of(&app), &mission_id, &evidence_id, verdict, &note)
}

#[tauri::command(async)]
pub fn record_research_decision(
    app: AppHandle,
    mission_id: String,
    decision: NewResearchDecision,
) -> Result<DecisionLogCheck, String> {
    osd_core::decisions::record(&env_of(&app), &mission_id, decision)
}

#[tauri::command(async)]
pub fn search_literature(
    app: AppHandle,
    mission_id: String,
    query: String,
    limit: usize,
) -> Result<LiteratureSearchResult, String> {
    osd_core::literature::search(&env_of(&app), &mission_id, &query, limit)
}

#[tauri::command(async)]
pub fn capture_literature(
    app: AppHandle,
    mission_id: String,
    work: LiteratureWork,
) -> Result<LiteratureImportResult, String> {
    osd_core::literature::capture(&env_of(&app), &mission_id, work)
}

#[tauri::command(async)]
pub fn create_research_release(
    app: AppHandle,
    mission_id: String,
) -> Result<ResearchRelease, String> {
    osd_core::release_package::create(&env_of(&app), &mission_id)
}

#[tauri::command(async)]
pub fn verify_research_release(
    app: AppHandle,
    path: String,
) -> Result<ReleaseVerification, String> {
    osd_core::release_package::verify(&env_of(&app), &path)
}

#[tauri::command(async)]
pub fn import_research_release(app: AppHandle, path: String) -> Result<ReleaseImport, String> {
    osd_core::release_package::import(&env_of(&app), &path)
}
