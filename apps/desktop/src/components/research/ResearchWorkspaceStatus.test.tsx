/** Verifies that active missions render kernel-backed research domains instead of static status. */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { MissionCheck, MissionRecord } from "@/lib/missions";

const releaseKernel = vi.hoisted(() => ({
  verify: vi.fn(),
  import: vi.fn(),
}));

vi.mock("@/lib/missions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/missions")>()),
  verifyResearchRelease: releaseKernel.verify,
  importResearchRelease: releaseKernel.import,
}));

import { ResearchWorkspaceStatus } from "./ResearchWorkspaceStatus";

const mission: MissionRecord = {
  contractVersion: 3,
  missionId: "m-evidence",
  kind: "evidence-sprint",
  rigor: "research",
  status: "running",
  sessionId: "session-1",
  deliverables: ["literature/search_log.md", "literature/evidence.csv"],
  qualityGates: [
    { key: "deliverables-present", status: "passed" },
    { key: "claim-evidence-valid", status: "passed" },
    { key: "sources-resolvable", status: "pending" },
  ],
  createdAt: 1,
  updatedAt: 2,
};

const check: MissionCheck = {
  mission,
  readyForReview: true,
  missingDeliverables: [],
  evidenceLedger: {
    path: "evidence/m-evidence.claims.jsonl",
    entries: [
      {
        schemaVersion: 1,
        evidenceId: "ev-support",
        claimId: "claim-2",
        claim: "The observed effect is bounded under the primary protocol.",
        stance: "supports",
        source: {
          id: "10.1000/study-a",
          title: "Study A",
          locator: "Results, p. 4",
          quote: "The observed effect was bounded in the primary analysis.",
        },
      },
      {
        schemaVersion: 1,
        evidenceId: "ev-contradiction",
        claimId: "claim-2",
        claim: "The observed effect is bounded under the primary protocol.",
        stance: "contradicts",
        source: {
          id: "https://example.org/study-b",
          title: "Study B",
          locator: "Table 2",
          quote: "The secondary cohort produced an unbounded estimate.",
        },
      },
    ],
    records: 2,
    claims: 1,
    sources: 2,
    supports: 1,
    contradicts: 1,
    qualifies: 0,
    contestedClaimIds: ["claim-2"],
    qualifiedOnlyClaimIds: [],
    issues: [],
  },
  sourceManifest: {
    path: "evidence/m-evidence.sources.jsonl",
    entries: [
      {
        schemaVersion: 1,
        sourceId: "10.1000/study-a",
        title: "Study A",
        retrievedUrl: "https://doi.org/10.1000/study-a",
        retrievedAt: 1_700_000_000,
        snapshotPath: "evidence/snapshots/study-a.txt",
        sha256: "a".repeat(64),
      },
    ],
    records: 1,
    verifiedSnapshots: 1,
    verifiedSourceIds: ["10.1000/study-a"],
    quoteMatches: 2,
    issues: [],
  },
  evidenceReview: {
    path: "evidence/m-evidence.reviews.jsonl",
    records: 0,
    decisions: [],
    accepted: 0,
    rejected: 0,
    needsReview: 0,
    unreviewedEvidenceIds: ["ev-support", "ev-contradiction"],
    issues: [],
  },
  claimPassports: [
    {
      schemaVersion: 1,
      claimId: "claim-2",
      claim: "The observed effect is bounded under the primary protocol.",
      status: "review-pending",
      supports: 1,
      contradicts: 1,
      qualifies: 0,
      accepted: 0,
      rejected: 0,
      needsReview: 0,
      unreviewed: 2,
      sourceCount: 2,
      verifiedSources: 1,
      fingerprint: "b".repeat(64),
    },
  ],
  decisionLog: {
    path: ".happy-science/decisions/m-evidence.jsonl",
    records: 0,
    decisions: [],
    issues: [],
  },
  literatureCorpus: {
    path: ".happy-science/literature/m-evidence.jsonl",
    records: 0,
    entries: [],
    fullTextSnapshots: 0,
    abstractSnapshots: 0,
    metadataSnapshots: 0,
    issues: [],
  },
  issues: [],
  nextActions: [],
};

describe("ResearchWorkspaceStatus", () => {
  it("turns Study Launch gates into an actionable protocol approval path", async () => {
    const studyMission: MissionRecord = {
      ...mission,
      contractVersion: 4,
      missionId: "hsm-study",
      kind: "study-launch",
      deliverables: ["research/protocol.md"],
      qualityGates: [
        { key: "protocol-complete", status: "passed" },
        { key: "approval-before-outcomes", status: "pending" },
        { key: "deliverables-present", status: "passed" },
      ],
    };
    const studyCheck: MissionCheck = {
      mission: studyMission,
      readyForReview: false,
      missingDeliverables: [],
      protocol: {
        path: "research/protocol.md",
        present: true,
        complete: true,
        unresolvedItems: 0,
        uncheckedItems: 0,
        sha256: "a".repeat(64),
        approved: false,
      },
      decisionLog: { path: ".happy-science/decisions/hsm-study.jsonl", records: 0, decisions: [], issues: [] },
      issues: ["the current protocol has not been approved before outcomes"],
      nextActions: [
        { key: "approve-protocol", owner: "researcher", target: "research/protocol.md" },
      ],
    };
    const onApproveProtocol = vi.fn().mockResolvedValue(studyCheck);
    render(
      <ResearchWorkspaceStatus
        mission={studyMission}
        check={studyCheck}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={vi.fn()}
        onApproveProtocol={onApproveProtocol}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={vi.fn()}
      />,
    );

    expect(screen.getByText("Complete protocol")).toBeInTheDocument();
    expect(screen.getAllByText("Approve before outcomes").length).toBeGreaterThan(0);
    await userEvent.click(screen.getByRole("button", { name: "Approve current protocol" }));
    expect(screen.getByText("Approve this protocol?")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Approve protocol" }));
    expect(onApproveProtocol).toHaveBeenCalledTimes(1);
  });

  it("shows mission identity and persisted quality gates before a check", () => {
    render(
      <ResearchWorkspaceStatus
        mission={mission}
        check={null}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={vi.fn()}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={vi.fn()}
      />,
    );

    expect(screen.getByText("Evidence sprint")).toBeInTheDocument();
    expect(screen.getByText("2 of 3 quality gates passed.")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /Mission/ })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Evidence" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Sources" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Artifacts" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Review" })).toBeInTheDocument();
  });

  it("shows kernel-owned next actions and resumes a paused mission", async () => {
    const pausedMission: MissionRecord = {
      ...mission,
      status: "paused",
      statusReason: "Researcher paused the mission",
    };
    const pausedCheck: MissionCheck = {
      ...check,
      mission: pausedMission,
      readyForReview: false,
      nextActions: [{ key: "resume-mission", owner: "researcher" }],
    };
    const onTransition = vi.fn().mockResolvedValue({ ...pausedMission, status: "running" });
    render(
      <ResearchWorkspaceStatus
        mission={pausedMission}
        check={pausedCheck}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={vi.fn()}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={vi.fn()}
        onTransition={onTransition}
      />,
    );

    expect(screen.getByText("Resume from the preserved checkpoint.")).toBeInTheDocument();
    expect(screen.getByText("You")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Resume" }));
    expect(onTransition).toHaveBeenCalledWith("resume", "Researcher selected resume");
  });

  it("surfaces preregistration-to-run deviations in the mission review", async () => {
    const integrityCheck: MissionCheck = {
      ...check,
      runIntegrity: {
        runsChecked: 2,
        attentionRuns: 1,
        alignedRuns: 1,
        noPlanRuns: 0,
        findings: [
          {
            kind: "plan-deviation",
            level: "material",
            tag: "stats · prereg",
            title: "Unregistered predictor",
            evidence: "The executed formula adds treatment:age.",
            path: "analysis.py",
            line: 12,
          },
        ],
      },
    };
    render(
      <ResearchWorkspaceStatus
        mission={mission}
        check={integrityCheck}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={vi.fn()}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: /Review/ }));
    expect(screen.getByText("Plan ↔ run integrity")).toBeInTheDocument();
    expect(screen.getByText("Unregistered predictor")).toBeInTheDocument();
    expect(screen.getByText("analysis.py:12")).toBeInTheDocument();
  });

  it("shows evidence, source, and review values from MissionCheck", async () => {
    render(
      <ResearchWorkspaceStatus
        mission={mission}
        check={check}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={vi.fn()}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: /Evidence/ }));
    expect(screen.getByText("Claims").previousElementSibling).toHaveTextContent("1");
    expect(screen.getByText("Contested").previousElementSibling).toHaveTextContent("1");
    expect(
      screen.getByText("The observed effect is bounded under the primary protocol."),
    ).toBeInTheDocument();
    expect(screen.getByText("The secondary cohort produced an unbounded estimate.")).toBeInTheDocument();
    expect(screen.getByText("Claim Passport")).toBeInTheDocument();
    expect(screen.getByText("Review pending")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("tab", { name: /Sources/ }));
    expect(screen.getByText("Verified snapshots").previousElementSibling).toHaveTextContent("1");
    expect(screen.getByText("Exact quotes").previousElementSibling).toHaveTextContent("2");
    expect(screen.getByText("Snapshot verified")).toBeInTheDocument();
    expect(screen.getByText("evidence/snapshots/study-a.txt")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("tab", { name: /Review/ }));
    expect(screen.getByText("Ready for review")).toBeInTheDocument();
  });

  it("checks status only when the researcher asks", async () => {
    const onRefresh = vi.fn();
    render(
      <ResearchWorkspaceStatus
        mission={mission}
        check={null}
        checking={false}
        onRefresh={onRefresh}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={vi.fn()}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: /Check status/ }));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  it("records an append-only research decision from the workspace", async () => {
    const recorded = {
      path: check.decisionLog.path,
      records: 1,
      issues: [],
      decisions: [
        {
          schemaVersion: 1 as const,
          decisionId: "hsd_0123456789abcdef",
          missionId: mission.missionId,
          title: "Primary estimator",
          choice: "Use inverse-probability weighting",
          rationale: "It matches the registered estimand.",
          alternatives: ["Complete-case analysis"],
          decidedAt: 1_700_000_000,
        },
      ],
    };
    const onResearchDecision = vi.fn().mockResolvedValue(recorded);
    render(
      <ResearchWorkspaceStatus
        mission={mission}
        check={check}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={onResearchDecision}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: /Decisions/ }));
    await userEvent.type(screen.getByLabelText("Decision point"), "Primary estimator");
    await userEvent.type(screen.getByLabelText("Chosen approach"), "Use inverse-probability weighting");
    await userEvent.type(screen.getByLabelText("Rationale and evidence"), "It matches the registered estimand.");
    await userEvent.type(screen.getByLabelText(/Alternatives considered/), "Complete-case analysis");
    await userEvent.click(screen.getByRole("button", { name: "Record decision" }));

    expect(onResearchDecision).toHaveBeenCalledWith({
      title: "Primary estimator",
      choice: "Use inverse-probability weighting",
      rationale: "It matches the registered estimand.",
      alternatives: ["Complete-case analysis"],
      impact: undefined,
      supersedes: undefined,
    });
    expect(await screen.findByText("Use inverse-probability weighting")).toBeInTheDocument();
  });

  it("searches, deduplicates, and captures a verified literature snapshot", async () => {
    const work = {
      doi: "10.1000/study-a",
      title: "A verified study",
      authors: ["Ada Researcher"],
      year: 2024,
      containerTitle: "Research Journal",
      landingUrl: "https://doi.org/10.1000/study-a",
      abstractText: "A detailed abstract.",
      fullTextUrls: [],
    };
    const onLiteratureSearch = vi.fn().mockResolvedValue({
      provider: "crossref",
      query: "verified study",
      returnedAt: 1,
      works: [work],
      duplicatesRemoved: 2,
    });
    const corpus = {
      ...check.literatureCorpus!,
      records: 1,
      abstractSnapshots: 1,
      entries: [
        {
          schemaVersion: 1 as const,
          missionId: mission.missionId,
          canonicalId: work.doi,
          work,
          snapshotStatus: "abstract-only" as const,
          retrievedUrl: work.landingUrl,
          snapshotPath: "evidence/snapshots/crossref-a.txt",
          sha256: "a".repeat(64),
          addedAt: 2,
        },
      ],
    };
    const onLiteratureCapture = vi.fn().mockResolvedValue({
      added: true,
      entry: corpus.entries[0],
      corpus,
      sourceManifest: check.sourceManifest,
    });
    render(
      <ResearchWorkspaceStatus
        mission={mission}
        check={check}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={vi.fn()}
        onLiteratureSearch={onLiteratureSearch}
        onLiteratureCapture={onLiteratureCapture}
        onCreateRelease={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: /Literature/ }));
    await userEvent.type(screen.getByPlaceholderText(/Search by topic/), "verified study");
    await userEvent.click(screen.getByRole("button", { name: "Search literature" }));
    expect(await screen.findByText("A verified study")).toBeInTheDocument();
    expect(screen.getByText(/2 duplicates removed/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Capture source" }));
    expect(onLiteratureCapture).toHaveBeenCalledWith(work);
    expect(await screen.findByText("Abstract only")).toBeInTheDocument();
  });

  it("requires a reason and persists a human evidence decision", async () => {
    const onEvidenceDecision = vi.fn().mockResolvedValue({
      ...check.evidenceReview,
      records: 1,
      rejected: 1,
      unreviewedEvidenceIds: ["ev-contradiction"],
    });
    render(
      <ResearchWorkspaceStatus
        mission={mission}
        check={check}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={onEvidenceDecision}
        onResearchDecision={vi.fn()}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: /Evidence/ }));
    await userEvent.click(screen.getAllByRole("button", { name: "Reject" })[0]);
    const note = screen.getByPlaceholderText("Why should this evidence be excluded?");
    expect(screen.getByRole("button", { name: "Save decision" })).toBeDisabled();
    await userEvent.type(note, "Population does not match the protocol.");
    await userEvent.click(screen.getByRole("button", { name: "Save decision" }));

    expect(onEvidenceDecision).toHaveBeenCalledWith(
      "ev-support",
      "rejected",
      "Population does not match the protocol.",
    );
  });

  it("seals a release only after every Claim Passport is cleared", async () => {
    releaseKernel.verify.mockResolvedValue({
      schemaVersion: 1,
      path: "releases/happy-science-hsm.zip",
      valid: true,
      fingerprint: "c".repeat(64),
      missionId: mission.missionId,
      payloadFiles: 6,
      payloadBytes: 4096,
      claimPassports: 1,
      issues: [],
    });
    releaseKernel.import.mockResolvedValue({
      schemaVersion: 1,
      sourcePath: "releases/happy-science-hsm.zip",
      destinationPath: "imports/happy-science-hsm-isolated",
      fingerprint: "c".repeat(64),
      missionId: mission.missionId,
      payloadFiles: 6,
      payloadBytes: 4096,
      importedAt: 1_700_000_001,
    });
    const onCreateRelease = vi.fn().mockResolvedValue({
      schemaVersion: 1,
      path: "releases/happy-science-hsm.zip",
      fileName: "happy-science-hsm.zip",
      fingerprint: "c".repeat(64),
      payloadFiles: 6,
      payloadBytes: 4096,
      claimPassports: 1,
      createdAt: 1_700_000_000,
    });
    const releasable: MissionCheck = {
      ...check,
      evidenceReview: {
        ...check.evidenceReview!,
        records: 2,
        decisions: [
          {
            schemaVersion: 1,
            missionId: mission.missionId,
            evidenceId: "ev-support",
            verdict: "accepted",
            note: "",
            decidedAt: 1,
          },
          {
            schemaVersion: 1,
            missionId: mission.missionId,
            evidenceId: "ev-contradiction",
            verdict: "rejected",
            note: "Outside the protocol population.",
            decidedAt: 2,
          },
        ],
        accepted: 1,
        rejected: 1,
        unreviewedEvidenceIds: [],
      },
      claimPassports: [{ ...check.claimPassports![0], status: "supported", unreviewed: 0 }],
    };
    render(
      <ResearchWorkspaceStatus
        mission={mission}
        check={releasable}
        checking={false}
        onRefresh={() => {}}
        onEvidenceDecision={vi.fn()}
        onResearchDecision={vi.fn()}
        onLiteratureSearch={vi.fn()}
        onLiteratureCapture={vi.fn()}
        onCreateRelease={onCreateRelease}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: /Review/ }));
    await userEvent.click(screen.getByRole("button", { name: "Seal release" }));

    expect(onCreateRelease).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("Sealed")).toBeInTheDocument();
    expect(screen.getByText("6 files")).toBeInTheDocument();
    expect(screen.getByText("1 passports")).toBeInTheDocument();
    expect(screen.getByText("4.0 KiB")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Verify" }));
    expect(await screen.findByText("Verified · 6 payload files")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Safe import" }));
    expect(
      await screen.findByText(/imports\/happy-science-hsm-isolated/),
    ).toBeInTheDocument();
    expect(releaseKernel.import).toHaveBeenCalledWith("releases/happy-science-hsm.zip");
  });
});
