import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import {
  ArrowDown,
  Bot,
  FlaskConical,
  FolderOpen,
  Loader2,
  NotebookPen,
  PanelBottom,
  PanelLeft,
  PanelRight,
  PlugZap,
  RotateCcw,
  X,
} from "lucide-react";
import type { RuntimeStatus } from "@ai4s/shared";
import {
  contextLimitFor,
  datedWorkspaceName,
  draftKeyFor,
  inheritedDraftFolder,
  rootSessionOf,
  useRuntimeStore,
} from "@/lib/runtime";
import { useLayoutStore } from "@/lib/layout";
import { startPaneDrag } from "@/lib/dragPane";
import { isGatewayWeb } from "@/lib/webMode";
import { useIsMobile } from "@/lib/useIsMobile";
import { queryRuns } from "@/lib/runs";
import { useOverlayTitlebar, useUiStore } from "@/lib/store";
import { overlayTitlebarStyle } from "@/lib/titlebar";
import { useCompactWidth } from "@/lib/useCompactWidth";
import { fileInspectorFromBlock } from "@/lib/artifacts";
import { useChatScroll } from "@/lib/scrollMemory";
import { useWheelChain } from "@/lib/wheelChain";
import {
  approveProtocol,
  checkMission,
  captureLiterature,
  createResearchRelease,
  decideEvidence,
  listMissions,
  planMission,
  recordResearchDecision,
  searchLiterature,
  startMission,
  transitionMission,
  type DecisionLogCheck,
  type MissionCheck,
  type MissionAction,
  type MissionRecord,
  type LiteratureImportResult,
  type LiteratureSearchResult,
  type LiteratureWork,
  type ResearchRelease,
  type EvidenceReviewCheck,
  type EvidenceVerdict,
  type NewResearchDecision,
} from "@/lib/missions";
import { toast } from "@/lib/toast";
import { BlockList, type BlockHandlers } from "@/components/thread/BlockList";
import { SubagentPane } from "@/components/thread/SubagentPane";
import { SelectionActions } from "@/components/thread/SelectionActions";
import { Elapsed } from "@/components/thread/ToolGroup";
import { Composer } from "@/components/thread/Composer";
import { GoalPill } from "@/components/thread/GoalPill";
import { GOAL_RESUME_NUDGE } from "@/lib/goalPrompts";
import { baseName } from "@/components/thread/WorkspaceChip";
import {
  missionPromptWithBrief,
  missionResumePrompt,
  type ResearchLaunch,
  type ResearchMissionId,
} from "@/lib/researchActions";
import { SplitMenu } from "@/components/session/SplitMenu";
import { InteractionPrompt } from "@/components/thread/InteractionPrompt";
import { InspectorShell } from "@/components/inspector/InspectorShell";
import { MaximizePaneButton, RightPane } from "@/components/inspector/RightPane";
import { SessionFilesPane } from "@/app/routes/FilesPage";
import { RunsPane } from "@/app/routes/RunsPage";
import { cn } from "@/lib/cn";

const ResearchWorkbench = lazy(() =>
  import("@/components/research/ResearchWorkbench").then((module) => ({ default: module.ResearchWorkbench })),
);
const ResearchWorkspaceStatus = lazy(() =>
  import("@/components/research/ResearchWorkspaceStatus").then((module) => ({ default: module.ResearchWorkspaceStatus })),
);

type ThreadBlocks = NonNullable<ReturnType<typeof useRuntimeStore.getState>["threads"][string]>["blocks"];
type ToolCallBlock = Extract<ThreadBlocks[number], { kind: "tool-call" }>;

/** The newest still-running tool step, or undefined. Allocation-free: a live
 *  pane re-derives this on every render, so it must not copy the block list. */
function findLastRunningTool(blocks?: ThreadBlocks): ToolCallBlock | undefined {
  for (let i = (blocks?.length ?? 0) - 1; i >= 0; i--) {
    const b = blocks![i];
    if (b.kind === "tool-call" && b.status === "running") return b;
  }
  return undefined;
}

/**
 * One agent session — header + conversation + composer + optional right pane.
 * Bound to `sessionId` (null = the single draft pane), NOT the global
 * `currentId`, so any number of these tile side-by-side in the pane tree and
 * each streams and sends on its own. The focused-session lifecycle (openSession,
 * URL, reconcile) lives in the LiveSessionPage wrapper, not here.
 */
/** Header width below which the tool buttons show icons without their labels. */
const HEADER_LABEL_MIN_PX = 620;
/** Mission Control shares the composer's transparent fade-in gutter, while
 *  ordinary transcripts reserve the full floating composer height. */
const MISSION_COMPOSER_OVERLAP_PX = 32;
const COMPOSER_CLEARANCE_PX = 12;
const CHAT_CONTENT_TOP_PX = 24;

/** Sessions already known to have (or not have) runs. The Runs toggle used to
 *  appear one async query after mount, and every header control that appears
 *  late steals width from the session title next to it — so switching Screens
 *  painted the full title and then re-truncated it a frame later. Re-mounting a
 *  session already seen this run now paints its final header immediately. */
const RUNS_KNOWN = new Map<string, boolean>();

export function SessionView({
  sessionId,
  leafId,
  focused,
  /** The primary pane doubles as the macOS titlebar and hosts the sidebar
   *  expand button (only one pane may — otherwise every header clears the
   *  traffic lights). In single-pane mode this is always the one view. */
  chromeAsTitlebar = true,
  /** Per-pane content zoom (1 = 100%); scales the conversation + composer. */
  zoom = 1,
  /** The only pane in its group. A tiled (non-solo) pane is narrow, so the
   *  files/artifact inspector fills the pane instead of sitting in a side
   *  column, and header toggles show icon-only. */
  solo = true,
  /** Close this pane (shown as an ✕ in the header). Omitted for the sole pane
   *  and on web/mobile. */
  onClose,
  /** This pane's screen is on display. Inactive screens stay MOUNTED but
   *  hidden, so the pane keeps its state and its stream — but anything that
   *  claims an app-wide hand-off, polls, or opens something the user cannot see
   *  must stand down until its screen is shown again. */
  visible = true,
  /** This pane has layout boxes: it is on display, or its screen is hidden in a
   *  way that keeps them. Everything that MEASURES the DOM keys off this rather
   *  than `visible` — a screen that never lost its layout must not re-measure on
   *  the way back, because forcing that layout mid-commit is what makes a switch
   *  feel heavy (measured: it doubled the React phase). */
  laidOut = true,
}: {
  sessionId: string | null;
  leafId: string;
  focused: boolean;
  chromeAsTitlebar?: boolean;
  zoom?: number;
  solo?: boolean;
  onClose?: () => void;
  visible?: boolean;
  laidOut?: boolean;
}) {
  const { t } = useTranslation(["session", "common"]);
  // `sid` is what this pane WRITES to (null = draft → create on first send).
  // `eid` is what it DISPLAYS: a real pane is its own session, but the focused
  // draft follows `currentId` so a first send's draft→session graft (which moves
  // the thread off the draft slot and sets currentId) never blanks the pane.
  // `key` addresses the per-session maps (threads/panes/agents): a real session
  // by id, else this pane's own `draft:<leafId>` slot.
  const sid = sessionId;
  // Only a focused DRAFT pane follows `currentId`. Selecting it unconditionally
  // subscribed every pane of every mounted Screen to a value that changes on
  // each switch — so switching re-rendered the whole app for nothing.
  const currentId = useRuntimeStore((s) => (sid === null && focused ? s.currentId : null));
  const eid = sid ?? currentId;
  // This pane's OWN draft slot, so several unbound panes each keep an
  // independent draft and each create their own session on first send (#2).
  const draftKey = draftKeyFor(leafId);
  const key = eid ?? draftKey;

  // Per-field selection (never a bare useRuntimeStore()): a background session's
  // SSE folds must not repaint this pane. The active thread is selected on its
  // own below (#34).
  const status = useRuntimeStore((s) => s.status);
  const switching = useRuntimeStore((s) => s.switching);
  const webReadOnly = useRuntimeStore((s) => s.webReadOnly);
  // Session-keyed maps are read PER SESSION, never as the whole map: they churn
  // as any session streams (background panes and invisible subagents included),
  // so subscribing to the map itself repaints this pane on every foreign token.
  // Selecting the scalar lets Zustand's identity check bail out instead (#34).
  const sending = useRuntimeStore((s) => !!s.sendingSessions[key]);
  const running = useRuntimeStore((s) => !!(eid && s.runningSessions[eid]));
  const backgroundReview = useRuntimeStore((s) =>
    eid ? s.backgroundReviews[eid] : undefined,
  );
  // A scalar again (see above): the providers array is replaced wholesale on
  // every catalog refresh, so selecting the resolved number keeps this pane out
  // of those repaints.
  const contextLimit = useRuntimeStore((s) => contextLimitFor(s, key));
  const step = useRuntimeStore((s) => (eid ? (s.stepCounts[eid] ?? 0) : 0));
  const retryNotice = useRuntimeStore((s) => (eid ? s.retryNotices[eid] : undefined));
  const serverUrl = useRuntimeStore((s) => s.serverUrl);
  const sessions = useRuntimeStore((s) => s.sessions);
  const draftWorkspaces = useRuntimeStore((s) => s.draftWorkspaces);
  const error = useRuntimeStore((s) => s.error);
  const questions = useRuntimeStore((s) => s.questions);
  const permissions = useRuntimeStore((s) => s.permissions);
  const sessionParents = useRuntimeStore((s) => s.sessionParents);
  const workspace = useRuntimeStore((s) => s.workspace);
  const pane = useRuntimeStore((s) => s.panes[key]);
  const commands = useRuntimeStore((s) => s.commands);
  const connect = useRuntimeStore((s) => s.connect);
  const sendPrompt = useRuntimeStore((s) => s.sendPrompt);
  const runShell = useRuntimeStore((s) => s.runShell);
  const runCommand = useRuntimeStore((s) => s.runCommand);
  const openArtifact = useRuntimeStore((s) => s.openArtifact);
  const closeArtifact = useRuntimeStore((s) => s.closeArtifact);
  const setShowFiles = useRuntimeStore((s) => s.setShowFiles);
  const setShowRuns = useRuntimeStore((s) => s.setShowRuns);
  const setShowAgents = useRuntimeStore((s) => s.setShowAgents);
  const answerQuestion = useRuntimeStore((s) => s.answerQuestion);
  const rejectQuestion = useRuntimeStore((s) => s.rejectQuestion);
  const replyPermission = useRuntimeStore((s) => s.replyPermission);
  const interrupt = useRuntimeStore((s) => s.interrupt);
  const cancelAutoReview = useRuntimeStore((s) => s.cancelAutoReview);
  const editMessage = useRuntimeStore((s) => s.editMessage);
  const revertMessage = useRuntimeStore((s) => s.revertMessage);
  const setComposerDraft = useUiStore((s) => s.setComposerDraft);
  const approvalMode = useRuntimeStore((s) => s.approvalMode);
  const setApprovalMode = useRuntimeStore((s) => s.setApprovalMode);
  const agents = useRuntimeStore((s) => s.agents);
  const sessionAgents = useRuntimeStore((s) => s.sessionAgents);
  const setAgentMode = useRuntimeStore((s) => s.setAgentMode);
  const bindSession = useLayoutStore((s) => s.bindSession);
  const aimDraft = useRuntimeStore((s) => s.aimDraft);
  const switchWorkspace = useRuntimeStore((s) => s.switchWorkspace);
  const dockSession = useLayoutStore((s) => s.dockSession);
  const setLeafZoom = useLayoutStore((s) => s.setLeafZoom);
  // Any real interaction with a tentative (preview) screen pins it (#3).
  const pinEphemeral = useLayoutStore((s) => s.pinEphemeral);
  const navigate = useNavigate();
  const location = useLocation();
  const requestedMissionId = (
    location.state as { researchMissionId?: ResearchMissionId } | null
  )?.researchMissionId;
  const isMobile = useIsMobile();
  // Split buttons/drag only make sense where tiling works (desktop, not web).
  const canSplit = !isGatewayWeb && !isMobile;

  const connected = status === "ready" || switching;
  const connecting = status === "connecting" && !switching;
  const displayStatus = switching ? "ready" : status;
  const [activeMission, setActiveMission] = useState<MissionRecord | null>(null);
  const [missionCheck, setMissionCheck] = useState<MissionCheck | null>(null);
  const [missionChecking, setMissionChecking] = useState(false);
  const activeMissionId = activeMission?.missionId ?? null;
  const activeMissionIdRef = useRef(activeMissionId);
  activeMissionIdRef.current = activeMissionId;
  const missionCheckingRef = useRef(false);
  const missionTransitioningRef = useRef(false);
  const missionWasRunning = useRef(false);

  // Mission identity is persisted by the Happy Science kernel, not by this
  // component. Resolve it by session when a real, visible pane becomes focused;
  // background panes never change the process-wide workspace just to decorate UI.
  useEffect(() => {
    if (!eid) {
      setActiveMission(null);
      setMissionCheck(null);
      return;
    }
    if (!focused || !visible) return;
    let cancelled = false;
    void listMissions()
      .then((records) => {
        if (cancelled) return;
        const found = records
          .filter((record) => record.sessionId === eid)
          .sort((a, b) => b.updatedAt - a.updatedAt)[0] ?? null;
        setActiveMission((current) => current?.sessionId === eid && !found ? current : found);
      })
      .catch(() => {
        // No mission store is a normal state for ordinary agent conversations.
      });
    return () => {
      cancelled = true;
    };
  }, [eid, focused, visible, workspace]);

  useEffect(() => {
    setMissionCheck((current) =>
      current?.mission.missionId === activeMissionId ? current : null,
    );
  }, [activeMissionId]);

  // A newly-created session (draft's first send) binds onto this leaf; the
  // wrapper then follows it into the URL and opens its folder.
  const bindIfCreated = (created: string | null) => {
    if (created && sid === null) bindSession(leafId, created);
  };
  // Split THIS pane: dock a fresh DRAFT pane on the given edge. No session or
  // folder is created until that pane's first send (#2), and it carries its own
  // independent draft (thread/composer/model).
  // Where a pane split off this one would continue. Null when there is nothing
  // to continue — a pane with no session and no folder of its own — and then
  // the split button has nothing to ask about. Deliberately not the active
  // workspace: that follows whichever session was opened last (#69).
  const splitFolder = inheritedDraftFolder({ leafId, sessionId: sid }, { sessions, draftWorkspaces });
  /** Split, with the destination the user just chose (null = its own dated folder). */
  const onSplit = (edge: "right" | "bottom", folder: string | null) => {
    const created = dockSession(leafId, edge, null);
    if (created && folder) aimDraft(draftKeyFor(created), folder);
  };
  const onSend = async (text: string, attachments?: string[]) => {
    pinEphemeral();
    if (
      activeMission &&
      [
        "waiting-for-input",
        "waiting-for-approval",
        "paused",
        "interrupted",
        "review-ready",
      ].includes(activeMission.status)
    ) {
      const resumed = await transitionMission(
        activeMission.missionId,
        "resume",
        "Researcher continued the mission",
      );
      setActiveMission(resumed);
    }
    const created = await sendPrompt(text, sid ?? undefined, draftKey, attachments);
    bindIfCreated(created);
    return created;
  };
  const onResearchLaunch = async (launch: ResearchLaunch) => {
    try {
      if (launch.kind === "prompt") {
        await onSend(launch.prompt);
        return;
      }
      // Mission records and their deliverables must share one workspace. A
      // plain draft normally materializes its dated folder inside sendPrompt,
      // which would be too late because planning already persists the record.
      if (!isGatewayWeb) {
        const intended = sid
          ? sessions.find((session) => session.id === sid)?.directory
          : draftWorkspaces[draftKey];
        if (intended && intended !== workspace) {
          await switchWorkspace({ path: intended, key: draftKey });
        } else if (!sid && !intended) {
          await switchWorkspace({ dated: datedWorkspaceName(), key: draftKey });
        }
        if (!sid && !useRuntimeStore.getState().draftWorkspaces[draftKey]) {
          throw new Error("The mission workspace could not be created");
        }
      }
      const plan = await planMission(launch.mission, launch.rigor);
      const created = await onSend(missionPromptWithBrief(plan.prompt, launch.brief));
      if (created) {
        const started = await startMission(plan.mission.missionId, created);
        setActiveMission(started);
        setMissionCheck(null);
      }
    } catch (missionError) {
      toast.error(
        t("starters.error.mission", {
          message: missionError instanceof Error ? missionError.message : String(missionError),
        }),
      );
    }
  };
  const runMissionCheck = useCallback(async () => {
    if (!activeMissionId || missionCheckingRef.current) return null;
    const checkedMissionId = activeMissionId;
    missionCheckingRef.current = true;
    setMissionChecking(true);
    try {
      const check = await checkMission(checkedMissionId);
      if (activeMissionIdRef.current === checkedMissionId) {
        setMissionCheck(check);
        setActiveMission(check.mission);
      }
      return check;
    } finally {
      missionCheckingRef.current = false;
      setMissionChecking(false);
    }
  }, [activeMissionId]);

  // A mission panel should open with kernel truth, not a stale persisted count.
  // This also migrates older gate contracts as soon as an idle session is viewed.
  useEffect(() => {
    if (
      !activeMissionId ||
      !focused ||
      !visible ||
      running ||
      missionCheck ||
      missionCheckingRef.current
    )
      return;
    void runMissionCheck().catch(() => {
      // Manual "Check status" remains available with the surfaced error path.
    });
  }, [activeMissionId, focused, missionCheck, runMissionCheck, running, visible]);

  const lastAutoCheckedStep = useRef<{ missionId: string | null; step: number }>({
    missionId: null,
    step: -1,
  });
  useEffect(() => {
    if (!activeMissionId || step < 1) return;
    const previous = lastAutoCheckedStep.current;
    if (previous.missionId === activeMissionId && previous.step === step) return;
    lastAutoCheckedStep.current = { missionId: activeMissionId, step };
    const timer = window.setTimeout(() => {
      void runMissionCheck().catch(() => {
        // A later step or the end-of-turn check retries transient partial writes.
      });
    }, 700);
    return () => window.clearTimeout(timer);
  }, [activeMissionId, runMissionCheck, step]);

  const refreshMissionStatus = () => {
    void runMissionCheck().catch((checkError) => {
      toast.error(
        t("starters.error.mission", {
          message: checkError instanceof Error ? checkError.message : String(checkError),
        }),
      );
    });
  };

  const onEvidenceDecision = useCallback(
    async (
      evidenceId: string,
      verdict: EvidenceVerdict,
      note: string,
    ): Promise<EvidenceReviewCheck> => {
      if (!activeMissionId) throw new Error(t("researchWorkspace.noActiveMission"));
      const reviewedMissionId = activeMissionId;
      const result = await decideEvidence(reviewedMissionId, evidenceId, verdict, note);
      if (activeMissionIdRef.current === reviewedMissionId) {
        setMissionCheck((current) =>
          current?.mission.missionId === reviewedMissionId
            ? {
                ...current,
                evidenceReview: result.review,
                claimPassports: result.claimPassports,
              }
            : current,
        );
      }
      const refreshed = await runMissionCheck();
      return refreshed?.evidenceReview ?? result.review;
    },
    [activeMissionId, runMissionCheck, t],
  );

  const onCreateResearchRelease = useCallback(async (): Promise<ResearchRelease> => {
    if (!activeMissionId) throw new Error(t("researchWorkspace.noActiveMission"));
    return createResearchRelease(activeMissionId);
  }, [activeMissionId, t]);

  const onMissionTransition = useCallback(
    async (action: MissionAction, reason?: string): Promise<MissionRecord> => {
      if (!activeMissionId) throw new Error(t("researchWorkspace.noActiveMission"));
      const record = await transitionMission(activeMissionId, action, reason);
      if (activeMissionIdRef.current === record.missionId) {
        setActiveMission(record);
        if (action === "resume") setMissionCheck(null);
      }
      return record;
    },
    [activeMissionId, t],
  );

  const onApproveProtocol = useCallback(async (): Promise<MissionCheck> => {
    if (!activeMissionId) throw new Error(t("researchWorkspace.noActiveMission"));
    const approvedMissionId = activeMissionId;
    const result = await approveProtocol(approvedMissionId);
    if (activeMissionIdRef.current === approvedMissionId) {
      setActiveMission(result.mission);
      setMissionCheck(result);
    }
    return result;
  }, [activeMissionId, t]);

  const onResearchDecision = useCallback(
    async (decision: NewResearchDecision): Promise<DecisionLogCheck> => {
      if (!activeMissionId) throw new Error(t("researchWorkspace.noActiveMission"));
      const decidedMissionId = activeMissionId;
      const decisionLog = await recordResearchDecision(decidedMissionId, decision);
      if (activeMissionIdRef.current === decidedMissionId) {
        setMissionCheck((current) =>
          current?.mission.missionId === decidedMissionId
            ? { ...current, decisionLog }
            : current,
        );
      }
      const refreshed = await runMissionCheck();
      return refreshed?.decisionLog ?? decisionLog;
    },
    [activeMissionId, runMissionCheck, t],
  );

  const onLiteratureSearch = useCallback(
    async (query: string): Promise<LiteratureSearchResult> => {
      if (!activeMissionId) throw new Error(t("researchWorkspace.noActiveMission"));
      return searchLiterature(activeMissionId, query);
    },
    [activeMissionId, t],
  );

  const onLiteratureCapture = useCallback(
    async (work: LiteratureWork): Promise<LiteratureImportResult> => {
      if (!activeMissionId) throw new Error(t("researchWorkspace.noActiveMission"));
      const capturedMissionId = activeMissionId;
      const result = await captureLiterature(capturedMissionId, work);
      if (activeMissionIdRef.current === capturedMissionId) {
        setMissionCheck((current) =>
          current?.mission.missionId === capturedMissionId
            ? {
                ...current,
                literatureCorpus: result.corpus,
                sourceManifest: result.sourceManifest,
              }
            : current,
        );
      }
      await runMissionCheck();
      return result;
    },
    [activeMissionId, runMissionCheck, t],
  );

  useEffect(() => {
    if (running) {
      missionWasRunning.current = true;
      return;
    }
    if (!missionWasRunning.current || !activeMissionId) return;
    missionWasRunning.current = false;
    void runMissionCheck()
      .then((check) => {
        if (!check) return;
        if (!check.readyForReview) {
          toast.error(
            t("starters.error.incomplete", {
              files: check.issues.join("; "),
            }),
          );
        } else if (check.evidenceLedger) {
          toast.success(
            t("starters.ledger.ready", {
              claims: check.evidenceLedger.claims,
              sources: check.evidenceLedger.sources,
              contested: check.evidenceLedger.contestedClaimIds.length,
              quotes: check.sourceManifest?.quoteMatches ?? 0,
            }),
          );
        }
      })
      .catch((checkError) => {
        toast.error(
          t("starters.error.mission", {
            message: checkError instanceof Error ? checkError.message : String(checkError),
          }),
        );
      });
  }, [activeMissionId, runMissionCheck, running, t]);
  const onRunShell = async (command: string) => {
    pinEphemeral();
    bindIfCreated(await runShell(command, sid ?? undefined, draftKey));
  };
  const onRunCommand = async (name: string, args: string) => {
    pinEphemeral();
    const localClear = name === "new" || name === "clear";
    const created = await runCommand(name, args, sid ?? undefined, draftKey);
    if (localClear) {
      // /new and /clear reset this pane to a fresh draft in the same folder.
      // focus→URL never navigates to bare "/live", so do it here (the draft
      // has no session id for the URL to reflect).
      bindSession(leafId, null);
      if (focused) navigate("/live");
    } else bindIfCreated(created);
  };
  const composerCommands = useMemo(() => {
    const local = [
      { name: "new", description: t("localCommand.newDescription"), source: "local" },
      { name: "clear", description: t("localCommand.clearDescription"), source: "local" },
    ];
    const localNames = new Set(local.map((c) => c.name));
    return [...local, ...commands.filter((c) => !localNames.has(c.name))];
  }, [commands, t]);

  const handlers: BlockHandlers = useMemo(
    () => ({
      onArtifactOpen: (a) => {
        pinEphemeral();
        openArtifact(a, sid ?? undefined);
      },
      onFigureComment: (a, title) =>
        void sendPrompt(
          `On the figure ${title}, at (${a.x.toFixed(0)}%, ${a.y.toFixed(0)}%): ${a.note}`,
          sid ?? undefined,
        ),
      onEditMessage: (id, text) => editMessage(id, text, sid ?? undefined),
      onRevertMessage: async (id, text) => {
        if (await revertMessage(id, sid ?? undefined)) setComposerDraft(text);
      },
    }),
    [openArtifact, sendPrompt, editMessage, revertMessage, setComposerDraft, sid, pinEphemeral],
  );
  const onEvaluate = (expr: string) =>
    void sendPrompt(`Evaluate in the notebook kernel:\n\`\`\`python\n${expr}\n\`\`\``, sid ?? undefined);

  // This session's thread, selected on its own so only its own folds repaint.
  const thread = useRuntimeStore((s) => s.threads[key]);
  const historyLoading = connected && !!eid && !thread?.loaded;
  const title = sessions.find((s) => s.id === eid)?.title;
  const isEmpty = !thread || thread.blocks.length === 0;
  const working = sending || running;
  // Scan backwards in place: copying + reversing the whole block list ran on
  // every render of a live pane, allocating a fresh array per streamed token.
  const currentTool = working ? findLastRunningTool(thread?.blocks) : undefined;
  const lastBlock = thread?.blocks[thread.blocks.length - 1];
  const missionInterrupted =
    !!activeMission &&
    !working &&
    lastBlock?.kind === "status-line" &&
    lastBlock.interrupted === true;
  const liveReasoningIndex =
    running && thread && lastBlock?.kind === "reasoning" ? thread.blocks.length - 1 : undefined;

  // Esc interrupts the running turn — but only in the FOCUSED pane, so a split
  // layout doesn't broadcast one Esc to every running session.
  useEffect(() => {
    if (!running || !focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      if (document.querySelector('[role="dialog"], [role="alertdialog"]')) return;
      void interrupt(sid ?? undefined);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [running, focused, interrupt, sid]);

  // The oldest unanswered request for THIS session (subagent asks resolve
  // through the parent chain to their root session).
  const belongsHere = (s: string) => !!eid && (s === eid || rootSessionOf(sessionParents, s) === eid);
  const activeQuestion = questions.find((q) => belongsHere(q.sessionId));
  const activePermission = permissions.find((p) => belongsHere(p.sessionId));
  const activeRequest = activeQuestion ?? activePermission;
  const requestOrigin =
    activeRequest && activeRequest.sessionId !== eid
      ? (sessions.find((s) => s.id === activeRequest.sessionId)?.title ?? t("live.subagentFallback"))
      : undefined;

  useEffect(() => {
    if (!activeMission || missionTransitioningRef.current) return;
    let action: MissionAction | null = null;
    let reason: string | undefined;
    if (
      missionInterrupted &&
      ["running", "waiting-for-input", "waiting-for-approval", "review-ready"].includes(
        activeMission.status,
      )
    ) {
      action = "interrupt";
      reason = "The agent turn was interrupted before mission completion";
    } else if (activePermission && activeMission.status === "running") {
      action = "wait-for-approval";
      reason = "The agent is waiting for a permission decision";
    } else if (activeQuestion && activeMission.status === "running") {
      action = "wait-for-input";
      reason = "The agent is waiting for researcher input";
    } else if (
      running &&
      !activeRequest &&
      ["waiting-for-input", "waiting-for-approval", "interrupted"].includes(activeMission.status)
    ) {
      action = "resume";
      reason = "The agent resumed execution";
    }
    if (!action) return;

    missionTransitioningRef.current = true;
    void transitionMission(activeMission.missionId, action, reason)
      .then((record) => {
        if (activeMissionIdRef.current === record.missionId) setActiveMission(record);
      })
      .catch(() => {
        // The next kernel refresh reconciles transient gateway/runtime failures.
      })
      .finally(() => {
        missionTransitioningRef.current = false;
      });
  }, [activeMission, activePermission, activeQuestion, activeRequest, missionInterrupted, running]);

  // Derived from the block list, so recompute only when the blocks change — not
  // on every unrelated re-render of a live pane.
  const uniqueNotebooks = useMemo(() => {
    const byPath = new Map<string, Extract<ThreadBlocks[number], { kind: "artifact" }>>();
    for (const b of thread?.blocks ?? [])
      if (b.kind === "artifact" && b.filename.endsWith(".ipynb")) byPath.set(b.path, b);
    return [...byPath.values()];
  }, [thread?.blocks]);

  // An ACP agent is driving instead of the bundled OpenCode runtime (#14).
  const acp = useRuntimeStore((s) => s.runtimeKind) === "acp";
  const acpConfigOptions = useRuntimeStore((s) => s.acpConfigOptions);
  const setAcpConfigOption = useRuntimeStore((s) => s.setAcpConfigOption);
  const planAvailable = agents.some((a) => a.name === "plan");
  const agentMode = sessionAgents[key] ?? "build";
  const activeArtifact = pane?.artifact ?? null;
  const showFiles = !activeArtifact && !!pane?.showFiles;
  const showRuns = !activeArtifact && !showFiles && !!pane?.showRuns;
  const showAgents = !activeArtifact && !showFiles && !showRuns && !!pane?.showAgents;
  const inspectorActive = !!activeArtifact || showFiles || showRuns || showAgents;
  const compactNotebooks = !solo || isMobile;
  // Header tool labels ("Files", "Runs", "Subagents") need real room. `solo`
  // only says this is the single pane, which a narrow window makes irrelevant.
  const headerRef = useRef<HTMLDivElement>(null);
  const headerCompact = useCompactWidth(headerRef, HEADER_LABEL_MIN_PX, laidOut);
  const showToolLabels = solo && !headerCompact;
  const openNotebook = (notebook: (typeof uniqueNotebooks)[number]) => {
    pinEphemeral();
    openArtifact(notebook, sid ?? undefined);
  };
  // A tiled (non-solo) pane is narrow: fill it with the inspector rather than a
  // side column that would squeeze the chat or overflow the pane.
  const inspectorFillsPane = inspectorActive && !solo;
  // The folder shown in the Files toggle: this session's own directory (falling
  // back to the active workspace on a draft that has none yet).
  const sessionDir = sessions.find((s) => s.id === eid)?.directory ?? workspace;

  // Offer the subagent panel only once this conversation has actually spawned
  // one — a plain chat should not carry a control for something it never does.
  const hasSubagents = useMemo(
    () =>
      (thread?.blocks ?? []).some(
        (b) => b.kind === "tool-call" && (b.tool === "task" || !!b.childSessionId),
      ),
    [thread?.blocks],
  );

  const [hasRuns, setHasRuns] = useState(() => (eid ? (RUNS_KNOWN.get(eid) ?? false) : false));
  // A pane can be pointed at another session without unmounting — adopt that
  // session's known answer in this render, not a frame later.
  const runsFor = useRef(eid);
  if (runsFor.current !== eid) {
    runsFor.current = eid;
    setHasRuns(eid ? (RUNS_KNOWN.get(eid) ?? false) : false);
  }
  useEffect(() => {
    // Re-checked when the screen is shown again, not only on mount: a hidden
    // pane's session can produce its first run while nobody is looking, and
    // panes no longer re-mount on a Screen switch to pick that up.
    if (!eid || !visible) return;
    let cancelled = false;
    void queryRuns({ sessionId: eid, limit: 1 }).then((p) => {
      RUNS_KNOWN.set(eid, p.total > 0);
      if (!cancelled) setHasRuns(p.total > 0);
    });
    return () => {
      cancelled = true;
    };
  }, [eid, visible]);

  const chatRef = useRef<HTMLDivElement>(null);
  const [chatViewportH, setChatViewportH] = useState(0);
  const missionControlVisible = isEmpty && !eid && !webReadOnly;

  const {
    contentRef: chatContentRef,
    onScroll: onChatScroll,
    atLatest,
    jumpToLatest,
    // A screen hidden WITHOUT layout reads every offset as 0, so recording one
    // would overwrite where the reader actually was; standing down also means
    // the position is restored on the way back. A screen that keeps its layout
    // keeps its scroll too, and must not be disturbed.
  } = useChatScroll(
    chatRef,
    missionControlVisible ? `mission-control:${key}` : `chat:${key}`,
    laidOut && !historyLoading && !inspectorFillsPane,
    missionControlVisible ? "top" : "bottom",
  );
  // Take back the vertical trackpad gestures WebKit latches onto a wide table
  // or code block inside the conversation.
  useWheelChain(chatRef);

  // Measure the floating composer so the conversation can pad its bottom by
  // exactly that height (in real px, outside the chat zoom) — the last message
  // always clears the composer, whatever the zoom or composer height.
  const composerRef = useRef<HTMLDivElement>(null);
  const [composerH, setComposerH] = useState(80);
  useEffect(() => {
    const el = composerRef.current;
    if (!el) return;
    // Zero means "not laid out" (a hidden screen), never a real height — taking
    // it would strip the conversation's bottom padding and re-add it a frame
    // after the screen is shown again.
    const measure = () => setComposerH((h) => el.offsetHeight || h);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [inspectorFillsPane]);

  useEffect(() => {
    const el = chatRef.current;
    if (!el) return;
    const measure = () => setChatViewportH((height) => el.clientHeight || height);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [inspectorFillsPane]);

  // The empty research screen can occupy the composer's transparent top fade.
  // Use the same reservation for both the scroller and workbench height so the
  // 32px overlap is not counted twice and does not create a phantom scrollbar.
  const composerReservedHeight = missionControlVisible
    ? Math.max(0, composerH - MISSION_COMPOSER_OVERLAP_PX)
    : composerH;
  const chatBottomPadding = composerReservedHeight + COMPOSER_CLEARANCE_PX;

  // Mission Control fills the remaining pane without shrinking its controls.
  // Short or narrow panes keep their natural content height and really scroll.
  const researchWorkbenchAvailableHeight =
    missionControlVisible && chatViewportH > 0
      ? Math.max(
          320,
          (chatViewportH - chatBottomPadding) / Math.max(zoom, 0.1) -
            CHAT_CONTENT_TOP_PX,
        )
      : undefined;

  const autoOpened = useRef(new Set<string>());
  useEffect(() => {
    // Not while hidden: an unbound pane's artifact falls back to the CURRENT
    // session, so a background Screen would open a notebook in the pane the
    // user is actually looking at. Deferred to the moment its Screen is shown.
    if (!visible) return;
    const agentNb = uniqueNotebooks.find(
      (b) => b.tool.toLowerCase().includes("jupyter") && !autoOpened.current.has(b.path),
    );
    if (agentNb) {
      autoOpened.current.add(agentNb.path);
      openArtifact(agentNb, sid ?? undefined);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uniqueNotebooks.length, visible]);

  // Per field, for the same reason as the runtime selectors above: a bare
  // `useUiStore()` subscribes this pane to the whole UI store, so anything that
  // lands there later (a per-keystroke value, say) would repaint every pane.
  const sidebarCollapsed = useUiStore((s) => s.sidebarCollapsed);
  const setSidebarCollapsed = useUiStore((s) => s.setSidebarCollapsed);
  const isMac = navigator.userAgent.includes("Mac");
  const overlayTitlebar = useOverlayTitlebar();
  // Only the primary pane clears the traffic lights / hosts the expand button.
  const asTitlebar = chromeAsTitlebar && overlayTitlebar;
  const showSidebarExpand = chromeAsTitlebar && sidebarCollapsed;

  // The files/artifact/runs inspector content — reused whether it fills a tiled
  // pane or sits in the solo pane's resizable side column.
  const inspectorNode = activeArtifact ? (
    <InspectorShell
      inspector={fileInspectorFromBlock(activeArtifact)}
      workspaceDirectory={sessionDir ?? undefined}
      onClose={() => closeArtifact(sid ?? undefined)}
      onEvaluate={onEvaluate}
      controls={<MaximizePaneButton />}
    />
  ) : showRuns ? (
    <RunsPane sessionId={eid!} onClose={() => setShowRuns(false, sid ?? undefined)} controls={<MaximizePaneButton />} />
  ) : showAgents ? (
    <SubagentPane
      sessionId={eid!}
      onClose={() => setShowAgents(false, sid ?? undefined)}
      controls={<MaximizePaneButton />}
    />
  ) : showFiles ? (
    <SessionFilesPane
      key={`files:${eid}`}
      sessionId={eid!}
      sessionDir={sessionDir ?? undefined}
      onClose={() => setShowFiles(false, sid ?? undefined)}
      controls={<MaximizePaneButton />}
    />
  ) : null;

  return (
    <div className="flex h-full min-w-0">
      {/* `relative` anchors the floating composer (absolute, below). */}
      <div className="relative flex h-full min-w-0 flex-1 flex-col">
        <div
          ref={headerRef}
          data-tauri-drag-region={asTitlebar || undefined}
          style={sidebarCollapsed && asTitlebar ? overlayTitlebarStyle(true) : undefined}
          className={cn(
            // `select-none`: this row is chrome (title, zoom, panel toggles) —
            // dragging across it used to leave stray highlight behind.
            "flex shrink-0 select-none items-center border-faint",
            // Tiled panes get a compact header — h-12 wastes vertical space in
            // a small pane. Solo/web keeps the full-height titlebar row.
            solo ? "gap-2 px-6" : "gap-1 px-2.5",
            eid && "border-b",
            !(sidebarCollapsed && asTitlebar) && (solo ? "h-12" : "h-8"),
          )}
        >
          {showSidebarExpand && (
            <button
              onClick={() => setSidebarCollapsed(false)}
              aria-label={t("live.header.expandSidebarAria")}
              title={t("live.header.expandSidebarTitle", { shortcut: isMac ? "⌘B" : "Ctrl+B" })}
              className="fade-in rounded p-1 text-text hover:bg-surface-2"
            >
              <PanelLeft size={14} strokeWidth={1.5} />
            </button>
          )}
          {eid && (
            // The title doubles as a drag handle to re-dock this pane. Opt it
            // out of the macOS window-drag region so grabbing it moves the pane,
            // not the window.
            <h1
              draggable={false}
              onDragStart={(e) => e.preventDefault()}
              // eslint-disable-next-line i18next/no-literal-string -- DragSource kind, not UI copy
              onPointerDown={(e) => startPaneDrag(e, { kind: "pane", leafId, sessionId: eid }, title ?? "")}
              // `select-none` stops the title text from being selected while
              // dragging (the reason a header drag looked like a text selection).
              className="min-w-0 shrink cursor-grab select-none truncate text-[13px] font-medium text-text active:cursor-grabbing"
            >
              {title ?? ""}
            </h1>
          )}
          {/* Only while on display: the pill polls the plugin's state file every
              few seconds, and its popover lives in a body portal — a hidden
              screen would keep polling and could leave that popover floating
              over the screen the user switched to. Re-mounting is free: the
              pill paints its last known goal from cache. */}
          {eid && visible && (
            <GoalPill
              sessionId={eid}
              compact={headerCompact}
              onResumed={() => void sendPrompt(GOAL_RESUME_NUDGE, sid ?? undefined)}
            />
          )}
          <div data-tauri-drag-region={asTitlebar || undefined} className="flex-1" />
          {eid && (
            <button
              onClick={() => {
                pinEphemeral();
                setShowFiles(!showFiles, sid ?? undefined);
              }}
              className={cn(
                "flex items-center gap-1 rounded-md px-1.5 py-1 text-xs transition-colors hover:bg-surface-2",
                showFiles ? "bg-surface-2 text-text" : "text-muted",
              )}
              title={`${t("live.filesToggle.title")}${sessionDir ? ` — ${sessionDir}` : ""}`}
              aria-pressed={showFiles}
            >
              <FolderOpen size={13} />
              {/* Tiled panes are narrow — show just the icon, not the folder name. */}
              {showToolLabels && (
                <span className="max-w-[160px] truncate">
                  {sessionDir ? baseName(sessionDir) : t("live.filesToggle.default")}
                </span>
              )}
            </button>
          )}
          {eid && hasRuns && (
            <button
              onClick={() => {
                pinEphemeral();
                setShowRuns(!showRuns, sid ?? undefined);
              }}
              className={cn(
                "flex items-center gap-1 rounded-md px-1.5 py-1 text-xs transition-colors hover:bg-surface-2",
                showRuns ? "bg-surface-2 text-text" : "text-muted",
              )}
              title={t("live.runsToggle.title")}
              aria-pressed={showRuns}
            >
              <FlaskConical size={13} />
              {showToolLabels && <span>{t("live.runsToggle.label")}</span>}
            </button>
          )}
          {/* Subagents: only offered once this conversation has spawned one,
              so a plain single-agent chat keeps a clean header. */}
          {eid && hasSubagents && (
            <button
              onClick={() => {
                pinEphemeral();
                setShowAgents(!showAgents, sid ?? undefined);
              }}
              className={cn(
                "flex items-center gap-1 rounded-md px-1.5 py-1 text-xs transition-colors hover:bg-surface-2",
                showAgents ? "bg-surface-2 text-text" : "text-muted",
              )}
              title={t("subagents.toggleTitle")}
              aria-pressed={showAgents}
            >
              <Bot size={13} />
              {showToolLabels && <span>{t("subagents.title")}</span>}
            </button>
          )}
          {/* Split this pane — the visible, discoverable way to tile (no
              keyboard shortcut needed). Right = side-by-side, down = stacked. */}
          {canSplit && (
            <>
              <ZoomMenu zoom={zoom} onPick={(z) => setLeafZoom(leafId, z)} />
              {/* Each split button asks where the new pane's work goes before
                  creating it — see SplitMenu. */}
              <SplitMenu
                sourceFolder={splitFolder}
                // eslint-disable-next-line i18next/no-literal-string -- DockEdge enum, not UI copy
                onSplit={(folder) => onSplit("right", folder)}
                icon={<PanelRight size={13} strokeWidth={1.5} />}
                label={t("group.splitRight")}
              />
              <SplitMenu
                sourceFolder={splitFolder}
                // eslint-disable-next-line i18next/no-literal-string -- DockEdge enum, not UI copy
                onSplit={(folder) => onSplit("bottom", folder)}
                icon={<PanelBottom size={13} strokeWidth={1.5} />}
                label={t("group.splitDown")}
              />
              {onClose && (
                <button
                  onClick={onClose}
                  className="rounded-md p-1 text-muted transition-colors hover:bg-border hover:text-error"
                  title={t("group.closePane")}
                  aria-label={t("group.closePane")}
                >
                  <X size={13} strokeWidth={1.5} />
                </button>
              )}
            </>
          )}
          {/* The green "ready" dot is noise per pane — only surface trouble. */}
          {displayStatus !== "ready" && <ConnBadge status={displayStatus} />}
          {!compactNotebooks && uniqueNotebooks.map((nb) => (
            <button
              key={nb.path}
              onClick={() => openNotebook(nb)}
              className={cn(
                "flex items-center gap-1 rounded-md px-1.5 py-1 font-mono text-xs transition-colors hover:bg-surface-2",
                activeArtifact?.path === nb.path ? "bg-surface-2 text-text" : "text-muted",
              )}
              title={t("live.notebook.openTitle", { path: nb.path })}
            >
              <NotebookPen size={12} />
              <span className="max-w-[180px] truncate">{nb.filename}</span>
            </button>
          ))}
          {compactNotebooks && uniqueNotebooks.length === 1 && (
            <button
              onClick={() => openNotebook(uniqueNotebooks[0])}
              className={cn(
                "rounded-md p-1 text-muted transition-colors hover:bg-surface-2 hover:text-text",
                activeArtifact?.path === uniqueNotebooks[0].path && "bg-surface-2 text-text",
              )}
              title={t("live.notebook.openTitle", { path: uniqueNotebooks[0].path })}
              aria-label={t("live.notebook.openTitle", { path: uniqueNotebooks[0].path })}
            >
              <NotebookPen size={13} />
            </button>
          )}
          {compactNotebooks && uniqueNotebooks.length > 1 && (
            <DropdownMenu.Root>
              <DropdownMenu.Trigger asChild>
                <button
                  className={cn(
                    "rounded-md p-1 text-muted outline-none transition-colors hover:bg-surface-2 hover:text-text",
                    activeArtifact?.path &&
                      uniqueNotebooks.some((notebook) => notebook.path === activeArtifact.path) &&
                      "bg-surface-2 text-text",
                  )}
                  title={t("live.notebook.chooseTitle")}
                  aria-label={t("live.notebook.chooseTitle")}
                >
                  <NotebookPen size={13} />
                </button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content
                  align="end"
                  sideOffset={4}
                  className="z-50 min-w-[220px] max-w-[min(320px,calc(100vw-16px))] rounded-card border border-border bg-surface p-1 text-xs text-text shadow-pop"
                >
                  {uniqueNotebooks.map((notebook) => (
                    <DropdownMenu.Item
                      key={notebook.path}
                      onSelect={() => openNotebook(notebook)}
                      className={cn(
                        "flex cursor-pointer items-center gap-2 rounded-input px-2 py-1.5 font-mono outline-none data-[highlighted]:bg-surface-2",
                        activeArtifact?.path === notebook.path && "bg-surface-2",
                      )}
                      title={notebook.path}
                    >
                      <NotebookPen size={12} className="shrink-0 text-muted" />
                      <span className="truncate">{notebook.filename}</span>
                    </DropdownMenu.Item>
                  ))}
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          )}
          {!connected && (
            <button
              onClick={connect}
              disabled={connecting}
              className="flex items-center gap-1.5 rounded-input bg-accent px-2.5 py-0.5 text-xs font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
            >
              {connecting ? <Loader2 size={13} className="animate-spin" /> : <PlugZap size={13} />}
              {t("live.connect")}
            </button>
          )}
        </div>

        {inspectorFillsPane ? (
          // Tiled pane: the inspector fills the pane (chat/composer hidden), so a
          // narrow pane isn't squeezed and nothing overflows. Its own header's
          // close (and the pressed folder/runs toggle) returns to the chat.
          <div className="min-h-0 flex-1 overflow-hidden">{inspectorNode}</div>
        ) : (
          <>
        <div
          ref={chatRef}
          onScroll={onChatScroll}
          // Ordinary transcripts clear the complete composer. Mission Control
          // reuses its transparent top gutter, matching the height calculation
          // above so a fitting empty screen has no artificial scroll range.
          style={{ paddingBottom: chatBottomPadding }}
          // `overflow-x-hidden` is deliberate: `overflow-y-auto` alone promotes
          // the other axis to `auto`, so one over-wide message (an unbreakable
          // path, a wide card) let the ENTIRE conversation be dragged sideways.
          // Nothing here needs to scroll horizontally as a page — tables, code
          // blocks and tool output each carry their own horizontal scroller.
          className="flex-1 overflow-y-auto overflow-x-hidden"
        >
          {/* Zoom the CHAT content (not the scroll box or the composer). */}
          <div
            ref={chatContentRef}
            // The conversation is document content, so it keeps the WebView's
            // own menu (Copy, Look Up, Translate) — see lib/nativeMenu.
            data-native-menu
            style={zoom !== 1 ? { zoom } : undefined}
            className={cn(
              "mx-auto flex w-full flex-col gap-4 pt-6",
              missionControlVisible
                ? "max-w-none px-2.5 sm:px-4 lg:px-5"
                : activeMission
                  ? "max-w-[980px] px-4 sm:px-6"
                  : "max-w-[760px] px-8",
            )}
          >
            {!connected && !connecting && !missionControlVisible && (
              <div className="rounded-card border border-border bg-surface p-5 shadow-card">
                <div className="text-sm font-medium text-text">{t("live.runtime.title")}</div>
                <p className="mt-1 text-sm text-muted">
                  {t("live.runtime.bodyPrefix")}{" "}
                  {/* eslint-disable-next-line i18next/no-literal-string -- literal shell command, not prose */}
                  <span className="font-mono">opencode serve</span>
                  {t("live.runtime.bodySuffix")}
                </p>
                <div className="mt-3 rounded-input bg-surface-2 px-3 py-2 font-mono text-xs text-text">
                  {serverUrl}
                </div>
              </div>
            )}
            {error && focused && (
              <div className="rounded-input border border-error/30 bg-error/10 px-3 py-2 text-sm text-error">
                {error}
              </div>
            )}
            {missionControlVisible && (
              <Suspense fallback={<LazyPaneFallback />}>
                <ResearchWorkbench
                  initialMissionId={requestedMissionId}
                  disabled={!connected}
                  availableHeight={researchWorkbenchAvailableHeight}
                  onLaunch={(launch) => void onResearchLaunch(launch)}
                />
              </Suspense>
            )}
            {activeMission && (
              <Suspense fallback={<LazyPaneFallback />}>
                <ResearchWorkspaceStatus
                  mission={activeMission}
                  check={missionCheck}
                  checking={missionChecking}
                  onRefresh={refreshMissionStatus}
                  onEvidenceDecision={onEvidenceDecision}
                  onResearchDecision={onResearchDecision}
                  onApproveProtocol={onApproveProtocol}
                  onLiteratureSearch={onLiteratureSearch}
                  onLiteratureCapture={onLiteratureCapture}
                  onCreateRelease={onCreateResearchRelease}
                  onTransition={onMissionTransition}
                />
              </Suspense>
            )}
            {activeMission && (
              <div className="flex items-center gap-3 pt-1 font-mono text-[9px] uppercase tracking-[0.16em] text-muted">
                <span>{t("researchWorkspace.executor")}</span>
                <span className="h-px flex-1 bg-border" aria-hidden />
              </div>
            )}
            {missionInterrupted && activeMission && (
              <div
                role="status"
                className="flex flex-col gap-3 rounded-card border border-warn/30 bg-warn/5 px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2 text-sm font-medium text-text">
                    <RotateCcw size={14} className="shrink-0 text-warn" />
                    <span>{t("researchWorkspace.recovery.title")}</span>
                  </div>
                  <p className="mt-1 text-xs leading-relaxed text-muted">
                    {t("researchWorkspace.recovery.description")}
                  </p>
                </div>
                {!webReadOnly && (
                  <button
                    type="button"
                    disabled={!connected}
                    onClick={() => void onSend(missionResumePrompt(activeMission.missionId))}
                    className="shrink-0 rounded-input bg-accent px-3 py-2 text-xs font-medium text-accent-fg transition-opacity hover:opacity-90 disabled:opacity-40"
                  >
                    {t("researchWorkspace.recovery.resume")}
                  </button>
                )}
              </div>
            )}
            {historyLoading && <ThreadSkeleton />}
            {!historyLoading && thread && (
              <BlockList
                blocks={thread.blocks}
                handlers={handlers}
                liveReasoningIndex={liveReasoningIndex}
                workspaceDirectory={sessionDir ?? undefined}
                contextLimit={contextLimit}
              />
            )}
            {backgroundReview && eid && (
              <div
                role="status"
                className="flex w-fit max-w-full items-center gap-2 rounded-full border border-border bg-surface-2 px-3 py-1.5 text-xs text-muted"
              >
                <Loader2 size={12} className="shrink-0 animate-spin text-accent" />
                <span className="truncate">
                  {backgroundReview === "queued"
                    ? t("live.review.queued")
                    : t("live.review.running")}
                </span>
                <button
                  type="button"
                  className="-mr-1 flex h-5 w-5 shrink-0 items-center justify-center rounded-full hover:bg-surface"
                  aria-label={t("live.review.cancelAria")}
                  title={t("live.review.cancelTitle")}
                  onClick={() => cancelAutoReview(eid)}
                >
                  <X size={11} />
                </button>
              </div>
            )}
            {/* Acts on a text selection anywhere in this pane's answers. */}
            {!webReadOnly && <SelectionActions sessionId={eid} />}
            {working && (
              <div className="flex min-w-0 items-center gap-2 text-sm text-muted">
                <Loader2 size={14} className="shrink-0 animate-spin" />
                <span className="shrink-0">
                  {activeRequest
                    ? t("live.status.paused")
                    : retryNotice
                      ? t("live.status.retrying", { attempt: Math.max(1, retryNotice.attempt) })
                      : sending && !eid
                        ? t("live.status.startingSession")
                        : t("live.status.working")}
                </span>
                {!activeRequest && !retryNotice && step >= 2 && (
                  <span className="shrink-0 text-xs text-muted/70">
                    {t("live.status.step", { count: step })}
                  </span>
                )}
                {!activeRequest && retryNotice && (
                  <span className="truncate font-mono text-xs text-warn" title={retryNotice.message}>
                    {retryNotice.message}
                  </span>
                )}
                {!activeRequest && !retryNotice && currentTool && (
                  <>
                    <span
                      className="truncate font-mono text-xs"
                      title={currentTool.command ?? currentTool.title}
                    >
                      {currentTool.title}
                    </span>
                    {currentTool.startedAt !== undefined && <Elapsed start={currentTool.startedAt} />}
                  </>
                )}
              </div>
            )}
          </div>
        </div>

        {!isEmpty && !atLatest && (
          <div
            className="pointer-events-none absolute inset-x-0 z-20 flex justify-center"
            style={{ bottom: composerH + 18 }}
          >
            <button
              onClick={jumpToLatest}
              className="pointer-events-auto flex items-center gap-1.5 rounded-full border border-border bg-surface/95 px-3 py-1.5 text-xs font-medium text-text shadow-card backdrop-blur-sm transition-colors hover:bg-surface-2"
              aria-label={t("live.latest")}
              title={t("live.latest")}
            >
              <ArrowDown size={13} strokeWidth={1.75} />
              <span>{t("live.latest")}</span>
            </button>
          </div>
        )}

        {/* Floating composer: absolute over the conversation's bottom, with a
            `pointer-events-none` transparent gutter so the conversation stays
            visible and scrolls BEHIND it (only the input box itself is opaque).
            The conversation's `pb-28` keeps the last message clear. Not zoomed —
            the pane zoom shrinks the CHAT text; the input stays a usable size.
            Width is a proportion of the pane (zoom-independent), so it never
            spans edge-to-edge nor gets too narrow. */}
        <div
          ref={composerRef}
          className={cn(
            "pointer-events-none absolute inset-x-0 bottom-0 z-10",
            solo
              ? "px-3 pb-3 pt-7 sm:px-6 sm:pb-4 lg:px-8 lg:pb-5 lg:pt-8"
              : "px-2.5 pb-3 pt-7",
          )}
        >
          {/* Frosted-glass layer BEHIND the input: a light tint + blur that both
              fade out toward the top via a mask, so there's no hard boundary
              line and the strip is most transparent at the top, gradually
              frosting toward the bottom. Kept separate from the input so masking
              never dims the input itself. */}
          <div
            aria-hidden
            className="pointer-events-none absolute inset-0 backdrop-blur-md"
            style={{
              background:
                "linear-gradient(to top, color-mix(in srgb, var(--surface) 68%, transparent), transparent 72%)",
              maskImage: "linear-gradient(to top, #000 28%, transparent)",
              WebkitMaskImage: "linear-gradient(to top, #000 28%, transparent)",
            }}
          />
          {/* Zoomed with the chat so the input scales down in a small/zoomed
              pane; width is a proportion of the pane, centered. `relative` keeps
              it above the frost layer. */}
          <div
            style={zoom !== 1 ? { zoom } : undefined}
            className={cn(
              "pointer-events-auto relative mx-auto space-y-3",
              solo ? "w-full max-w-[760px]" : "w-[94%]",
            )}
          >
            {activeRequest && (
              <InteractionPrompt
                question={activeQuestion}
                permission={activeQuestion ? undefined : activePermission}
                origin={requestOrigin}
                onAnswer={(id, answers) => void answerQuestion(id, answers)}
                onReject={(id) => void rejectQuestion(id)}
                onPermission={(id, reply) => void replyPermission(id, reply)}
              />
            )}
            <Composer
              onSend={onSend}
              onRunShell={(c) => void onRunShell(c)}
              onRunCommand={(n, a) => void onRunCommand(n, a)}
              onInteract={pinEphemeral}
              commands={composerCommands}
              disabled={!connected || working || webReadOnly}
              working={running}
              onStop={() => void interrupt(sid ?? undefined)}
              placeholder={
                webReadOnly
                  ? t("live.placeholder.readOnly")
                  : working
                    ? t("live.placeholder.waiting")
                    : !connected
                      ? t("live.placeholder.disconnected")
                      : planAvailable && agentMode === "plan"
                        ? t("composer.placeholder.plan")
                        : t("composer.placeholder.default")
              }
              // Both switches belong to the OpenCode runtime: the approval mode is
              // its config (an ACP agent asks for permission on its own terms),
              // and the model picker sends a per-turn model ACP v1 has no way to
              // honour — the agent owns its model. Withheld rather than shown
              // doing nothing (#14).
              approvalMode={acp ? undefined : approvalMode}
              onApprovalModeChange={acp ? undefined : (mode) => void setApprovalMode(mode)}
              agentMode={planAvailable ? agentMode : undefined}
              onAgentModeChange={planAvailable ? (mode) => setAgentMode(mode, key) : undefined}
              showModelPicker={connected && !webReadOnly && !acp}
              // The ACP agent's own selectors stand in for the model picker: the
              // agent owns its model list, and `session/set_config_option` is how
              // v1 changes it.
              configOptions={acp && !webReadOnly ? (acpConfigOptions[key] ?? []) : undefined}
              onConfigOption={
                acp && sid
                  ? (configId, value) => void setAcpConfigOption(sid, configId, value)
                  : undefined
              }
              modelSessionId={key}
              // Only the pane the user is looking at may take a prepared draft.
              acceptsHandoff={focused}
              visible={laidOut}
              draftKey={draftKey}
              showWorkspaceChip={eid === null}
              sessionDir={sessionDir ?? undefined}
              currentSessionId={eid}
            />
          </div>
        </div>
          </>
        )}
      </div>

      {/* Solo pane (or web/mobile): the inspector is a resizable side column.
          Tiled panes fill instead (handled above). */}
      {inspectorActive && solo && (
        <RightPane
          onClose={
            activeArtifact
              ? () => closeArtifact(sid ?? undefined)
              : showRuns
                ? () => setShowRuns(false, sid ?? undefined)
                : showAgents
                  ? () => setShowAgents(false, sid ?? undefined)
                  : () => setShowFiles(false, sid ?? undefined)
          }
        >
          {inspectorNode}
        </RightPane>
      )}
    </div>
  );
}

/** Lightweight placeholder while an infrequently used pane is downloaded. */
function LazyPaneFallback() {
  return (
    <div className="flex min-h-24 items-center justify-center text-muted" aria-hidden>
      <Loader2 size={16} className="animate-spin" />
    </div>
  );
}

/** Per-pane zoom control: a compact "NN%" button opening preset levels. Lets a
 *  narrow tiled pane shrink its content so the text isn't oversized. */
const ZOOM_LEVELS = [0.5, 0.75, 1, 1.25, 1.5];
function ZoomMenu({ zoom, onPick }: { zoom: number; onPick: (z: number) => void }) {
  const { t } = useTranslation("session");
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);
  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        className="rounded-md px-1 py-1 text-xs tabular-nums text-muted transition-colors hover:bg-surface-2 hover:text-text"
        title={t("group.zoom")}
        aria-label={t("group.zoom")}
      >
        {Math.round(zoom * 100)}%
      </button>
      {open && (
        <div className="absolute right-0 top-full z-30 mt-1 flex flex-col rounded-md border border-border bg-surface p-1 shadow-card">
          {ZOOM_LEVELS.map((z) => (
            <button
              key={z}
              onClick={() => {
                onPick(z);
                setOpen(false);
              }}
              className={cn(
                "rounded px-3 py-1 text-left text-xs tabular-nums hover:bg-surface-2",
                z === zoom ? "text-text" : "text-muted",
              )}
            >
              {Math.round(z * 100)}%
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** Loading placeholder mirroring the thread's real shapes. */
function ThreadSkeleton() {
  return (
    <div className="animate-pulse space-y-4" aria-hidden>
      <div className="h-11 rounded-card bg-surface-2" />
      <div className="space-y-2.5 px-1 pt-1">
        <div className="h-3.5 w-11/12 rounded bg-surface-2" />
        <div className="h-3.5 w-4/5 rounded bg-surface-2" />
        <div className="h-3.5 w-2/3 rounded bg-surface-2" />
      </div>
      <div className="ml-2 h-4 w-2/5 rounded bg-surface-2 opacity-60" />
      <div className="h-11 rounded-card bg-surface-2" />
      <div className="space-y-2.5 px-1 pt-1">
        <div className="h-3.5 w-5/6 rounded bg-surface-2" />
        <div className="h-3.5 w-3/5 rounded bg-surface-2" />
      </div>
    </div>
  );
}

function ConnBadge({ status }: { status: RuntimeStatus }) {
  const { t } = useTranslation(["session", "common"]);
  const tone = status === "ready" ? "text-ok" : status === "error" ? "text-error" : "text-muted";
  return (
    <span
      className={cn("flex items-center gap-1.5 text-xs", tone)}
      title={t("live.connBadge.title", { status: t(`live.connBadge.status.${status}`) })}
    >
      <span
        className={cn(
          "h-1.5 w-1.5 rounded-full",
          status === "ready" ? "bg-ok" : status === "error" ? "bg-error" : "bg-muted",
          status === "connecting" && "animate-pulse",
        )}
      />
      {status !== "ready" && t("live.connBadge.title", { status: t(`live.connBadge.status.${status}`) })}
    </span>
  );
}
