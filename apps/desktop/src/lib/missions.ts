/** Typed clients for the Happy Science mission kernel on desktop and gateway. */
import { isTauri } from "./tauri";
import { gatewayGet, gatewayPost, isGatewayWeb } from "./webMode";

export type MissionKind =
  | "study-launch"
  | "evidence-sprint"
  | "reproduction-challenge"
  | "manuscript-stress-test";
export type RigorLevel = "explore" | "research" | "publication";
export type MissionStatus =
  | "planned"
  | "running"
  | "waiting-for-input"
  | "waiting-for-approval"
  | "paused"
  | "interrupted"
  | "review-ready"
  | "completed"
  | "failed"
  | "cancelled";
export type MissionAction =
  | "wait-for-input"
  | "wait-for-approval"
  | "pause"
  | "interrupt"
  | "resume"
  | "fail"
  | "cancel"
  | "complete";
export type GateStatus = "pending" | "passed";

export interface QualityGate {
  key: string;
  status: GateStatus;
}

export interface MissionNextAction {
  key:
    | "start-mission"
    | "resume-mission"
    | "answer-question"
    | "review-approval"
    | "create-deliverable"
    | "resolve-protocol"
    | "approve-protocol"
    | "review-evidence"
    | "repair-evidence"
    | "review-run-integrity"
    | "complete-mission";
  owner: "agent" | "researcher";
  target?: string;
}

export interface MissionRecord {
  contractVersion: number;
  missionId: string;
  kind: MissionKind;
  rigor: RigorLevel;
  status: MissionStatus;
  statusReason?: string;
  sessionId?: string;
  deliverables: string[];
  qualityGates: QualityGate[];
  createdAt: number;
  updatedAt: number;
}

export interface MissionPlan {
  mission: MissionRecord;
  prompt: string;
}

export type EvidenceStance = "supports" | "contradicts" | "qualifies";
export const EVIDENCE_VERDICT = {
  accepted: "accepted",
  rejected: "rejected",
  needsReview: "needs-review",
} as const;
export type EvidenceVerdict = (typeof EVIDENCE_VERDICT)[keyof typeof EVIDENCE_VERDICT];
export const CLAIM_STATUS = {
  reviewPending: "review-pending",
  supported: "supported",
  contested: "contested",
  contradicted: "contradicted",
  qualified: "qualified",
  unsupported: "unsupported",
} as const;
export type ClaimStatus = (typeof CLAIM_STATUS)[keyof typeof CLAIM_STATUS];

export interface EvidenceEntry {
  schemaVersion: number;
  evidenceId: string;
  claimId: string;
  claim: string;
  stance: EvidenceStance;
  source: {
    id: string;
    title: string;
    locator: string;
    quote: string;
  };
}

export interface SourceSnapshot {
  schemaVersion: number;
  sourceId: string;
  title: string;
  retrievedUrl: string;
  retrievedAt: number;
  snapshotPath: string;
  sha256: string;
}

export interface EvidenceDecision {
  schemaVersion: number;
  missionId: string;
  evidenceId: string;
  verdict: EvidenceVerdict;
  note: string;
  decidedAt: number;
}

export interface EvidenceReviewCheck {
  path: string;
  records: number;
  decisions: EvidenceDecision[];
  accepted: number;
  rejected: number;
  needsReview: number;
  unreviewedEvidenceIds: string[];
  issues: Array<{ line: number; message: string }>;
}

export interface ClaimPassport {
  schemaVersion: number;
  claimId: string;
  claim: string;
  status: ClaimStatus;
  supports: number;
  contradicts: number;
  qualifies: number;
  accepted: number;
  rejected: number;
  needsReview: number;
  unreviewed: number;
  sourceCount: number;
  verifiedSources: number;
  fingerprint: string;
}

export interface EvidenceDecisionResult {
  review: EvidenceReviewCheck;
  claimPassports: ClaimPassport[];
}

export interface ResearchDecision {
  schemaVersion: 1;
  decisionId: string;
  missionId: string;
  title: string;
  choice: string;
  rationale: string;
  alternatives: string[];
  impact?: string;
  supersedes?: string;
  decidedAt: number;
}

export interface NewResearchDecision {
  title: string;
  choice: string;
  rationale: string;
  alternatives: string[];
  impact?: string;
  supersedes?: string;
}

export interface DecisionLogCheck {
  path: string;
  records: number;
  decisions: ResearchDecision[];
  issues: Array<{ line: number; message: string }>;
}

export interface LiteratureWork {
  doi: string;
  title: string;
  authors: string[];
  year?: number;
  containerTitle?: string;
  publisher?: string;
  landingUrl: string;
  abstractText?: string;
  fullTextUrls: string[];
}

export type SnapshotStatus = "full-text" | "abstract-only" | "metadata-only";

export interface LiteratureEntry {
  schemaVersion: 1;
  missionId: string;
  canonicalId: string;
  work: LiteratureWork;
  snapshotStatus: SnapshotStatus;
  retrievedUrl: string;
  snapshotPath: string;
  sha256: string;
  addedAt: number;
}

export interface LiteratureCorpusCheck {
  path: string;
  records: number;
  entries: LiteratureEntry[];
  fullTextSnapshots: number;
  abstractSnapshots: number;
  metadataSnapshots: number;
  issues: Array<{ line: number; message: string }>;
}

export interface LiteratureSearchResult {
  provider: "crossref";
  query: string;
  returnedAt: number;
  works: LiteratureWork[];
  duplicatesRemoved: number;
}

export interface LiteratureImportResult {
  added: boolean;
  entry: LiteratureEntry;
  corpus: LiteratureCorpusCheck;
  sourceManifest: NonNullable<MissionCheck["sourceManifest"]>;
}

export interface ResearchRelease {
  schemaVersion: number;
  path: string;
  fileName: string;
  fingerprint: string;
  payloadFiles: number;
  payloadBytes: number;
  claimPassports: number;
  createdAt: number;
}

export interface ReleaseVerification {
  schemaVersion: number;
  path: string;
  valid: boolean;
  fingerprint: string | null;
  missionId: string | null;
  payloadFiles: number;
  payloadBytes: number;
  claimPassports: number;
  issues: string[];
}

export interface ReleaseImport {
  schemaVersion: number;
  sourcePath: string;
  destinationPath: string;
  fingerprint: string;
  missionId: string;
  payloadFiles: number;
  payloadBytes: number;
  importedAt: number;
}

export interface MissionCheck {
  mission: MissionRecord;
  readyForReview: boolean;
  missingDeliverables: string[];
  evidenceLedger?: {
    path: string;
    entries: EvidenceEntry[];
    records: number;
    claims: number;
    sources: number;
    supports: number;
    contradicts: number;
    qualifies: number;
    contestedClaimIds: string[];
    qualifiedOnlyClaimIds: string[];
    issues: Array<{ line: number; message: string }>;
  };
  sourceManifest?: {
    path: string;
    entries: SourceSnapshot[];
    records: number;
    verifiedSnapshots: number;
    verifiedSourceIds: string[];
    quoteMatches: number;
    issues: Array<{ line: number; message: string }>;
  };
  evidenceReview?: EvidenceReviewCheck;
  claimPassports?: ClaimPassport[];
  literatureCorpus?: LiteratureCorpusCheck;
  protocol?: {
    path: string;
    present: boolean;
    complete: boolean;
    unresolvedItems: number;
    uncheckedItems: number;
    sha256?: string;
    approved: boolean;
  };
  decisionLog: DecisionLogCheck;
  issues: string[];
  nextActions: MissionNextAction[];
  runIntegrity?: {
    runsChecked: number;
    attentionRuns: number;
    alignedRuns: number;
    noPlanRuns: number;
    findings: Array<{
      kind: string;
      level: string;
      tag: string;
      title: string;
      evidence: string;
      path: string;
      line: number;
    }>;
  };
}

function unavailable(): never {
  throw new Error("The Happy Science mission kernel requires the desktop app or gateway");
}

export async function planMission(kind: MissionKind, rigor: RigorLevel): Promise<MissionPlan> {
  if (isGatewayWeb) {
    return (await gatewayPost<MissionPlan>("/v1/missions", { kind, rigor })) ?? unavailable();
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<MissionPlan>("plan_mission", { kind, rigor });
}

export async function startMission(missionId: string, sessionId: string): Promise<MissionRecord> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<MissionRecord>(`/v1/missions/${encodeURIComponent(missionId)}/start`, {
        sessionId,
      })) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<MissionRecord>("start_mission", { missionId, sessionId });
}

export async function transitionMission(
  missionId: string,
  action: MissionAction,
  reason?: string,
): Promise<MissionRecord> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<MissionRecord>(
        `/v1/missions/${encodeURIComponent(missionId)}/transition`,
        { action, reason },
      )) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<MissionRecord>("transition_mission", { missionId, action, reason });
}

export async function listMissions(): Promise<MissionRecord[]> {
  if (isGatewayWeb) return (await gatewayGet<MissionRecord[]>("/v1/missions")) ?? unavailable();
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<MissionRecord[]>("list_missions");
}

export async function checkMission(missionId: string): Promise<MissionCheck> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<MissionCheck>(`/v1/missions/${encodeURIComponent(missionId)}/check`, {})) ??
      unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<MissionCheck>("check_mission", { missionId });
}

export async function approveProtocol(missionId: string): Promise<MissionCheck> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<MissionCheck>(
        `/v1/missions/${encodeURIComponent(missionId)}/approve-protocol`,
        {},
      )) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<MissionCheck>("approve_protocol", { missionId });
}

export async function decideEvidence(
  missionId: string,
  evidenceId: string,
  verdict: EvidenceVerdict,
  note: string,
): Promise<EvidenceDecisionResult> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<EvidenceDecisionResult>(
        `/v1/missions/${encodeURIComponent(missionId)}/evidence-decisions`,
        { evidenceId, verdict, note },
      )) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<EvidenceDecisionResult>("decide_evidence", {
    missionId,
    evidenceId,
    verdict,
    note,
  });
}

export async function recordResearchDecision(
  missionId: string,
  decision: NewResearchDecision,
): Promise<DecisionLogCheck> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<DecisionLogCheck>(
        `/v1/missions/${encodeURIComponent(missionId)}/decisions`,
        decision,
      )) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DecisionLogCheck>("record_research_decision", { missionId, decision });
}

export async function searchLiterature(
  missionId: string,
  query: string,
  limit = 10,
): Promise<LiteratureSearchResult> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<LiteratureSearchResult>(
        `/v1/missions/${encodeURIComponent(missionId)}/literature/search`,
        { query, limit },
      )) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<LiteratureSearchResult>("search_literature", { missionId, query, limit });
}

export async function captureLiterature(
  missionId: string,
  work: LiteratureWork,
): Promise<LiteratureImportResult> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<LiteratureImportResult>(
        `/v1/missions/${encodeURIComponent(missionId)}/literature/capture`,
        { work },
      )) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<LiteratureImportResult>("capture_literature", { missionId, work });
}

export async function createResearchRelease(missionId: string): Promise<ResearchRelease> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<ResearchRelease>(
        `/v1/missions/${encodeURIComponent(missionId)}/release`,
        {},
      )) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ResearchRelease>("create_research_release", { missionId });
}

export async function verifyResearchRelease(path: string): Promise<ReleaseVerification> {
  if (isGatewayWeb) {
    return (
      (await gatewayPost<ReleaseVerification>("/v1/releases/verify", { path })) ?? unavailable()
    );
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ReleaseVerification>("verify_research_release", { path });
}

export async function importResearchRelease(path: string): Promise<ReleaseImport> {
  if (isGatewayWeb) {
    return (await gatewayPost<ReleaseImport>("/v1/releases/import", { path })) ?? unavailable();
  }
  if (!isTauri) return unavailable();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ReleaseImport>("import_research_release", { path });
}
