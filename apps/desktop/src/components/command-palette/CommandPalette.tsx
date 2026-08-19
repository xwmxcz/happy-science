import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Command } from "cmdk";
import { useNavigate } from "react-router-dom";
import {
  BookOpenCheck,
  ClipboardCheck,
  FileSearch,
  Moon,
  NotebookPen,
  PackagePlus,
  Plus,
  Settings,
  ShieldCheck,
  RefreshCw,
} from "lucide-react";
import { useUiStore } from "@/lib/store";
import { useRuntimeStore } from "@/lib/runtime";
import { useLayoutStore } from "@/lib/layout";
import { useIsMobile } from "@/lib/useIsMobile";
import { isGatewayWeb } from "@/lib/webMode";
import { toast } from "@/lib/toast";
import {
  RESEARCH_ACTIONS,
  researchLaunchFor,
  type ResearchActionId,
} from "@/lib/researchActions";

interface Action {
  id: string;
  label: string;
  icon: React.ReactNode;
  run: () => void;
}

export function CommandPalette() {
  const { t } = useTranslation("nav");
  const open = useUiStore((s) => s.paletteOpen);
  const setOpen = useUiStore((s) => s.setPaletteOpen);
  const toggleTheme = useUiStore((s) => s.toggleTheme);
  const navigate = useNavigate();
  // Tiling is desktop-only: web/phone show one pane, so there is no Screen to open.
  const newPaneAllowed = !useIsMobile() && !isGatewayWeb;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen(!useUiStore.getState().paletteOpen);
      }
      // Consume Esc only when the palette is open — a marked-handled Esc must
      // not also interrupt a running agent turn (LiveSessionPage listens too).
      if (e.key === "Escape" && useUiStore.getState().paletteOpen) {
        e.preventDefault();
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setOpen]);

  const close = () => setOpen(false);

  // Mission actions open a fresh ResearchWorkbench so their required brief can
  // be authored; quick prompts still create and reveal a session immediately.
  const runResearchAction = async (actionId: ResearchActionId) => {
    close();
    try {
      const action = RESEARCH_ACTIONS.find((candidate) => candidate.id === actionId);
      if (action?.kind === "mission") {
        const layout = useLayoutStore.getState();
        if (newPaneAllowed) layout.openInNewGroup(null);
        useRuntimeStore.getState().startDraft();
        navigate("/live", { state: { researchMissionId: action.id } });
        return;
      }
      const launch = researchLaunchFor(actionId);
      if (launch.kind !== "prompt") throw new Error("Mission brief is required");
      const layout = useLayoutStore.getState();
      const leafId = newPaneAllowed ? layout.openInNewGroup(null) : null;
      const runtime = useRuntimeStore.getState();
      runtime.startDraft();
      const id = await runtime.sendPrompt(launch.prompt);
      if (id && leafId) layout.bindSession(leafId, id);
      if (id) navigate(`/live/${id}`);
    } catch (actionError) {
      toast.error(actionError instanceof Error ? actionError.message : String(actionError));
    }
  };

  const actions: Action[] = [
    { id: "new", label: t("commandPalette.actions.newSession"), icon: <Plus size={16} />, run: () => { if (newPaneAllowed) useLayoutStore.getState().openInNewGroup(null); useRuntimeStore.getState().startDraft(); navigate("/live"); close(); } },
    { id: "plan", label: t("commandPalette.actions.planStudy"), icon: <ClipboardCheck size={16} />, run: () => void runResearchAction("plan") },
    { id: "literature", label: t("commandPalette.actions.reviewLiterature"), icon: <BookOpenCheck size={16} />, run: () => void runResearchAction("literature") },
    { id: "analyze", label: t("commandPalette.actions.analyzeData"), icon: <FileSearch size={16} />, run: () => void runResearchAction("analyze") },
    { id: "reproduce", label: t("commandPalette.actions.reproduceResult"), icon: <RefreshCw size={16} />, run: () => void runResearchAction("reproduce") },
    { id: "review", label: t("commandPalette.actions.auditReport"), icon: <ShieldCheck size={16} />, run: () => void runResearchAction("audit") },
    { id: "notebooks", label: t("commandPalette.actions.openNotebooks"), icon: <NotebookPen size={16} />, run: () => { navigate("/notebooks"); close(); } },
    { id: "skills", label: t("commandPalette.actions.manageSkills"), icon: <PackagePlus size={16} />, run: () => { navigate("/skills"); close(); } },
    { id: "settings", label: t("commandPalette.actions.openSettings"), icon: <Settings size={16} />, run: () => { navigate("/settings"); close(); } },
    { id: "theme", label: t("commandPalette.actions.toggleTheme"), icon: <Moon size={16} />, run: () => { toggleTheme(); close(); } },
  ];

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/20 pt-[16vh]"
      onClick={close}
    >
      <div onClick={(e) => e.stopPropagation()} className="w-full max-w-lg">
        <Command
          label={t("commandPalette.ariaLabel")}
          className="overflow-hidden rounded-card border border-border bg-surface shadow-pop"
        >
          <Command.Input
            autoFocus
            placeholder={t("commandPalette.placeholder")}
            className="w-full border-b border-border bg-transparent px-4 py-3 text-sm text-text outline-none placeholder:text-muted"
          />
          <Command.List className="max-h-80 overflow-y-auto p-2">
            <Command.Empty className="px-3 py-6 text-center text-sm text-muted">
              {t("commandPalette.noResults")}
            </Command.Empty>
            {actions.map((a) => (
              <Command.Item
                key={a.id}
                value={a.label}
                onSelect={a.run}
                className="flex cursor-pointer items-center gap-3 rounded-input px-3 py-2 text-sm text-text data-[selected=true]:bg-surface-2"
              >
                <span className="text-muted">{a.icon}</span>
                {a.label}
              </Command.Item>
            ))}
          </Command.List>
        </Command>
      </div>
    </div>
  );
}
