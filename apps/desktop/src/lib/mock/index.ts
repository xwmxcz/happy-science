// First-install, read-only examples owned by Happy Science.
// Each session demonstrates one governed research mission with synthetic data.

import type { Project, Session } from "@ai4s/shared";
import { claimAuditFigure, evidenceBalanceFigure, reproductionFigure } from "./figures";

const projectId = "happy-science-demo";

const preregistrationSession: Session = {
  id: "happy-preregistration",
  projectId,
  title: "Research Launch — seed germination",
  group: "Examples",
  status: "done",
  blocks: [
    {
      kind: "user",
      text: "Turn this seed-germination idea into a decision-ready protocol. Lock the hypothesis, sample, exclusions, endpoint, analysis, and stopping rule before any outcomes are inspected.",
    },
    {
      kind: "step-summary",
      summary: "Defined the research contract, resolved 3 TBDs, and passed all 5 launch gates",
      steps: 8,
      details: [
        "primary endpoint and decision threshold locked",
        "randomization and blinded scoring specified",
        "exclusions and missing-data rules specified",
        "outcome access remains blocked until approval",
      ],
    },
    {
      kind: "agent",
      markdown:
        "I converted the idea into a protocol without opening outcome data. The contract below makes the sample size, endpoint, exclusions, model, and go/no-go rule explicit; any later change must enter the decision log.",
    },
    {
      kind: "table",
      columns: ["Contract field", "Locked value", "Gate"],
      rows: [
        ["Hypothesis", "Treatment increases day-7 germination", "protocol-complete"],
        ["Sample", "240 seeds · 4 randomized blocks", "power-recorded"],
        ["Primary endpoint", "Proportion germinated by 168 h", "endpoint-locked"],
        ["Analysis", "Binomial GLM + block fixed effects", "analysis-locked"],
        ["Decision rule", "Proceed if adjusted risk difference ≥ 0.08", "approval-required"],
      ],
      caption: "All values are part of a synthetic demonstration protocol.",
    },
    {
      kind: "reviewer",
      note: "The protocol gate runs before the agent may inspect outcomes.",
      findings: [
        {
          level: "ok",
          check: "integrity",
          tag: "protocol · preregistration",
          title: "All consequential choices were fixed before outcome access",
          evidence:
            "The protocol records the hypothesis, N, endpoint, exclusion rules, model, missing-data handling, stopping rule, and go/no-go threshold. Outcome files are not listed among inspected inputs.",
        },
      ],
    },
    {
      kind: "agent",
      markdown:
        "Protocol ready for human approval. Happy Science will keep the outcome directory sealed until the approval event is recorded.",
    },
    { kind: "status-line", text: "5/5 quality gates passed · awaiting protocol approval", tone: "done" },
  ],
  inspector: {
    variant: "artifact",
    title: "research/protocol.md",
    filename: "protocol.md",
    versions: [
      { label: "v1", reviewPassed: false },
      { label: "v2", reviewPassed: true },
    ],
    activeVersion: "v2",
    reviewPassed: true,
    inputs: ["inputs/study-brief.md"],
    language: "markdown",
    codeStartLine: 1,
    code: `# Seed germination protocol

## Primary hypothesis
The treatment increases germination by day 7 relative to control.

## Design
- 240 seeds across four randomized blocks
- blinded image scoring every 12 hours
- primary endpoint: germinated by 168 hours

## Analysis
Binomial GLM with treatment and block fixed effects.

## Decision rule
Proceed only if the adjusted risk difference is at least 0.08.

## Outcome access
Blocked until protocol approval is recorded.`,
    executionLog:
      "Happy Science contract validator\n[ok] required fields 9/9\n[ok] quality gates 5/5\n[ok] outcome access blocked\nprotocol version: 2",
    environment: "Happy Science mission kernel · contract schema v3 · synthetic demonstration",
    messages: [
      "turn this seed-germination idea into a protocol",
      "lock the decision rule before reading outcomes",
    ],
  },
};

const evidenceSession: Session = {
  id: "happy-evidence-sprint",
  projectId,
  title: "Evidence Sprint — urban cooling claim",
  group: "Examples",
  status: "warn",
  blocks: [
    {
      kind: "user",
      text: "Use the bundled demonstration source pack to assess whether street-tree interventions reduce pedestrian heat exposure. Preserve the search record, link every claim to a source, and surface contradictory evidence.",
    },
    {
      kind: "step-summary",
      summary: "Screened 22 records, retained 14, linked every finding, and isolated 2 contradictions",
      steps: 11,
      details: [
        "query and inclusion criteria frozen before screening",
        "14 source snapshots hashed and retained",
        "claim-to-source evidence table generated",
        "conflicts and indirect measurements separated",
      ],
    },
    {
      kind: "tool-call",
      tool: "search",
      verb: "Searched",
      title: "demonstration source pack · urban trees × pedestrian heat",
      status: "success",
      meta: "22 records",
    },
    {
      kind: "table",
      columns: ["Source", "Design", "Finding", "Stance", "Trace"],
      rows: [
        ["HS-DEMO-003", "field crossover", "lower radiant exposure", "supports", "snapshot ✓"],
        ["HS-DEMO-006", "street comparison", "benefit depends on canopy", "qualifies", "snapshot ✓"],
        ["HS-DEMO-011", "sensor transect", "no midday difference", "contradicts", "snapshot ✓"],
        ["HS-DEMO-014", "simulation", "cooling under low wind", "qualifies", "snapshot ✓"],
      ],
      caption: "Illustrative records from the bundled synthetic source pack; not real citations.",
    },
    {
      kind: "figure",
      title: "evidence-balance.svg",
      src: evidenceBalanceFigure,
      caption: "14 included demonstration records · stance classified before synthesis",
    },
    {
      kind: "reviewer",
      note: "Happy Science keeps the minority evidence visible instead of averaging it away.",
      findings: [
        {
          level: "warn",
          check: "number",
          title: "Four records measure modeled temperature rather than pedestrian exposure",
          evidence:
            "HS-DEMO-004, 008, 012, and 014 use modeled air temperature. They qualify mechanism and context but cannot directly support the pedestrian-exposure claim.",
        },
        {
          level: "ok",
          check: "citation",
          title: "Every synthesized finding resolves to a retained source snapshot",
          evidence: "14/14 evidence rows carry source IDs, locators, excerpts, retrieval metadata, and SHA-256 hashes.",
        },
      ],
    },
    {
      kind: "agent",
      markdown:
        "Decision-ready synthesis: the demonstration corpus leans supportive, but the effect is conditional on canopy and measurement type. Two contradictory records remain explicit, and modeled-temperature studies are not treated as direct pedestrian evidence.",
    },
    { kind: "status-line", text: "14 sources · 14 snapshots · 2 contradictions preserved", tone: "review" },
  ],
  inspector: {
    variant: "artifact",
    title: "evidence/evidence-table.csv",
    filename: "evidence-table.csv",
    versions: [{ label: "v1", reviewPassed: true }],
    activeVersion: "v1",
    reviewPassed: true,
    inputs: ["evidence/search-log.jsonl", "evidence/snapshots/"],
    language: "csv",
    code: `source_id,design,measurement,stance,snapshot_sha256
HS-DEMO-003,field crossover,pedestrian radiant exposure,supports,91bd…a10c
HS-DEMO-006,street comparison,pedestrian heat index,qualifies,4f06…77e2
HS-DEMO-011,sensor transect,pedestrian heat index,contradicts,ac72…981d
HS-DEMO-014,simulation,modeled air temperature,qualifies,37e8…2c41`,
    executionLog:
      "Evidence contract validator\n[ok] search log present\n[ok] included sources 14/14 snapshotted\n[ok] evidence rows 14/14 linked\n[warn] indirect measurements 4\n[ok] contradictions retained 2",
    environment: "Happy Science mission kernel · evidence schema v1 · synthetic demonstration corpus",
    messages: [
      "assess the urban cooling claim",
      "preserve contradictory evidence and source snapshots",
    ],
  },
};

const reproductionSession: Session = {
  id: "happy-reproduction",
  projectId,
  title: "Reproduction Challenge — dose response",
  group: "Examples",
  status: "done",
  blocks: [
    {
      kind: "user",
      text: "Reproduce the reference coefficient from the bundled synthetic dose-response dataset. Pin the environment, verify the input hash, and decide against the ±0.01 tolerance.",
    },
    {
      kind: "step-summary",
      summary: "Verified the input, rebuilt the model, and compared the estimate with the locked tolerance",
      steps: 9,
      details: [
        "input SHA-256 matched the protocol record",
        "Python and package versions pinned",
        "seed fixed before bootstrap resampling",
        "reference -0.420 · reproduced -0.416",
      ],
    },
    {
      kind: "tool-call",
      tool: "bash",
      verb: "Ran",
      title: "python reproduce.py --seed 2026 --bootstrap 1000",
      status: "success",
      meta: "4.8s",
      command: "python reproduce.py --input data/dose_response_demo.csv --seed 2026 --bootstrap 1000",
      output:
        "input sha256: 7c4a…33d9 (match)\ncoefficient: -0.416\n95% CI: [-0.451, -0.381]\nreference: -0.420\nabsolute delta: 0.004\nresult: PASS",
    },
    {
      kind: "figure",
      title: "reproduction-convergence.svg",
      src: reproductionFigure,
      caption: "Synthetic benchmark · estimate converges to -0.416; reference line is -0.420",
    },
    {
      kind: "reviewer",
      note: "The reproducibility gate compares protocol, input, environment, command, and result.",
      findings: [
        {
          level: "ok",
          check: "integrity",
          tag: "reproduction · input",
          title: "Protocol and run used the same input snapshot",
          evidence: "The run input hash 7c4a…33d9 matches the hash recorded before outcome access.",
        },
        {
          level: "ok",
          check: "number",
          title: "Absolute delta 0.004 is inside the locked ±0.01 tolerance",
          evidence: "|-0.416 - (-0.420)| = 0.004; the pass threshold was fixed before execution.",
        },
      ],
    },
    {
      kind: "agent",
      markdown:
        "Reproduction passed. The coefficient is **-0.416** versus the reference **-0.420** (absolute Δ **0.004**). The exact input hash, environment, seed, command, and output are attached to the run record.",
    },
    { kind: "status-line", text: "reproduced · Δ 0.004 · tolerance passed", tone: "done" },
  ],
  inspector: {
    variant: "artifact",
    title: "analysis/reproduce.py",
    filename: "reproduce.py",
    versions: [{ label: "v1", reviewPassed: true }],
    activeVersion: "v1",
    reviewPassed: true,
    inputs: ["data/dose_response_demo.csv", "research/protocol.md"],
    language: "python",
    code: `import hashlib
import numpy as np
import pandas as pd
import statsmodels.formula.api as smf

SEED = 2026
REFERENCE = -0.420
TOLERANCE = 0.010

path = "data/dose_response_demo.csv"
digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
df = pd.read_csv(path)
fit = smf.ols("response ~ dose + baseline", data=df).fit()
estimate = float(fit.params["dose"])

rng = np.random.default_rng(SEED)
bootstrap = []
for _ in range(1000):
    sample = df.iloc[rng.integers(0, len(df), len(df))]
    bootstrap.append(smf.ols("response ~ dose + baseline", data=sample).fit().params["dose"])

assert abs(estimate - REFERENCE) <= TOLERANCE
print({"sha256": digest, "estimate": estimate, "pass": True})`,
    executionLog:
      "$ python reproduce.py --seed 2026 --bootstrap 1000\n[ok] input hash matched\n[ok] coefficient -0.416\n[ok] absolute delta 0.004 <= 0.010\nrun id: hs-demo-repro-001",
    environment: "python 3.11 · pandas 2.2 · statsmodels 0.14 · numpy 2.0 · seed 2026",
    messages: [
      "reproduce the locked dose-response coefficient",
      "decide using the preregistered tolerance",
    ],
  },
};

const manuscriptSession: Session = {
  id: "happy-manuscript-audit",
  projectId,
  title: "Manuscript Stress Test — battery cycle life",
  group: "Examples",
  status: "warn",
  blocks: [
    {
      kind: "user",
      text: "Stress-test this synthetic battery cycle-life manuscript. Trace every quantitative claim, recompute the key numbers, and block release if the text, table, figure, and code disagree.",
    },
    {
      kind: "step-summary",
      summary: "Audited 22 claims, recomputed 9 quantities, and blocked release on one figure mismatch",
      steps: 13,
      details: [
        "18 claims fully traced",
        "3 claims missing source locators",
        "Figure 2 percentage disagrees with the results table",
        "release gate remains blocked",
      ],
    },
    {
      kind: "reviewer",
      note: "This demo uses a synthetic manuscript and synthetic measurements.",
      findings: [
        {
          level: "error",
          check: "figure",
          title: "Figure 2 reports 84% retention, but the table and code produce 81%",
          evidence:
            "results.csv row cell_B_500 = 0.812; make_figure2.py rounds to 81%. The 84% label exists only in the manuscript caption, so release is blocked.",
        },
        {
          level: "warn",
          check: "citation",
          title: "Three quantitative claims lack source locators",
          evidence: "Claims C07, C12, and C19 cite the demonstration source pack but omit page, table, or row locators.",
        },
        {
          level: "ok",
          check: "number",
          title: "Nine primary quantities recompute from the retained analysis table",
          evidence: "Recomputation matched the manuscript within the declared rounding rule for 9/9 primary quantities.",
        },
      ],
    },
    {
      kind: "figure",
      title: "claim-audit.svg",
      src: claimAuditFigure,
      caption: "22 synthetic manuscript claims · release blocked until the mismatch is resolved",
    },
    {
      kind: "agent",
      markdown:
        "Release blocked. Correct Figure 2 and add locators for claims C07, C12, and C19, then rerun the stress test. The remaining 18 claims and all 9 primary quantities are traceable.",
    },
    { kind: "status-line", text: "release blocked · 1 mismatch · 3 unlinked claims", tone: "error" },
  ],
  inspector: {
    variant: "pdf",
    title: "stress-test.pdf",
    doc: {
      title: "Battery cycle-life demonstration manuscript",
      subtitle: "synthetic data · Happy Science stress-test example",
      summaryTable: {
        kind: "table",
        columns: ["Claims", "Traced", "Unlinked", "Recomputed", "Mismatches", "Release"],
        rows: [["22", "18", "3", "9/9", "1", "Blocked"]],
      },
      figure: {
        kind: "figure",
        title: "Claim audit",
        src: claimAuditFigure,
        caption: "The mismatch is preserved as a release-blocking finding.",
      },
      sections: [
        {
          heading: "1  Audit scope",
          body: "The stress test maps each quantitative statement to a source locator, retained dataset, analysis command, and rendered figure or table.",
        },
        {
          heading: "2  Release decision",
          body: "Release is blocked because the Figure 2 caption reports 84% retention while the retained results table and plotting script both produce 81%.",
        },
      ],
    },
  },
};

export const mockProject: Project = {
  id: projectId,
  name: "Happy Science mission examples",
  sessions: [preregistrationSession, evidenceSession, reproductionSession, manuscriptSession],
};

export const mockProjects: Project[] = [mockProject];

export function findSession(sessionId: string): Session | undefined {
  return mockProject.sessions.find((session) => session.id === sessionId);
}

export const defaultSessionId = preregistrationSession.id;
