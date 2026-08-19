//! Automatic research-integrity checks attached to every local run record.
//!
//! This module is the single owner of plan-deviation, fixed-seed, and
//! interpretation rules. The runtime skill only renders these persisted
//! results; it does not independently reimplement the checks.

use std::collections::HashSet;
use std::path::{Component, Path};

use regex::Regex;

use crate::runs::RunArtifact;

const SCHEMA_VERSION: u8 = 1;
const WALK_CAP: usize = 20_000;
const FILE_CAP: u64 = 1_000_000;

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunIntegrityCheck {
    pub schema_version: u8,
    /// `no-plan` | `aligned` | `attention`.
    pub status: String,
    pub plan_paths: Vec<String>,
    pub findings: Vec<RunIntegrityFinding>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunIntegrityFinding {
    /// Stable machine-readable identifier.
    pub kind: String,
    pub level: String,
    pub tag: String,
    pub title: String,
    pub evidence: String,
    pub path: String,
    pub line: usize,
}

struct SourceFile {
    path: String,
    text: String,
}

fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".openscience" | "node_modules" | ".venv" | "venv" | "__pycache__"
    ) || (name.starts_with('.') && name != ".happy-science")
}

fn rel_key(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn is_plan(path: &Path, key: &str) -> bool {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let normalized = stem.replace('-', "_");
    normalized.contains("prereg")
        || normalized.contains("pre_reg")
        || normalized.contains("analysis_plan")
        || key.eq_ignore_ascii_case("research/protocol.md")
}

fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > FILE_CAP {
        return None;
    }
    let raw = std::fs::read_to_string(path).ok()?;
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ipynb"))
    {
        let notebook: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let code = notebook
            .get("cells")?
            .as_array()?
            .iter()
            .filter(|cell| cell.get("cell_type").and_then(|v| v.as_str()) == Some("code"))
            .filter_map(|cell| cell.get("source").and_then(|v| v.as_array()))
            .flat_map(|source| source.iter().filter_map(|line| line.as_str()))
            .collect::<String>();
        return Some(code);
    }
    Some(raw)
}

fn discover_plans(root: &Path) -> Vec<SourceFile> {
    let mut plans = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if visited >= WALK_CAP {
                break;
            }
            visited += 1;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if !is_ignored_dir(&name) {
                    stack.push(path);
                }
                continue;
            }
            let Some(key) = rel_key(root, &path) else {
                continue;
            };
            if is_plan(&path, &key) {
                if let Some(text) = read_text(&path) {
                    plans.push(SourceFile { path: key, text });
                }
            }
        }
    }
    plans.sort_by(|a, b| a.path.cmp(&b.path));
    plans
}

fn artifact_sources(root: &Path, artifacts: &[RunArtifact]) -> Vec<SourceFile> {
    artifacts
        .iter()
        .filter_map(|artifact| {
            let path = root.join(&artifact.path);
            read_text(&path).map(|text| SourceFile {
                path: artifact.path.clone(),
                text,
            })
        })
        .collect()
}

fn line_of(text: &str, index: usize) -> usize {
    text[..index.min(text.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1
}

fn line_text(text: &str, line: usize) -> String {
    text.lines()
        .nth(line.saturating_sub(1))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn finding(
    kind: &str,
    tag: &str,
    title: &str,
    detail: &str,
    file: &SourceFile,
    index: usize,
) -> RunIntegrityFinding {
    let line = line_of(&file.text, index);
    let excerpt = line_text(&file.text, line);
    RunIntegrityFinding {
        kind: kind.into(),
        level: "warn".into(),
        tag: tag.into(),
        title: title.into(),
        evidence: format!("{}:{}  {}\n  {}", file.path, line, excerpt, detail),
        path: file.path.clone(),
        line,
    }
}

fn plan_findings(plans: &[SourceFile], code: &[SourceFile]) -> Vec<RunIntegrityFinding> {
    if plans.is_empty() {
        return Vec::new();
    }
    let plan_text = plans
        .iter()
        .map(|p| p.text.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let formula =
        Regex::new(r#"([A-Za-z_.][\w.]*)\s*~\s*([^\"')\n]+)"#).expect("valid formula regex");
    let identifiers = Regex::new(r"[A-Za-z_.][\w.]*").expect("valid identifier regex");
    let interaction_named =
        Regex::new(r"(?i)interact|moderat|\*|\bx\b").expect("valid interaction regex");
    let stop: HashSet<&str> = [
        "c", "factor", "np", "pd", "sm", "smf", "log", "exp", "poly", "i", "as", "data", "family",
        "binomial", "gaussian",
    ]
    .into_iter()
    .collect();
    let mut findings = Vec::new();
    let mut seen = HashSet::new();

    for file in code {
        for captures in formula.captures_iter(&file.text) {
            let Some(rhs) = captures.get(2) else { continue };
            for term in rhs
                .as_str()
                .split('+')
                .map(str::trim)
                .filter(|term| !term.is_empty() && *term != "1")
            {
                let variables: Vec<String> = identifiers
                    .find_iter(term)
                    .map(|m| m.as_str().to_string())
                    .filter(|name| !stop.contains(name.to_ascii_lowercase().as_str()))
                    .collect();
                if variables.is_empty() {
                    continue;
                }
                let missing: Vec<&str> = variables
                    .iter()
                    .map(String::as_str)
                    .filter(|name| !plan_text.contains(&name.to_ascii_lowercase()))
                    .collect();
                if !missing.is_empty() {
                    let key = format!("predictor:{}", missing.join(","));
                    if seen.insert(key) {
                        findings.push(finding(
                            "unregistered-predictor",
                            "stats · prereg",
                            "Predictor not in the research plan",
                            &format!(
                                "Term `{term}` uses {}, which the plan does not name. Register it or label the analysis exploratory.",
                                missing.join(", ")
                            ),
                            file,
                            captures.get(0).map_or(0, |m| m.start()),
                        ));
                    }
                    continue;
                }
                let interaction = term.contains('*') || term.contains(':');
                if interaction && !interaction_named.is_match(&plan_text) {
                    let key = format!("interaction:{term}");
                    if seen.insert(key) {
                        findings.push(finding(
                            "unregistered-interaction",
                            "stats · prereg",
                            "Interaction not in the research plan",
                            &format!(
                                "Interaction `{term}` is not described in the plan. Register it or label it exploratory."
                            ),
                            file,
                            captures.get(0).map_or(0, |m| m.start()),
                        ));
                    }
                }
            }
        }
    }
    findings
}

fn seed_findings(code: &[SourceFile]) -> Vec<RunIntegrityFinding> {
    let random_use = Regex::new(
        r"(?i)\b(np\.random|numpy\.random|random\.(random|randint|sample|shuffle|choice)|train_test_split|\.sample\(|bootstrap|permutation|resample|KFold|StratifiedKFold|RandomForest|shuffle\s*=\s*True|rnorm|runif|rbinom|sample\s*\()",
    )
    .expect("valid random-use regex");
    let seed = Regex::new(
        r"(?i)(np\.random\.seed|numpy\.random\.seed|random\.seed|random_state\s*=|set\.seed|default_rng\(\s*\d|seed\s*=\s*\d)",
    )
    .expect("valid seed regex");
    code.iter()
        .filter_map(|file| {
            let used = random_use.find(&file.text)?;
            if seed.is_match(&file.text) {
                return None;
            }
            Some(finding(
                "missing-seed",
                "stats · seed",
                "Randomised analysis has no fixed seed",
                "This run uses randomness without a fixed seed, so its estimates may change between runs.",
                file,
                used.start(),
            ))
        })
        .collect()
}

fn interpretation_findings(reports: &[SourceFile]) -> Vec<RunIntegrityFinding> {
    let stats = Regex::new(
        r"(?i)\b(regression|correlat|coefficient|associat|odds\s*ratio|p\s*[<=>]\s*0?\.\d|p-?value|beta|r-?squared|significant)\b",
    )
    .expect("valid statistics regex");
    let causal = Regex::new(
        r"(?i)\b(cause|causes|caused|causing|leads?\s+to|led\s+to|results?\s+in|due\s+to|because\s+of|the\s+effect\s+of|effects?\s+on|proves?|drives?|driven\s+by|responsible\s+for)\b",
    )
    .expect("valid causal regex");
    let mut findings = Vec::new();
    for file in reports {
        if !stats.is_match(&file.text) {
            continue;
        }
        let mut lines = HashSet::new();
        for hit in causal.find_iter(&file.text) {
            let line = line_of(&file.text, hit.start());
            if lines.insert(line) {
                findings.push(finding(
                    "causal-overreach",
                    "stats · interpretation",
                    "Causal language over an association",
                    "This wording asserts causation. Report the estimate and uncertainty unless the design supports a causal claim.",
                    file,
                    hit.start(),
                ));
            }
        }
    }
    findings
}

/// Check the exact code input and report-like outputs captured for a run.
pub fn check_run_integrity(
    root: &Path,
    code: &[RunArtifact],
    outputs: &[RunArtifact],
) -> RunIntegrityCheck {
    let plans = discover_plans(root);
    let code = artifact_sources(root, code);
    let reports: Vec<SourceFile> = artifact_sources(root, outputs)
        .into_iter()
        .filter(|file| {
            matches!(
                Path::new(&file.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("md" | "txt" | "rmd")
            )
        })
        .collect();
    let mut findings = plan_findings(&plans, &code);
    findings.extend(seed_findings(&code));
    findings.extend(interpretation_findings(&reports));
    findings.sort_by(|a, b| (&a.path, a.line, &a.kind).cmp(&(&b.path, b.line, &b.kind)));
    let status = if !findings.is_empty() {
        "attention"
    } else if plans.is_empty() {
        "no-plan"
    } else {
        "aligned"
    };
    RunIntegrityCheck {
        schema_version: SCHEMA_VERSION,
        status: status.into(),
        plan_paths: plans.into_iter().map(|p| p.path).collect(),
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "happy-science-integrity-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn artifact(path: &str) -> RunArtifact {
        RunArtifact {
            path: path.into(),
            hash: None,
            size: 1,
        }
    }

    #[test]
    fn flags_unregistered_predictors_and_missing_seed() {
        let root = temp_root("deviation");
        std::fs::create_dir_all(root.join("research")).unwrap();
        std::fs::write(
            root.join("research/protocol.md"),
            "Model: happiness ~ income\n",
        )
        .unwrap();
        std::fs::write(
            root.join("analysis.py"),
            "m = ols('happiness ~ income + gender', df)\nx = np.random.choice(n, n)\n",
        )
        .unwrap();
        let result = check_run_integrity(&root, &[artifact("analysis.py")], &[]);
        assert_eq!(result.status, "attention");
        assert_eq!(result.plan_paths, vec!["research/protocol.md"]);
        assert!(result
            .findings
            .iter()
            .any(|f| f.kind == "unregistered-predictor"));
        assert!(result.findings.iter().any(|f| f.kind == "missing-seed"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reports_an_aligned_run() {
        let root = temp_root("aligned");
        std::fs::write(
            root.join("analysis_plan.md"),
            "Model: happiness ~ income + gender\n",
        )
        .unwrap();
        std::fs::write(
            root.join("analysis.py"),
            "np.random.seed(42)\nm = ols('happiness ~ income + gender', df)\n",
        )
        .unwrap();
        let result = check_run_integrity(&root, &[artifact("analysis.py")], &[]);
        assert_eq!(result.status, "aligned");
        assert!(result.findings.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn distinguishes_a_missing_plan_and_flags_report_overreach() {
        let root = temp_root("no-plan");
        std::fs::write(root.join("analysis.py"), "print('ok')\n").unwrap();
        let no_plan = check_run_integrity(&root, &[artifact("analysis.py")], &[]);
        assert_eq!(no_plan.status, "no-plan");

        std::fs::write(
            root.join("report.md"),
            "Regression p < 0.05. Income causes happiness.\n",
        )
        .unwrap();
        let warning = check_run_integrity(&root, &[], &[artifact("report.md")]);
        assert_eq!(warning.status, "attention");
        assert_eq!(warning.findings[0].kind, "causal-overreach");
        let _ = std::fs::remove_dir_all(root);
    }
}
