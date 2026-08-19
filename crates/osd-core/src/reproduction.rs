//! Prepared run reproductions and deterministic baseline comparisons.
//!
//! A user action prepares one exact baseline command. The next matching local
//! run consumes that request and persists a comparison of inputs, code,
//! environment, and outputs directly on the candidate run record.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::provenance::EnvInfo;
use crate::runs::{read_runs, refresh_artifacts, RunArtifact, RunRecord};
use crate::runtime::workspace_dir;
use crate::Env;

const STORE_PATH: &str = ".happy-science/reproductions.jsonl";
const PENDING_TTL_SECONDS: u64 = 24 * 60 * 60;
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactComparison {
    pub matched: Vec<String>,
    pub changed: Vec<String>,
    pub missing: Vec<String>,
    pub added: Vec<String>,
    pub unverifiable: Vec<String>,
}

impl ArtifactComparison {
    fn different(&self) -> bool {
        !self.changed.is_empty() || !self.missing.is_empty() || !self.added.is_empty()
    }

    fn uncertain(&self) -> bool {
        !self.unverifiable.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentComparison {
    /// None means at least one run has no captured environment.
    pub matches: Option<bool>,
    pub changes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReproductionOutcome {
    Identical,
    Different,
    Unverifiable,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReproductionPreflight {
    pub inputs: ArtifactComparison,
    pub code: ArtifactComparison,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReproductionRequest {
    pub request_id: String,
    pub baseline_run_id: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub requested_at: u64,
    pub preflight: ReproductionPreflight,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReproduction {
    pub request_id: String,
    pub baseline_run_id: String,
    pub outcome: ReproductionOutcome,
    pub inputs: ArtifactComparison,
    pub code: ArtifactComparison,
    pub environment: EnvironmentComparison,
    pub outputs: ArtifactComparison,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
enum ReproductionEvent {
    Prepared {
        request: ReproductionRequest,
    },
    Completed {
        request_id: String,
        candidate_run_id: String,
        completed_at: u64,
    },
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn request_id() -> String {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    format!(
        "hsr_{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn append_event(root: &Path, event: &ReproductionEvent) -> Result<(), String> {
    let path = root.join(STORE_PATH);
    fs::create_dir_all(path.parent().expect("reproduction store has a parent"))
        .map_err(|error| format!("reproduction store directory failed: {error}"))?;
    let line = serde_json::to_string(event)
        .map_err(|error| format!("reproduction event serialize failed: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("reproduction store open failed: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("reproduction store write failed: {error}"))
}

fn pending_requests(root: &Path) -> Vec<ReproductionRequest> {
    let file = match fs::File::open(root.join(STORE_PATH)) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    let mut prepared = HashMap::new();
    let mut completed = HashSet::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        match serde_json::from_str::<ReproductionEvent>(&line) {
            Ok(ReproductionEvent::Prepared { request }) => {
                prepared.insert(request.request_id.clone(), request);
            }
            Ok(ReproductionEvent::Completed { request_id, .. }) => {
                completed.insert(request_id);
            }
            Err(_) => {}
        }
    }
    let cutoff = now().saturating_sub(PENDING_TTL_SECONDS);
    let mut requests = prepared
        .into_values()
        .filter(|request| {
            !completed.contains(&request.request_id) && request.requested_at >= cutoff
        })
        .collect::<Vec<_>>();
    requests.sort_by_key(|request| std::cmp::Reverse(request.requested_at));
    requests
}

fn compare_artifacts(baseline: &[RunArtifact], candidate: &[RunArtifact]) -> ArtifactComparison {
    let baseline = baseline
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact))
        .collect::<HashMap<_, _>>();
    let candidate = candidate
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact))
        .collect::<HashMap<_, _>>();
    let mut comparison = ArtifactComparison {
        matched: Vec::new(),
        changed: Vec::new(),
        missing: Vec::new(),
        added: Vec::new(),
        unverifiable: Vec::new(),
    };
    for (path, before) in &baseline {
        let Some(after) = candidate.get(path) else {
            comparison.missing.push((*path).to_owned());
            continue;
        };
        match (&before.hash, &after.hash) {
            (Some(before), Some(after)) if before == after => {
                comparison.matched.push((*path).to_owned())
            }
            (Some(_), Some(_)) => comparison.changed.push((*path).to_owned()),
            _ if before.size != after.size => comparison.changed.push((*path).to_owned()),
            _ => comparison.unverifiable.push((*path).to_owned()),
        }
    }
    for path in candidate.keys() {
        if !baseline.contains_key(path) {
            comparison.added.push((*path).to_owned());
        }
    }
    comparison.matched.sort();
    comparison.changed.sort();
    comparison.missing.sort();
    comparison.added.sort();
    comparison.unverifiable.sort();
    comparison
}

fn compare_environment(
    baseline: Option<&EnvInfo>,
    candidate: Option<&EnvInfo>,
) -> EnvironmentComparison {
    let (Some(baseline), Some(candidate)) = (baseline, candidate) else {
        return EnvironmentComparison {
            matches: None,
            changes: vec!["environment-unavailable".into()],
        };
    };
    let mut changes = Vec::new();
    if baseline.python != candidate.python {
        changes.push("python".into());
    }
    if baseline.platform != candidate.platform {
        changes.push("platform".into());
    }
    if baseline.app != candidate.app {
        changes.push("app".into());
    }
    if baseline.packages.as_ref().map(|packages| &packages.hash)
        != candidate.packages.as_ref().map(|packages| &packages.hash)
    {
        changes.push("packages".into());
    }
    let baseline_hardware = baseline.hardware.as_ref();
    let candidate_hardware = candidate.hardware.as_ref();
    if baseline_hardware.and_then(|hardware| hardware.cpu.as_ref())
        != candidate_hardware.and_then(|hardware| hardware.cpu.as_ref())
        || baseline_hardware.and_then(|hardware| hardware.accelerator.as_ref())
            != candidate_hardware.and_then(|hardware| hardware.accelerator.as_ref())
        || baseline_hardware.map(|hardware| &hardware.gpu)
            != candidate_hardware.map(|hardware| &hardware.gpu)
    {
        changes.push("hardware".into());
    }
    EnvironmentComparison {
        matches: Some(changes.is_empty()),
        changes,
    }
}

fn outcome(
    status: &str,
    inputs: &ArtifactComparison,
    code: &ArtifactComparison,
    environment: &EnvironmentComparison,
    outputs: &ArtifactComparison,
) -> ReproductionOutcome {
    if status != "ok" {
        return ReproductionOutcome::Failed;
    }
    if inputs.different()
        || code.different()
        || outputs.different()
        || environment.matches == Some(false)
    {
        return ReproductionOutcome::Different;
    }
    if inputs.uncertain()
        || code.uncertain()
        || outputs.uncertain()
        || environment.matches.is_none()
    {
        return ReproductionOutcome::Unverifiable;
    }
    ReproductionOutcome::Identical
}

pub fn prepare(env: &Env, baseline_run_id: &str) -> Result<ReproductionRequest, String> {
    let root = workspace_dir(env)?;
    prepare_at(&root, baseline_run_id)
}

fn prepare_at(root: &Path, baseline_run_id: &str) -> Result<ReproductionRequest, String> {
    let baseline = read_runs(root)
        .into_iter()
        .find(|run| run.run_id == baseline_run_id)
        .ok_or_else(|| format!("unknown run: {baseline_run_id}"))?;
    if baseline
        .surface
        .as_deref()
        .is_some_and(|surface| surface != "local")
    {
        return Err("remote runs must be reproduced on their original compute surface".into());
    }
    let current_inputs = refresh_artifacts(root, &baseline.inputs);
    let current_code = refresh_artifacts(root, &baseline.code);
    let request = ReproductionRequest {
        request_id: request_id(),
        baseline_run_id: baseline.run_id,
        command: baseline.command,
        session_id: baseline.session_id,
        requested_at: now(),
        preflight: ReproductionPreflight {
            inputs: compare_artifacts(&baseline.inputs, &current_inputs),
            code: compare_artifacts(&baseline.code, &current_code),
        },
    };
    let _guard = STORE_LOCK
        .lock()
        .map_err(|_| "reproduction store lock poisoned")?;
    append_event(
        root,
        &ReproductionEvent::Prepared {
            request: request.clone(),
        },
    )?;
    Ok(request)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn complete_for_run(
    root: &Path,
    candidate_run_id: &str,
    command: &str,
    session_id: Option<&str>,
    status: &str,
    inputs: &[RunArtifact],
    code: &[RunArtifact],
    outputs: &[RunArtifact],
    env: Option<&EnvInfo>,
) -> Option<RunReproduction> {
    let _guard = STORE_LOCK.lock().ok()?;
    let request = pending_requests(root).into_iter().find(|request| {
        request.command == command
            && (request.session_id.is_none() || request.session_id.as_deref() == session_id)
    })?;
    let baseline: RunRecord = read_runs(root)
        .into_iter()
        .find(|run| run.run_id == request.baseline_run_id)?;
    let input_comparison = compare_artifacts(&baseline.inputs, inputs);
    let code_comparison = compare_artifacts(&baseline.code, code);
    let environment_comparison = compare_environment(baseline.env.as_ref(), env);
    let output_comparison = compare_artifacts(&baseline.outputs, outputs);
    let result = RunReproduction {
        request_id: request.request_id.clone(),
        baseline_run_id: request.baseline_run_id,
        outcome: outcome(
            status,
            &input_comparison,
            &code_comparison,
            &environment_comparison,
            &output_comparison,
        ),
        inputs: input_comparison,
        code: code_comparison,
        environment: environment_comparison,
        outputs: output_comparison,
    };
    append_event(
        root,
        &ReproductionEvent::Completed {
            request_id: request.request_id,
            candidate_run_id: candidate_run_id.into(),
            completed_at: now(),
        },
    )
    .ok()?;
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::record_run_inner;

    fn root() -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("happy-science-reproduction-{}", request_id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_prepared_command_records_its_baseline_comparison() {
        let root = root();
        fs::write(root.join("analysis.py"), "print('v1')\n").unwrap();
        fs::write(root.join("data.csv"), "x\n1\n").unwrap();
        let baseline = record_run_inner(
            &root,
            "python analysis.py data.csv",
            None,
            Some(1_000),
            None,
            "ok",
            Some("local".into()),
            Some("ses_1".into()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(baseline.inputs[0].path, "data.csv");
        let request = prepare_at(&root, &baseline.run_id).unwrap();
        assert!(request.preflight.code.changed.is_empty());

        fs::write(root.join("analysis.py"), "print('v2')\n").unwrap();
        let candidate = record_run_inner(
            &root,
            "python analysis.py data.csv",
            None,
            Some(2_000),
            None,
            "ok",
            Some("local".into()),
            Some("ses_1".into()),
            None,
            None,
        )
        .unwrap();
        let comparison = candidate.reproduction.expect("reproduction attached");
        assert_eq!(comparison.baseline_run_id, baseline.run_id);
        assert_eq!(comparison.outcome, ReproductionOutcome::Different);
        assert_eq!(comparison.code.changed, vec!["analysis.py"]);
        assert_eq!(comparison.inputs.matched, vec!["data.csv"]);
        assert!(pending_requests(&root).is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
