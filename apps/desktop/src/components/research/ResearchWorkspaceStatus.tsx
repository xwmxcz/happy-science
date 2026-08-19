/** Presents mission evidence, human review, and verified release export/import handoff. */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  BookOpenCheck,
  Boxes,
  Check,
  CheckCircle2,
  CircleHelp,
  ClipboardCheck,
  FileCheck2,
  Fingerprint,
  FileSearch,
  FolderOpen,
  LibraryBig,
  PackageCheck,
  Pause,
  Play,
  Quote,
  RefreshCw,
  ArchiveRestore,
  Scale,
  ShieldCheck,
  X,
  type LucideIcon,
} from "lucide-react";
import type {
  ClaimPassport,
  DecisionLogCheck,
  EvidenceDecision,
  EvidenceReviewCheck,
  EvidenceVerdict,
  LiteratureCorpusCheck,
  LiteratureImportResult,
  LiteratureSearchResult,
  LiteratureWork,
  MissionCheck,
  MissionAction,
  MissionKind,
  MissionNextAction,
  MissionRecord,
  NewResearchDecision,
  ResearchRelease,
  ReleaseImport,
  ReleaseVerification,
} from "@/lib/missions";
import {
  CLAIM_STATUS,
  EVIDENCE_VERDICT,
  importResearchRelease,
  verifyResearchRelease,
} from "@/lib/missions";
import { presentArtifact } from "@/lib/artifactFile";
import { openExternal } from "@/lib/tauri";
import { cn } from "@/lib/cn";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { toast } from "@/lib/toast";

type ResearchDomain = "mission" | "decisions" | "literature" | "evidence" | "sources" | "artifacts" | "review";

const DOMAINS: Array<{
  id: ResearchDomain;
  icon: LucideIcon;
}> = [
  { id: "mission", icon: ClipboardCheck },
  { id: "decisions", icon: Scale },
  { id: "literature", icon: LibraryBig },
  { id: "evidence", icon: BookOpenCheck },
  { id: "sources", icon: Boxes },
  { id: "artifacts", icon: FileCheck2 },
  { id: "review", icon: ShieldCheck },
];

type MissionCopyKey =
  | "starters.plan.title"
  | "starters.literature.title"
  | "starters.reproduce.title"
  | "starters.audit.title";

const MISSION_COPY_KEY: Record<MissionKind, MissionCopyKey> = {
  "study-launch": "starters.plan.title",
  "evidence-sprint": "starters.literature.title",
  "reproduction-challenge": "starters.reproduce.title",
  "manuscript-stress-test": "starters.audit.title",
};

type RigorCopyKey =
  | "starters.rigor.explore.title"
  | "starters.rigor.research.title"
  | "starters.rigor.publication.title";

const RIGOR_COPY_KEY: Record<MissionRecord["rigor"], RigorCopyKey> = {
  explore: "starters.rigor.explore.title",
  research: "starters.rigor.research.title",
  publication: "starters.rigor.publication.title",
};

const STUDY_GATE_COPY = {
  "protocol-complete": {
    title: "researchWorkspace.gate.protocol-complete.title",
    description: "researchWorkspace.gate.protocol-complete.description",
  },
  "approval-before-outcomes": {
    title: "researchWorkspace.gate.approval-before-outcomes.title",
    description: "researchWorkspace.gate.approval-before-outcomes.description",
  },
  "deliverables-present": {
    title: "researchWorkspace.gate.deliverables-present.title",
    description: "researchWorkspace.gate.deliverables-present.description",
  },
} as const;

export function ResearchWorkspaceStatus({
  mission,
  check,
  checking,
  onRefresh,
  onEvidenceDecision,
  onResearchDecision,
  onApproveProtocol,
  onLiteratureSearch,
  onLiteratureCapture,
  onCreateRelease,
  onTransition,
}: {
  mission: MissionRecord;
  check: MissionCheck | null;
  checking: boolean;
  onRefresh: () => void;
  onEvidenceDecision: (
    evidenceId: string,
    verdict: EvidenceVerdict,
    note: string,
  ) => Promise<EvidenceReviewCheck>;
  onResearchDecision: (decision: NewResearchDecision) => Promise<DecisionLogCheck>;
  onApproveProtocol?: () => Promise<MissionCheck>;
  onLiteratureSearch: (query: string) => Promise<LiteratureSearchResult>;
  onLiteratureCapture: (work: LiteratureWork) => Promise<LiteratureImportResult>;
  onCreateRelease: () => Promise<ResearchRelease>;
  onTransition?: (action: MissionAction, reason?: string) => Promise<MissionRecord>;
}) {
  const { t } = useTranslation("session");
  const [domain, setDomain] = useState<ResearchDomain>("mission");
  const [confirmApproval, setConfirmApproval] = useState(false);
  const [approving, setApproving] = useState(false);
  const [transitioning, setTransitioning] = useState(false);
  const gates = check?.mission.qualityGates ?? mission.qualityGates;
  const passedGates = gates.filter((gate) => gate.status === "passed").length;
  const nextGate = gates.find((gate) => gate.status !== "passed");
  const missing = check?.missingDeliverables ?? [];
  const presentArtifacts = mission.deliverables.length - missing.length;
  const evidenceRequired = mission.kind !== "study-launch";

  const badgeFor = (id: ResearchDomain): string | null => {
    if (id === "mission") return `${passedGates}/${gates.length}`;
    if (id === "decisions" && check) return String(check.decisionLog.records);
    if (id === "literature" && check?.literatureCorpus) return String(check.literatureCorpus.records);
    if (id === "evidence" && check?.evidenceLedger) return String(check.evidenceLedger.claims);
    if (id === "sources" && check?.sourceManifest)
      return String(check.sourceManifest.verifiedSnapshots);
    if (id === "artifacts" && check) return `${presentArtifacts}/${mission.deliverables.length}`;
    if (id === "review" && check) {
      if (!check.readyForReview) return String(check.issues.length);
      if (check.evidenceReview?.unreviewedEvidenceIds.length)
        return String(check.evidenceReview.unreviewedEvidenceIds.length);
      if (check.claimPassports) {
        const unresolved = check.claimPassports.filter(
          (passport) =>
            passport.status !== CLAIM_STATUS.supported &&
            passport.status !== CLAIM_STATUS.qualified,
        ).length;
        return unresolved === 0 ? "✓" : String(unresolved);
      }
      return "✓";
    }
    return null;
  };

  const gateCopy = (key: string) => {
    const copy = STUDY_GATE_COPY[key as keyof typeof STUDY_GATE_COPY];
    return copy
      ? { title: t(copy.title), description: t(copy.description) }
      : { title: key, description: t("researchWorkspace.gate.generic") };
  };

  const approveCurrentProtocol = async () => {
    if (!onApproveProtocol) return;
    setConfirmApproval(false);
    setApproving(true);
    try {
      await onApproveProtocol();
      toast.success(t("researchWorkspace.gate.approvalRecorded"));
    } catch (error) {
      toast.error(
        t("researchWorkspace.gate.approvalFailed", {
          message: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setApproving(false);
    }
  };

  const nextActionCopy = (action: MissionNextAction) =>
    t(`researchWorkspace.nextActions.${action.key}`, {
      target: action.target ?? "",
      count: Number(action.target) || 0,
    });

  const transition = async (action: MissionAction) => {
    if (!onTransition) return;
    setTransitioning(true);
    try {
      await onTransition(action, `Researcher selected ${action}`);
    } catch (error) {
      toast.error(
        t("researchWorkspace.lifecycle.failed", {
          message: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setTransitioning(false);
    }
  };

  return (
    <section className="research-status overflow-hidden border border-border bg-surface shadow-card">
      <div className="flex items-center gap-2 border-b border-border px-3 py-2.5 sm:px-4">
        <div className="min-w-0 flex-1">
          <div className="truncate font-serif text-[17px] leading-tight text-text">
            {t(MISSION_COPY_KEY[mission.kind])}
          </div>
          <div
            className="mt-0.5 flex items-center gap-2 font-mono text-[9px] uppercase tracking-[0.12em] text-muted"
            title={mission.statusReason}
          >
            <span>{t(RIGOR_COPY_KEY[mission.rigor])}</span>
            <span aria-hidden>·</span>
            <span>{t(`researchWorkspace.status.${mission.status}`)}</span>
          </div>
        </div>
        {onTransition &&
          (["paused", "interrupted"].includes(mission.status) ? (
            <button
              type="button"
              disabled={transitioning}
              onClick={() => void transition("resume")}
              className="flex shrink-0 items-center gap-1.5 border border-border px-2.5 py-1.5 text-[10px] text-muted outline-none transition-colors hover:border-accent hover:text-text focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
            >
              <Play size={11} />
              {t("researchWorkspace.lifecycle.resume")}
            </button>
          ) : mission.status === "review-ready" ? (
            <button
              type="button"
              disabled={transitioning}
              onClick={() => void transition("complete")}
              className="flex shrink-0 items-center gap-1.5 bg-accent px-2.5 py-1.5 text-[10px] font-medium text-accent-fg outline-none transition-opacity hover:opacity-90 focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
            >
              <Check size={11} />
              {t("researchWorkspace.lifecycle.complete")}
            </button>
          ) : ["running", "waiting-for-input", "waiting-for-approval"].includes(
              mission.status,
            ) ? (
            <button
              type="button"
              disabled={transitioning}
              onClick={() => void transition("pause")}
              className="flex shrink-0 items-center gap-1.5 border border-border px-2.5 py-1.5 text-[10px] text-muted outline-none transition-colors hover:border-accent hover:text-text focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
            >
              <Pause size={11} />
              {t("researchWorkspace.lifecycle.pause")}
            </button>
          ) : null)}
        <button
          type="button"
          disabled={checking}
          onClick={onRefresh}
          className="flex shrink-0 items-center gap-1.5 border border-border px-2.5 py-1.5 font-mono text-[9px] uppercase tracking-[0.1em] text-muted outline-none transition-colors hover:border-accent hover:text-text focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-wait disabled:opacity-50 motion-reduce:transition-none"
        >
          <RefreshCw size={11} className={checking ? "animate-spin" : undefined} />
          {checking ? t("researchWorkspace.checking") : t("researchWorkspace.refresh")}
        </button>
      </div>

      <div className="no-scrollbar flex overflow-x-auto border-b border-border" role="tablist">
        {DOMAINS.map(({ id, icon: Icon }) => {
          const active = domain === id;
          const badge = badgeFor(id);
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => setDomain(id)}
              className={cn(
                "relative flex min-w-[112px] flex-1 items-center justify-center gap-2 px-3 py-3 text-[11px] outline-none transition-colors focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent motion-reduce:transition-none",
                active ? "bg-accent/[0.07] text-text" : "text-muted hover:bg-surface-2 hover:text-text",
              )}
            >
              <Icon size={14} strokeWidth={1.55} className={active ? "text-accent" : undefined} />
              {t(`researchWorkspace.tabs.${id}`)}
              {badge !== null && (
                <span className="font-mono text-[9px] text-accent">{badge}</span>
              )}
              {active && <span className="absolute inset-x-0 bottom-0 h-0.5 bg-accent" />}
            </button>
          );
        })}
      </div>

      <div role="tabpanel" className="min-h-[104px] px-4 py-4 sm:px-5">
        {domain === "mission" && (
          <div>
            <div className="flex flex-wrap items-end justify-between gap-2">
              <div>
                <div className="font-mono text-[9px] uppercase tracking-[0.15em] text-muted">
                  {t("researchWorkspace.gate.progress")}
                </div>
                <p className="mt-1 text-xs leading-5 text-text/85">
                  {t("researchWorkspace.gates", { passed: passedGates, total: gates.length })}
                </p>
              </div>
              {check?.protocol && !check.protocol.complete && (
                <span className="rounded-full bg-warn/10 px-2.5 py-1 font-mono text-[9px] text-warn">
                  {t("researchWorkspace.gate.unresolved", {
                    count: check.protocol.unresolvedItems + check.protocol.uncheckedItems,
                  })}
                </span>
              )}
            </div>

            <ol className="mt-3 grid gap-px overflow-hidden rounded-[12px] border border-border bg-border sm:grid-cols-3">
              {gates.map((gate, index) => {
                const copy = gateCopy(gate.key);
                const passed = gate.status === "passed";
                const current = nextGate?.key === gate.key;
                return (
                  <li
                    key={gate.key}
                    className={cn(
                      "relative min-w-0 bg-surface px-3 py-3",
                      current && "bg-accent/[0.055]",
                    )}
                  >
                    <div className="flex items-center gap-2">
                      <span
                        className={cn(
                          "grid h-5 w-5 shrink-0 place-items-center rounded-full border font-mono text-[8px]",
                          passed
                            ? "border-accent bg-accent text-accent-fg"
                            : current
                              ? "border-accent text-accent"
                              : "border-border text-muted",
                        )}
                      >
                        {passed ? <Check size={10} strokeWidth={2.2} /> : index + 1}
                      </span>
                      <span className="truncate text-[10.5px] font-medium text-text">
                        {copy.title}
                      </span>
                    </div>
                    <p className="mt-2 line-clamp-2 text-[9.5px] leading-4 text-muted">
                      {copy.description}
                    </p>
                    <div className="mt-2 font-mono text-[7.5px] uppercase tracking-[0.12em] text-muted/75">
                      {passed
                        ? t("researchWorkspace.gate.verified")
                        : current
                          ? t("researchWorkspace.gate.current")
                          : t("researchWorkspace.gate.pending")}
                    </div>
                  </li>
                );
              })}
            </ol>

            {check && check.nextActions.length > 0 && (
              <div className="mt-3 border border-border bg-surface-2/45 px-3.5 py-3">
                <div className="flex items-center justify-between gap-3">
                  <div className="font-mono text-[8px] uppercase tracking-[0.14em] text-muted">
                    {t("researchWorkspace.nextActions.title")}
                  </div>
                  <span className="font-mono text-[8px] text-muted">
                    {check.nextActions.length}
                  </span>
                </div>
                <ol className="mt-2 space-y-1.5">
                  {check.nextActions.map((action, index) => (
                    <li key={`${action.key}-${action.target ?? index}`} className="flex gap-2.5">
                      <span className="mt-0.5 font-mono text-[8px] text-accent">
                        {String(index + 1).padStart(2, "0")}
                      </span>
                      <span className="min-w-0 flex-1 text-[10.5px] leading-4 text-text/85">
                        {nextActionCopy(action)}
                      </span>
                      <span
                        className={cn(
                          "shrink-0 font-mono text-[8px] uppercase tracking-[0.08em]",
                          action.owner === "researcher" ? "text-warn" : "text-accent",
                        )}
                      >
                        {t(`researchWorkspace.nextActions.owner.${action.owner}`)}
                      </span>
                    </li>
                  ))}
                </ol>
              </div>
            )}

            <div className="mt-3 flex flex-col gap-3 rounded-[11px] border border-accent/20 bg-accent/[0.045] px-3.5 py-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <div className="font-mono text-[8px] uppercase tracking-[0.14em] text-accent">
                  {nextGate
                    ? t("researchWorkspace.gate.next")
                    : t("researchWorkspace.gate.complete")}
                </div>
                <div className="mt-1 text-[11.5px] font-medium text-text">
                  {nextGate
                    ? gateCopy(nextGate.key).title
                    : t("researchWorkspace.gate.allVerified")}
                </div>
                {nextGate && (
                  <p className="mt-0.5 text-[9.5px] leading-4 text-muted">
                    {gateCopy(nextGate.key).description}
                  </p>
                )}
              </div>
              {nextGate?.key === "approval-before-outcomes" && check?.protocol?.complete && (
                <button
                  type="button"
                  disabled={approving || !onApproveProtocol}
                  onClick={() => setConfirmApproval(true)}
                  className="shrink-0 rounded-[9px] bg-accent px-3 py-2 text-[10px] font-semibold text-accent-fg outline-none transition-opacity hover:opacity-90 focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 disabled:opacity-45"
                >
                  {approving
                    ? t("researchWorkspace.gate.approving")
                    : t("researchWorkspace.gate.approve")}
                </button>
              )}
            </div>
          </div>
        )}

        {domain === "decisions" && (
          check ? (
            <DecisionLogPanel log={check.decisionLog} onRecord={onResearchDecision} />
          ) : (
            <PendingState message={t("researchWorkspace.notChecked")} />
          )
        )}

        {domain === "literature" && (
          !evidenceRequired ? (
            <PendingState message={t("researchWorkspace.literature.notRequired")} />
          ) : check?.literatureCorpus ? (
            <LiteraturePanel
              corpus={check.literatureCorpus}
              onSearch={onLiteratureSearch}
              onCapture={onLiteratureCapture}
            />
          ) : (
            <PendingState message={t("researchWorkspace.notChecked")} />
          )
        )}

        {domain === "evidence" && (
          !evidenceRequired ? (
            <PendingState message={t("researchWorkspace.notRequired")} />
          ) : check?.evidenceLedger ? (
            <EvidenceInspector
              ledger={check.evidenceLedger}
              review={check.evidenceReview}
              passports={check.claimPassports ?? []}
              onDecision={onEvidenceDecision}
            />
          ) : (
            <PendingState message={t("researchWorkspace.notChecked")} />
          )
        )}

        {domain === "sources" && (
          !evidenceRequired ? (
            <PendingState message={t("researchWorkspace.notRequired")} />
          ) : check?.sourceManifest ? (
            <SourceVault manifest={check.sourceManifest} />
          ) : (
            <PendingState message={t("researchWorkspace.notChecked")} />
          )
        )}

        {domain === "artifacts" && (
          <div>
            <p className="text-xs text-text/85">
              {check
                ? t("researchWorkspace.artifacts", {
                    present: presentArtifacts,
                    total: mission.deliverables.length,
                  })
                : t("researchWorkspace.notChecked")}
            </p>
            <div className="mt-3 flex flex-wrap gap-1.5">
              {mission.deliverables.map((path) => (
                <span
                  key={path}
                  className={cn(
                    "border px-2 py-1 font-mono text-[8.5px]",
                    check && !missing.includes(path)
                      ? "border-accent/35 text-accent"
                      : "border-border text-muted",
                  )}
                >
                  {path}
                </span>
              ))}
            </div>
          </div>
        )}

        {domain === "review" && (
          check ? (
            <div>
              <div className={cn("font-serif text-lg", check.readyForReview ? "text-accent" : "text-text")}>
                {check.readyForReview
                  ? t("researchWorkspace.ready")
                  : t("researchWorkspace.blocked", { count: check.issues.length })}
              </div>
              {check.issues.length > 0 && (
                <ul className="mt-2 max-h-24 space-y-1 overflow-y-auto text-[10.5px] leading-4 text-muted">
                  {check.issues.map((issue) => <li key={issue}>· {issue}</li>)}
                </ul>
              )}
              {check.runIntegrity && <MissionRunIntegritySummary integrity={check.runIntegrity} />}
              {check.evidenceReview && (
                <ReviewProgress review={check.evidenceReview} total={check.evidenceLedger?.records ?? 0} />
              )}
              {check.claimPassports && <PassportReviewSummary passports={check.claimPassports} />}
              <ResearchReleaseSeal check={check} onCreateRelease={onCreateRelease} />
            </div>
          ) : (
            <PendingState message={t("researchWorkspace.notChecked")} />
          )
        )}
      </div>
      {confirmApproval && (
        <ConfirmDialog
          title={t("researchWorkspace.gate.confirm.title")}
          body={t("researchWorkspace.gate.confirm.body")}
          confirmLabel={t("researchWorkspace.gate.confirm.confirm")}
          onConfirm={() => void approveCurrentProtocol()}
          onCancel={() => setConfirmApproval(false)}
        />
      )}
    </section>
  );
}

function MissionRunIntegritySummary({
  integrity,
}: {
  integrity: NonNullable<MissionCheck["runIntegrity"]>;
}) {
  const { t } = useTranslation("session");
  return (
    <div
      className={cn(
        "mt-3 border-l-2 px-3 py-2.5",
        integrity.attentionRuns > 0
          ? "border-l-error bg-error/5"
          : "border-l-accent bg-accent/[0.045]",
      )}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="font-mono text-[8px] uppercase tracking-[0.13em] text-muted">
          {t("researchWorkspace.runIntegrity.title")}
        </span>
        <span className="font-mono text-[8px] text-muted">
          {t("researchWorkspace.runIntegrity.summary", {
            runs: integrity.runsChecked,
            attention: integrity.attentionRuns,
            aligned: integrity.alignedRuns,
          })}
        </span>
      </div>
      {integrity.findings.length > 0 ? (
        <ul className="mt-2 max-h-28 space-y-2 overflow-y-auto">
          {integrity.findings.map((finding) => (
            <li key={`${finding.kind}-${finding.path}-${finding.line}`}>
              <div className="text-[10.5px] font-medium text-text">{finding.title}</div>
              <div className="mt-0.5 text-[9.5px] leading-4 text-muted">
                {finding.evidence}
              </div>
              <div className="mt-0.5 font-mono text-[8px] text-error">
                {finding.path}:{finding.line}
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-1.5 text-[10px] text-muted">
          {integrity.noPlanRuns > 0
            ? t("researchWorkspace.runIntegrity.noPlan", { count: integrity.noPlanRuns })
            : t("researchWorkspace.runIntegrity.aligned")}
        </p>
      )}
    </div>
  );
}

function LiteraturePanel({
  corpus,
  onSearch,
  onCapture,
}: {
  corpus: LiteratureCorpusCheck;
  onSearch: (query: string) => Promise<LiteratureSearchResult>;
  onCapture: (work: LiteratureWork) => Promise<LiteratureImportResult>;
}) {
  const { t } = useTranslation("session");
  const [current, setCurrent] = useState(corpus);
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<LiteratureSearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [capturing, setCapturing] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const captured = new Set(current.entries.map((entry) => entry.canonicalId));

  const search = async () => {
    if (query.trim().length < 2 || searching) return;
    setSearching(true);
    setError(null);
    try {
      setResult(await onSearch(query.trim()));
    } catch (searchError) {
      setError(searchError instanceof Error ? searchError.message : String(searchError));
    } finally {
      setSearching(false);
    }
  };

  const capture = async (work: LiteratureWork) => {
    setCapturing(work.doi);
    setError(null);
    try {
      const imported = await onCapture(work);
      setCurrent(imported.corpus);
    } catch (captureError) {
      setError(captureError instanceof Error ? captureError.message : String(captureError));
    } finally {
      setCapturing(null);
    }
  };

  return (
    <div>
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
        <label className="block min-w-0">
          <span className="mb-1 block font-mono text-[9px] uppercase tracking-[0.12em] text-muted">
            {t("researchWorkspace.literature.searchLabel")}
          </span>
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void search();
              }
            }}
            placeholder={t("researchWorkspace.literature.searchPlaceholder")}
            className="min-h-10 w-full border border-border bg-surface px-3 text-xs text-text outline-none placeholder:text-muted/60 focus:border-accent"
          />
        </label>
        <button
          type="button"
          disabled={query.trim().length < 2 || searching}
          onClick={() => void search()}
          className="min-h-10 border border-accent bg-accent px-4 font-mono text-[9px] uppercase tracking-[0.1em] text-accent-fg disabled:cursor-not-allowed disabled:border-border disabled:bg-surface disabled:text-muted"
        >
          {searching ? t("researchWorkspace.literature.searching") : t("researchWorkspace.literature.search")}
        </button>
      </div>

      <div className="mt-3 flex flex-wrap gap-1.5">
        <LiteratureMetric label={t("researchWorkspace.literature.corpus", { count: current.records })} />
        <LiteratureMetric label={t("researchWorkspace.literature.fullText", { count: current.fullTextSnapshots })} good />
        <LiteratureMetric label={t("researchWorkspace.literature.abstractOnly", { count: current.abstractSnapshots })} />
        <LiteratureMetric label={t("researchWorkspace.literature.metadataOnly", { count: current.metadataSnapshots })} />
      </div>

      {result && (
        <div className="mt-4">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border pb-2 font-mono text-[8px] uppercase tracking-[0.1em] text-muted">
            <span>{t("researchWorkspace.literature.results", { count: result.works.length })}</span>
            <span>
              {result.provider}
              {result.duplicatesRemoved > 0 &&
                ` · ${t("researchWorkspace.literature.deduplicated", { count: result.duplicatesRemoved })}`}
            </span>
          </div>
          {result.works.length === 0 ? (
            <p className="py-6 text-center text-xs text-muted">{t("researchWorkspace.literature.noResults")}</p>
          ) : (
            <ol className="divide-y divide-border">
              {result.works.map((work) => {
                const isCaptured = captured.has(work.doi.toLowerCase());
                return (
                  <li key={work.doi} className="grid gap-3 py-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start">
                    <div className="min-w-0">
                      <button
                        type="button"
                        onClick={() => void openExternal(work.landingUrl)}
                        className="text-left font-serif text-[15px] leading-5 text-text hover:text-link hover:underline"
                      >
                        {work.title}
                      </button>
                      <p className="mt-0.5 truncate text-[10px] text-muted">
                        {[work.authors.slice(0, 4).join(", "), work.containerTitle, work.year]
                          .filter(Boolean)
                          .join(" · ")}
                      </p>
                      <div className="mt-1 flex flex-wrap gap-1.5 font-mono text-[8px] text-muted">
                        <span>{work.doi}</span>
                        <span aria-hidden>·</span>
                        <span>
                          {work.fullTextUrls.length > 0
                            ? t("researchWorkspace.literature.fullTextCandidate")
                            : work.abstractText
                              ? t("researchWorkspace.literature.abstractCandidate")
                              : t("researchWorkspace.literature.metadataCandidate")}
                        </span>
                      </div>
                    </div>
                    <button
                      type="button"
                      disabled={isCaptured || capturing !== null}
                      onClick={() => void capture(work)}
                      className="min-h-8 border border-border px-2.5 font-mono text-[8px] uppercase tracking-[0.09em] text-text hover:border-accent disabled:cursor-not-allowed disabled:text-muted"
                    >
                      {isCaptured
                        ? t("researchWorkspace.literature.captured")
                        : capturing === work.doi
                          ? t("researchWorkspace.literature.capturing")
                          : t("researchWorkspace.literature.capture")}
                    </button>
                  </li>
                );
              })}
            </ol>
          )}
        </div>
      )}

      {current.entries.length > 0 && (
        <div className="mt-4 border-t border-border pt-3">
          <div className="font-mono text-[8px] uppercase tracking-[0.1em] text-muted">
            {t("researchWorkspace.literature.capturedSources")}
          </div>
          <ul className="mt-2 grid gap-2 sm:grid-cols-2">
            {[...current.entries].reverse().map((entry) => (
              <li key={entry.canonicalId} className="min-w-0 border border-border bg-surface-2/40 px-2.5 py-2">
                <div className="truncate text-[11px] font-medium text-text" title={entry.work.title}>{entry.work.title}</div>
                <div className="mt-1 flex flex-wrap items-center gap-1.5 font-mono text-[8px] text-muted">
                  <span className={entry.snapshotStatus === "full-text" ? "text-ok" : undefined}>
                    {t(`researchWorkspace.literature.status.${entry.snapshotStatus}`)}
                  </span>
                  <span aria-hidden>·</span>
                  <span className="truncate" title={entry.snapshotPath}>{entry.snapshotPath}</span>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}
      {error && <p className="mt-3 text-[10px] leading-4 text-error">{error}</p>}
    </div>
  );
}

function LiteratureMetric({ label, good }: { label: string; good?: boolean }) {
  return (
    <span className={cn("border px-2 py-1 font-mono text-[8px]", good ? "border-ok/35 text-ok" : "border-border text-muted")}>
      {label}
    </span>
  );
}

function DecisionLogPanel({
  log,
  onRecord,
}: {
  log: DecisionLogCheck;
  onRecord: (decision: NewResearchDecision) => Promise<DecisionLogCheck>;
}) {
  const { t } = useTranslation("session");
  const [current, setCurrent] = useState(log);
  const [title, setTitle] = useState("");
  const [choice, setChoice] = useState("");
  const [rationale, setRationale] = useState("");
  const [alternatives, setAlternatives] = useState("");
  const [impact, setImpact] = useState("");
  const [supersedes, setSupersedes] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const canSave = title.trim() && choice.trim() && rationale.trim() && !saving;

  const save = async () => {
    if (!canSave) return;
    setSaving(true);
    setError(null);
    try {
      const next = await onRecord({
        title: title.trim(),
        choice: choice.trim(),
        rationale: rationale.trim(),
        alternatives: alternatives
          .split("\n")
          .map((value) => value.trim())
          .filter(Boolean),
        impact: impact.trim() || undefined,
        supersedes: supersedes || undefined,
      });
      setCurrent(next);
      setTitle("");
      setChoice("");
      setRationale("");
      setAlternatives("");
      setImpact("");
      setSupersedes("");
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : String(saveError));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(260px,0.85fr)_minmax(0,1.15fr)]">
      <form
        className="space-y-2.5"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <div>
          <div className="font-serif text-[17px] text-text">{t("researchWorkspace.decisionLog.title")}</div>
          <p className="mt-1 text-[10.5px] leading-4 text-muted">
            {t("researchWorkspace.decisionLog.description")}
          </p>
        </div>
        <DecisionField
          label={t("researchWorkspace.decisionLog.question")}
          value={title}
          onChange={setTitle}
          maxLength={160}
        />
        <DecisionField
          label={t("researchWorkspace.decisionLog.choice")}
          value={choice}
          onChange={setChoice}
          maxLength={400}
          multiline
        />
        <DecisionField
          label={t("researchWorkspace.decisionLog.rationale")}
          value={rationale}
          onChange={setRationale}
          maxLength={2000}
          multiline
        />
        <DecisionField
          label={t("researchWorkspace.decisionLog.alternatives")}
          value={alternatives}
          onChange={setAlternatives}
          maxLength={2400}
          multiline
          optional
        />
        <DecisionField
          label={t("researchWorkspace.decisionLog.impact")}
          value={impact}
          onChange={setImpact}
          maxLength={1000}
          multiline
          optional
        />
        {current.decisions.length > 0 && (
          <label className="block text-[10px] text-muted">
            <span className="mb-1 block font-mono uppercase tracking-[0.09em]">
              {t("researchWorkspace.decisionLog.supersedes")}
            </span>
            <select
              value={supersedes}
              onChange={(event) => setSupersedes(event.target.value)}
              className="min-h-9 w-full border border-border bg-surface px-2 text-xs text-text outline-none focus:border-accent"
            >
              <option value="">{t("researchWorkspace.decisionLog.none")}</option>
              {current.decisions.map((decision) => (
                <option key={decision.decisionId} value={decision.decisionId}>
                  {decision.title}
                </option>
              ))}
            </select>
          </label>
        )}
        <button
          type="submit"
          disabled={!canSave}
          className="min-h-9 border border-accent bg-accent px-3 py-2 font-mono text-[9px] uppercase tracking-[0.1em] text-accent-fg disabled:cursor-not-allowed disabled:border-border disabled:bg-surface disabled:text-muted"
        >
          {saving ? t("researchWorkspace.decisionLog.saving") : t("researchWorkspace.decisionLog.save")}
        </button>
        {error && <p className="text-[10px] leading-4 text-error">{error}</p>}
      </form>

      <div className="min-w-0">
        <div className="flex items-center justify-between gap-2 border-b border-border pb-2">
          <span className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted">
            {t("researchWorkspace.decisionLog.history", { count: current.records })}
          </span>
          <span className="truncate font-mono text-[8px] text-muted" title={current.path}>{current.path}</span>
        </div>
        {current.decisions.length === 0 ? (
          <p className="py-6 text-center text-xs text-muted">{t("researchWorkspace.decisionLog.empty")}</p>
        ) : (
          <ol className="max-h-[520px] space-y-3 overflow-y-auto py-3">
            {[...current.decisions].reverse().map((decision) => (
              <li key={decision.decisionId} className="border-l-2 border-accent/45 pl-3">
                <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                  <h4 className="font-serif text-[15px] text-text">{decision.title}</h4>
                  <time className="font-mono text-[8px] text-muted">
                    {new Date(decision.decidedAt * 1000).toLocaleString()}
                  </time>
                </div>
                <p className="mt-1 text-xs font-medium leading-5 text-text">{decision.choice}</p>
                <p className="mt-1 text-[10.5px] leading-4 text-muted">{decision.rationale}</p>
                {decision.alternatives.length > 0 && (
                  <p className="mt-1.5 text-[9.5px] leading-4 text-muted">
                    {t("researchWorkspace.decisionLog.alternativesLabel", {
                      values: decision.alternatives.join(" · "),
                    })}
                  </p>
                )}
                {decision.impact && (
                  <p className="mt-1 text-[9.5px] leading-4 text-muted">
                    {t("researchWorkspace.decisionLog.impactLabel", { value: decision.impact })}
                  </p>
                )}
                {decision.supersedes && (
                  <span className="mt-1 inline-block font-mono text-[8px] text-accent">
                    {t("researchWorkspace.decisionLog.supersedesLabel", { id: decision.supersedes })}
                  </span>
                )}
              </li>
            ))}
          </ol>
        )}
      </div>
    </div>
  );
}

function DecisionField({
  label,
  value,
  onChange,
  maxLength,
  multiline,
  optional,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  maxLength: number;
  multiline?: boolean;
  optional?: boolean;
}) {
  const { t } = useTranslation("session");
  const className =
    "w-full border border-border bg-surface px-2.5 py-2 text-xs text-text outline-none placeholder:text-muted/60 focus:border-accent";
  return (
    <label className="block text-[10px] text-muted">
      <span className="mb-1 flex items-center gap-1 font-mono uppercase tracking-[0.09em]">
        {label}
        {optional && <span className="normal-case tracking-normal opacity-70">({t("researchWorkspace.decisionLog.optional")})</span>}
      </span>
      {multiline ? (
        <textarea
          value={value}
          onChange={(event) => onChange(event.target.value)}
          maxLength={maxLength}
          rows={2}
          className={cn(className, "resize-y")}
        />
      ) : (
        <input
          value={value}
          onChange={(event) => onChange(event.target.value)}
          maxLength={maxLength}
          className={className}
        />
      )}
    </label>
  );
}

function ResearchReleaseSeal({
  check,
  onCreateRelease,
}: {
  check: MissionCheck;
  onCreateRelease: () => Promise<ResearchRelease>;
}) {
  const { t } = useTranslation("session");
  const [release, setRelease] = useState<ResearchRelease | null>(null);
  const [packagePath, setPackagePath] = useState("");
  const [verification, setVerification] = useState<ReleaseVerification | null>(null);
  const [imported, setImported] = useState<ReleaseImport | null>(null);
  const [creating, setCreating] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const passports = check.claimPassports ?? [];
  const unresolved = passports.filter(
    (passport) =>
      passport.status !== CLAIM_STATUS.supported && passport.status !== CLAIM_STATUS.qualified,
  ).length;
  const finalReview =
    !check.evidenceReview ||
    (check.evidenceReview.unreviewedEvidenceIds.length === 0 && check.evidenceReview.needsReview === 0);
  const releasable = check.readyForReview && check.issues.length === 0 && unresolved === 0 && finalReview;

  const create = async () => {
    setCreating(true);
    setError(null);
    try {
      const created = await onCreateRelease();
      setRelease(created);
      setPackagePath(created.path);
      setVerification(null);
      setImported(null);
    } catch (releaseError) {
      setError(releaseError instanceof Error ? releaseError.message : String(releaseError));
    } finally {
      setCreating(false);
    }
  };

  const verify = async () => {
    const path = packagePath.trim();
    if (!path) return;
    setVerifying(true);
    setError(null);
    setImported(null);
    try {
      setVerification(await verifyResearchRelease(path));
    } catch (verificationError) {
      setError(
        verificationError instanceof Error ? verificationError.message : String(verificationError),
      );
    } finally {
      setVerifying(false);
    }
  };

  const importPackage = async () => {
    const path = packagePath.trim();
    if (!path) return;
    setImporting(true);
    setError(null);
    try {
      const result = await importResearchRelease(path);
      setImported(result);
      setVerification(await verifyResearchRelease(path));
    } catch (importError) {
      setError(importError instanceof Error ? importError.message : String(importError));
    } finally {
      setImporting(false);
    }
  };

  const present = async () => {
    if (!release) return;
    setError(null);
    try {
      if (!(await presentArtifact(release.path, release.fileName))) {
        setError(t("researchWorkspace.release.unavailable"));
      }
    } catch (presentError) {
      setError(presentError instanceof Error ? presentError.message : String(presentError));
    }
  };

  return (
    <aside
      className={cn(
        "mt-4 border-l-2 border-y border-r bg-surface-2/35 px-3 py-3 sm:px-4",
        release || releasable ? "border-accent/45 border-l-accent" : "border-border border-l-error/55",
      )}
    >
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-[8px] uppercase tracking-[0.13em] text-muted">
            <span>{t("researchWorkspace.release.eyebrow")}</span>
            <span aria-hidden>·</span>
            <span>{t("researchWorkspace.release.version")}</span>
            <span
              className={cn(
                "ml-auto sm:ml-0",
                release || releasable ? "text-ok" : "text-error",
              )}
            >
              {release
                ? t("researchWorkspace.release.sealed")
                : releasable
                  ? t("researchWorkspace.release.ready")
                  : t("researchWorkspace.release.blocked")}
            </span>
          </div>
          <h3 className="mt-1.5 font-serif text-[17px] leading-5 text-text">
            {t("researchWorkspace.release.title")}
          </h3>
          <p className="mt-1 max-w-2xl text-[10.5px] leading-4 text-text/70">
            {releasable || release
              ? t("researchWorkspace.release.description")
              : t("researchWorkspace.release.blockedHint")}
          </p>
        </div>
        <div className="flex shrink-0 flex-wrap gap-2">
          <button
            type="button"
            disabled={!releasable || creating}
            onClick={() => void create()}
            className="flex min-h-9 items-center gap-1.5 border border-accent bg-accent px-3 py-2 font-mono text-[8px] uppercase tracking-[0.1em] text-accent-fg outline-none transition-colors hover:bg-accent/90 focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-not-allowed disabled:border-border disabled:bg-surface disabled:text-muted disabled:opacity-60 motion-reduce:transition-none"
          >
            <PackageCheck size={12} aria-hidden />
            {creating
              ? t("researchWorkspace.release.generating")
              : t("researchWorkspace.release.generate")}
          </button>
          {release && (
            <button
              type="button"
              onClick={() => void present()}
              className="flex min-h-9 items-center gap-1.5 border border-border bg-surface px-3 py-2 font-mono text-[8px] uppercase tracking-[0.1em] text-text outline-none transition-colors hover:border-accent focus-visible:ring-2 focus-visible:ring-accent motion-reduce:transition-none"
            >
              <FolderOpen size={12} aria-hidden />
              {t("researchWorkspace.release.get")}
            </button>
          )}
        </div>
      </div>

      {release && (
        <div className="mt-3 grid gap-px overflow-hidden border border-border bg-border sm:grid-cols-[auto_auto_auto_minmax(0,1fr)]">
          <ReleaseDatum label={t("researchWorkspace.release.files", { count: release.payloadFiles })} />
          <ReleaseDatum label={t("researchWorkspace.release.claims", { count: release.claimPassports })} />
          <ReleaseDatum label={formatBytes(release.payloadBytes)} />
          <div className="min-w-0 bg-surface px-2.5 py-2">
            <div className="font-mono text-[7px] uppercase tracking-[0.12em] text-muted">
              {t("researchWorkspace.release.fingerprint")}
            </div>
            <div className="mt-1 flex min-w-0 items-center gap-1.5 text-accent" title={release.fingerprint}>
              <Fingerprint size={10} className="shrink-0" aria-hidden />
              <span className="truncate font-mono text-[8px]">{release.fingerprint}</span>
            </div>
          </div>
        </div>
      )}
      <div className="mt-3 border border-border bg-surface/70 p-2.5">
        <div className="font-mono text-[7px] uppercase tracking-[0.12em] text-muted">
          {t("researchWorkspace.release.verifyTitle")}
        </div>
        <p className="mt-1 text-[10px] leading-4 text-text/70">
          {t("researchWorkspace.release.verifyHint")}
        </p>
        <div className="mt-2 flex flex-col gap-2 sm:flex-row">
          <input
            value={packagePath}
            onChange={(event) => {
              setPackagePath(event.target.value);
              setVerification(null);
              setImported(null);
            }}
            placeholder={t("researchWorkspace.release.pathPlaceholder")}
            aria-label={t("researchWorkspace.release.pathLabel")}
            className="min-h-9 min-w-0 flex-1 border border-border bg-surface px-2.5 font-mono text-[9px] text-text outline-none placeholder:text-muted focus:border-accent focus:ring-1 focus:ring-accent"
          />
          <button
            type="button"
            disabled={!packagePath.trim() || verifying || importing}
            onClick={() => void verify()}
            className="flex min-h-9 items-center justify-center gap-1.5 border border-border bg-surface px-3 py-2 font-mono text-[8px] uppercase tracking-[0.1em] text-text outline-none transition-colors hover:border-accent focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-50 motion-reduce:transition-none"
          >
            <FileSearch size={12} aria-hidden />
            {verifying
              ? t("researchWorkspace.release.verifying")
              : t("researchWorkspace.release.verify")}
          </button>
          <button
            type="button"
            disabled={!packagePath.trim() || verifying || importing}
            onClick={() => void importPackage()}
            className="flex min-h-9 items-center justify-center gap-1.5 border border-border bg-surface px-3 py-2 font-mono text-[8px] uppercase tracking-[0.1em] text-text outline-none transition-colors hover:border-accent focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-50 motion-reduce:transition-none"
          >
            <ArchiveRestore size={12} aria-hidden />
            {importing
              ? t("researchWorkspace.release.importing")
              : t("researchWorkspace.release.import")}
          </button>
        </div>
        {verification && (
          <div
            className={cn(
              "mt-2 border-l-2 px-2 py-1.5 text-[9px] leading-4",
              verification.valid
                ? "border-l-ok bg-ok/5 text-ok"
                : "border-l-error bg-error/5 text-error",
            )}
          >
            <div className="font-mono uppercase tracking-[0.08em]">
              {verification.valid
                ? t("researchWorkspace.release.verified", {
                    count: verification.payloadFiles,
                  })
                : t("researchWorkspace.release.invalid", {
                    count: verification.issues.length,
                  })}
            </div>
            {!verification.valid && (
              <ul className="mt-1 list-disc pl-4">
                {verification.issues.map((issue) => (
                  <li key={issue}>{issue}</li>
                ))}
              </ul>
            )}
          </div>
        )}
        {imported && (
          <p className="mt-2 break-all border-l-2 border-l-ok bg-ok/5 px-2 py-1.5 font-mono text-[9px] leading-4 text-ok">
            {t("researchWorkspace.release.imported", {
              path: imported.destinationPath,
            })}
          </p>
        )}
      </div>
      {error && <p className="mt-2 text-[10px] leading-4 text-error">{error}</p>}
    </aside>
  );
}

function ReleaseDatum({ label }: { label: string }) {
  return (
    <div className="bg-surface px-2.5 py-2 font-mono text-[8px] uppercase tracking-[0.09em] text-muted">
      {label}
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

function EvidenceInspector({
  ledger,
  review,
  passports,
  onDecision,
}: {
  ledger: NonNullable<MissionCheck["evidenceLedger"]>;
  review: MissionCheck["evidenceReview"];
  passports: ClaimPassport[];
  onDecision: (
    evidenceId: string,
    verdict: EvidenceVerdict,
    note: string,
  ) => Promise<EvidenceReviewCheck>;
}) {
  const { t } = useTranslation("session");
  const claims = Array.from(
    ledger.entries.reduce((groups, entry) => {
      const existing = groups.get(entry.claimId);
      if (existing) existing.entries.push(entry);
      else groups.set(entry.claimId, { claim: entry.claim, entries: [entry] });
      return groups;
    }, new Map<string, { claim: string; entries: typeof ledger.entries }>()),
  );

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-px overflow-hidden border border-border bg-border sm:grid-cols-4">
        <Metric label={t("researchWorkspace.metrics.claims")} value={ledger.claims} />
        <Metric label={t("researchWorkspace.metrics.sources")} value={ledger.sources} />
        <Metric label={t("researchWorkspace.metrics.contested")} value={ledger.contestedClaimIds.length} />
        <Metric label={t("researchWorkspace.metrics.relations")} value={ledger.records} />
      </div>

      {review && <ReviewProgress review={review} total={ledger.records} />}

      {claims.length > 0 ? (
        <div className="max-h-[360px] space-y-3 overflow-y-auto pr-1">
          {claims.map(([claimId, group]) => {
            const contested = ledger.contestedClaimIds.includes(claimId);
            const passport = passports.find((item) => item.claimId === claimId);
            return (
              <article key={claimId} className="border border-border bg-surface-2/35">
                <header className="grid gap-2 border-b border-border px-3 py-3 sm:grid-cols-[104px_minmax(0,1fr)_auto] sm:items-start">
                  <span className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted">
                    {claimId}
                  </span>
                  <h3 className="font-serif text-[15px] leading-5 text-text">{group.claim}</h3>
                  {passport ? (
                    <ClaimPassportStamp passport={passport} />
                  ) : contested ? (
                    <span className="w-fit border border-error/35 bg-error/[0.06] px-2 py-1 font-mono text-[8px] uppercase tracking-[0.1em] text-error">
                      {t("researchWorkspace.contested")}
                    </span>
                  ) : null}
                </header>
                <div className="divide-y divide-border">
                  {group.entries.map((entry) => (
                    <EvidenceRelation
                      key={entry.evidenceId}
                      entry={entry}
                      decision={review?.decisions.find(
                        (decision) => decision.evidenceId === entry.evidenceId,
                      )}
                      onDecision={onDecision}
                    />
                  ))}
                </div>
              </article>
            );
          })}
        </div>
      ) : (
        <PendingState message={t("researchWorkspace.emptyEvidence")} />
      )}
    </div>
  );
}

function ClaimPassportStamp({ passport }: { passport: ClaimPassport }) {
  const { t } = useTranslation("session");
  const reviewed = passport.accepted + passport.rejected + passport.needsReview;
  const relations = passport.supports + passport.contradicts + passport.qualifies;
  const tone =
    passport.status === CLAIM_STATUS.supported
      ? "border-ok/45 bg-ok/[0.06] text-ok"
      : passport.status === CLAIM_STATUS.qualified || passport.status === CLAIM_STATUS.reviewPending
        ? "border-accent/45 bg-accent/[0.06] text-accent"
        : "border-error/45 bg-error/[0.06] text-error";

  return (
    <div className={cn("min-w-[176px] border px-2.5 py-2", tone)}>
      <div className="flex items-center justify-between gap-2 font-mono text-[7.5px] uppercase tracking-[0.11em]">
        <span>{t("researchWorkspace.claimPassport")}</span>
        <span>{t("researchWorkspace.passportVersion")}</span>
      </div>
      <div className="mt-1 font-serif text-[13px] leading-4 text-current">
        {t(`researchWorkspace.claimStatuses.${passport.status}`)}
      </div>
      <div className="mt-1.5 flex flex-wrap gap-x-2 gap-y-0.5 font-mono text-[7.5px] opacity-75">
        <span>{t("researchWorkspace.passportReviewed", { reviewed, total: relations })}</span>
        <span>
          {t("researchWorkspace.passportSources", {
            verified: passport.verifiedSources,
            total: passport.sourceCount,
          })}
        </span>
      </div>
      <div className="mt-1.5 flex min-w-0 items-center gap-1" title={passport.fingerprint}>
        <Fingerprint size={9} className="shrink-0" aria-hidden />
        <span className="truncate font-mono text-[7.5px]">{passport.fingerprint}</span>
      </div>
    </div>
  );
}

function EvidenceRelation({
  entry,
  decision,
  onDecision,
}: {
  entry: NonNullable<MissionCheck["evidenceLedger"]>["entries"][number];
  decision: EvidenceDecision | undefined;
  onDecision: (
    evidenceId: string,
    verdict: EvidenceVerdict,
    note: string,
  ) => Promise<EvidenceReviewCheck>;
}) {
  const { t } = useTranslation("session");
  const [draftVerdict, setDraftVerdict] = useState<
    Exclude<EvidenceVerdict, typeof EVIDENCE_VERDICT.accepted> | null
  >(null);
  const [note, setNote] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async (verdict: EvidenceVerdict, decisionNote: string) => {
    setSaving(true);
    setError(null);
    try {
      await onDecision(entry.evidenceId, verdict, decisionNote);
      setDraftVerdict(null);
      setNote("");
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : String(saveError));
    } finally {
      setSaving(false);
    }
  };

  const openNote = (
    verdict: Exclude<EvidenceVerdict, typeof EVIDENCE_VERDICT.accepted>,
  ) => {
    setDraftVerdict(verdict);
    setNote(decision?.verdict === verdict ? decision.note : "");
    setError(null);
  };

  return (
    <div
      className={cn(
        "grid gap-2 border-l-2 px-3 py-3 sm:grid-cols-[104px_minmax(0,1fr)]",
        entry.stance === "supports" && "border-l-ok",
        entry.stance === "contradicts" && "border-l-error",
        entry.stance === "qualifies" && "border-l-accent",
      )}
    >
      <div>
        <span
          className={cn(
            "font-mono text-[8px] uppercase tracking-[0.1em]",
            entry.stance === "supports" && "text-ok",
            entry.stance === "contradicts" && "text-error",
            entry.stance === "qualifies" && "text-accent",
          )}
        >
          {t(`researchWorkspace.stances.${entry.stance}`)}
        </span>
        <div className="mt-1 truncate font-mono text-[8px] text-muted">{entry.evidenceId}</div>
      </div>
      <div className="min-w-0">
        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
          <span className="text-[11px] font-medium text-text">{entry.source.title}</span>
          <span className="font-mono text-[8.5px] text-muted">{entry.source.locator}</span>
        </div>
        <blockquote className="mt-2 flex gap-2 text-[11px] leading-[1.55] text-text/75">
          <Quote size={12} className="mt-0.5 shrink-0 text-accent" aria-hidden />
          <span>{entry.source.quote}</span>
        </blockquote>
        <div className="mt-2 truncate font-mono text-[8px] text-muted">{entry.source.id}</div>

        <div className="mt-3 flex flex-wrap items-center gap-1.5 border-t border-border pt-2.5">
          <DecisionButton
            icon={Check}
            verdict={EVIDENCE_VERDICT.accepted}
            current={decision?.verdict}
            disabled={saving}
            onClick={() => void save(EVIDENCE_VERDICT.accepted, "")}
          />
          <DecisionButton
            icon={CircleHelp}
            verdict={EVIDENCE_VERDICT.needsReview}
            current={decision?.verdict}
            disabled={saving}
            onClick={() => openNote(EVIDENCE_VERDICT.needsReview)}
          />
          <DecisionButton
            icon={X}
            verdict={EVIDENCE_VERDICT.rejected}
            current={decision?.verdict}
            disabled={saving}
            onClick={() => openNote(EVIDENCE_VERDICT.rejected)}
          />
          {decision?.note && !draftVerdict && (
            <span className="min-w-0 truncate text-[9px] text-muted" title={decision.note}>
              {decision.note}
            </span>
          )}
        </div>

        {draftVerdict && (
          <div className="mt-2 border-l-2 border-accent/45 pl-2.5">
            <textarea
              value={note}
              maxLength={2000}
              rows={2}
              autoFocus
              onChange={(event) => setNote(event.target.value)}
              placeholder={t(`researchWorkspace.decisionNote.${draftVerdict}`)}
              className="w-full resize-y border border-border bg-surface px-2.5 py-2 text-[11px] leading-4 text-text outline-none placeholder:text-muted/70 focus:border-accent focus:ring-1 focus:ring-accent"
            />
            <div className="mt-1.5 flex items-center gap-2">
              <button
                type="button"
                disabled={saving || note.trim().length === 0}
                onClick={() => void save(draftVerdict, note)}
                className="border border-accent bg-accent px-2.5 py-1 font-mono text-[8px] uppercase tracking-[0.1em] text-accent-fg outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-40"
              >
                {saving ? t("researchWorkspace.savingDecision") : t("researchWorkspace.saveDecision")}
              </button>
              <button
                type="button"
                disabled={saving}
                onClick={() => setDraftVerdict(null)}
                className="px-2 py-1 font-mono text-[8px] uppercase tracking-[0.1em] text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent"
              >
                {t("researchWorkspace.cancelDecision")}
              </button>
            </div>
          </div>
        )}
        {error && <p className="mt-2 text-[10px] leading-4 text-error">{error}</p>}
      </div>
    </div>
  );
}

function DecisionButton({
  icon: Icon,
  verdict,
  current,
  disabled,
  onClick,
}: {
  icon: LucideIcon;
  verdict: EvidenceVerdict;
  current: EvidenceVerdict | undefined;
  disabled: boolean;
  onClick: () => void;
}) {
  const { t } = useTranslation("session");
  const active = current === verdict;
  return (
    <button
      type="button"
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex items-center gap-1 border px-2 py-1 font-mono text-[8px] uppercase tracking-[0.08em] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-wait disabled:opacity-50 motion-reduce:transition-none",
        active && verdict === EVIDENCE_VERDICT.accepted && "border-ok/45 bg-ok/[0.08] text-ok",
        active && verdict === EVIDENCE_VERDICT.needsReview && "border-accent/45 bg-accent/[0.08] text-accent",
        active && verdict === EVIDENCE_VERDICT.rejected && "border-error/45 bg-error/[0.08] text-error",
        !active && "border-border text-muted hover:border-accent/50 hover:text-text",
      )}
    >
      <Icon size={10} aria-hidden />
      {t(`researchWorkspace.decisions.${verdict}`)}
    </button>
  );
}

function ReviewProgress({ review, total }: { review: EvidenceReviewCheck; total: number }) {
  const { t } = useTranslation("session");
  const reviewed = review.decisions.length;
  return (
    <div className="grid gap-2 border-l-2 border-accent px-3 py-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <div>
        <div className="font-mono text-[8px] uppercase tracking-[0.13em] text-accent">
          {t("researchWorkspace.adjudication")}
        </div>
        <p className="mt-1 text-[10.5px] text-text/75">
          {t("researchWorkspace.reviewed", { reviewed, total })}
        </p>
      </div>
      <div className="flex flex-wrap gap-2 font-mono text-[8px] uppercase tracking-[0.08em] text-muted">
        <span className="text-ok">{t("researchWorkspace.reviewCounts.accepted", { count: review.accepted })}</span>
        <span className="text-accent">{t("researchWorkspace.reviewCounts.needsReview", { count: review.needsReview })}</span>
        <span className="text-error">{t("researchWorkspace.reviewCounts.rejected", { count: review.rejected })}</span>
      </div>
    </div>
  );
}

function PassportReviewSummary({ passports }: { passports: ClaimPassport[] }) {
  const { t } = useTranslation("session");
  const cleared = passports.filter(
    (passport) =>
      passport.status === CLAIM_STATUS.supported || passport.status === CLAIM_STATUS.qualified,
  ).length;
  const unresolved = passports.length - cleared;
  return (
    <div className="mt-3 border-t border-border pt-3">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span className="font-mono text-[8px] uppercase tracking-[0.13em] text-muted">
          {t("researchWorkspace.claimPassport")}
        </span>
        <span className={cn("font-serif text-[15px]", unresolved === 0 ? "text-ok" : "text-error")}>
          {unresolved === 0
            ? t("researchWorkspace.passportsReady", { count: cleared })
            : t("researchWorkspace.passportsBlocked", { count: unresolved })}
        </span>
      </div>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {passports.map((passport) => (
          <span
            key={passport.claimId}
            className="border border-border px-2 py-1 font-mono text-[8px] text-muted"
            title={passport.fingerprint}
          >
            {passport.claimId} · {t(`researchWorkspace.claimStatuses.${passport.status}`)}
          </span>
        ))}
      </div>
    </div>
  );
}

function SourceVault({
  manifest,
}: {
  manifest: NonNullable<MissionCheck["sourceManifest"]>;
}) {
  const { t, i18n } = useTranslation("session");
  const dateFormatter = new Intl.DateTimeFormat(i18n.resolvedLanguage ?? "en", {
    year: "numeric",
    month: "short",
    day: "numeric",
  });

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-px overflow-hidden border border-border bg-border sm:grid-cols-3">
        <Metric label={t("researchWorkspace.metrics.manifestRecords")} value={manifest.records} />
        <Metric label={t("researchWorkspace.metrics.snapshots")} value={manifest.verifiedSnapshots} />
        <Metric label={t("researchWorkspace.metrics.quotes")} value={manifest.quoteMatches} />
      </div>

      {manifest.entries.length > 0 ? (
        <div className="max-h-[360px] divide-y divide-border overflow-y-auto border border-border">
          {manifest.entries.map((source) => {
            const verified = manifest.verifiedSourceIds.includes(source.sourceId);
            return (
              <article
                key={source.sourceId}
                className="grid gap-3 bg-surface px-3 py-3 sm:grid-cols-[minmax(0,1fr)_minmax(190px,0.7fr)] sm:px-4"
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="font-serif text-[15px] leading-5 text-text">{source.title}</h3>
                    <span
                      className={cn(
                        "flex items-center gap-1 font-mono text-[8px] uppercase tracking-[0.1em]",
                        verified ? "text-ok" : "text-error",
                      )}
                    >
                      <CheckCircle2 size={10} aria-hidden />
                      {verified
                        ? t("researchWorkspace.snapshotVerified")
                        : t("researchWorkspace.snapshotUnverified")}
                    </span>
                  </div>
                  <div className="mt-1 truncate font-mono text-[8.5px] text-muted">
                    {source.sourceId}
                  </div>
                  <div className="mt-2 truncate text-[10px] text-text/65">{source.retrievedUrl}</div>
                  <div className="mt-1 font-mono text-[8px] uppercase tracking-[0.08em] text-muted">
                    {t("researchWorkspace.retrieved", {
                      date: dateFormatter.format(new Date(source.retrievedAt * 1000)),
                    })}
                  </div>
                </div>
                <div className="min-w-0 border-l border-border pl-3">
                  <div className="truncate font-mono text-[8.5px] text-text/75">
                    {source.snapshotPath}
                  </div>
                  <div className="mt-2 flex min-w-0 items-center gap-1.5 text-muted">
                    <Fingerprint size={11} className="shrink-0 text-accent" aria-hidden />
                    <span className="truncate font-mono text-[8px]" title={source.sha256}>
                      {t("researchWorkspace.hash")} {source.sha256}
                    </span>
                  </div>
                </div>
              </article>
            );
          })}
        </div>
      ) : (
        <PendingState message={t("researchWorkspace.emptySources")} />
      )}
    </div>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div className="bg-surface px-3 py-3">
      <div className="font-serif text-[22px] leading-none text-text">{value}</div>
      <div className="mt-1.5 font-mono text-[8.5px] uppercase tracking-[0.11em] text-muted">
        {label}
      </div>
    </div>
  );
}

function PendingState({ message }: { message: string }) {
  return (
    <div className="flex min-h-[70px] items-center border-l-2 border-border pl-4 text-xs leading-5 text-muted">
      {message}
    </div>
  );
}
