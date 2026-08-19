/** Owns the research instrument's pane-aware reflow and available-space fill,
 * alongside mission choice, evidence-contract authoring, and launch. */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  BookOpenCheck,
  Check,
  ChevronRight,
  ClipboardCheck,
  Crosshair,
  FileSearch,
  Globe2,
  LineChart,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { PRODUCT_NAME } from "@ai4s/shared";
import type { RigorLevel } from "@/lib/missions";
import {
  RESEARCH_ACTIONS,
  type QuickAction,
  type ResearchBrief,
  type ResearchActionId,
  type ResearchLaunch,
  type ResearchMissionId,
} from "@/lib/researchActions";
import { toast } from "@/lib/toast";

const MISSION_IDS: ResearchMissionId[] = ["plan", "literature", "reproduce", "audit"];
const RIGOR_LEVELS: RigorLevel[] = ["explore", "research", "publication"];
const STUDY_BRIEF_FIELDS: Array<"population" | "intervention" | "primaryOutcome"> = [
  "population",
  "intervention",
  "primaryOutcome",
];
type ResearchWorkbenchLayout = "wide" | "stacked" | "compact" | "small" | "narrow";
type ResearchWorkbenchDensity = "comfortable" | "compact" | "short";

function layoutForWidth(width: number): ResearchWorkbenchLayout {
  if (width <= 460) return "narrow";
  if (width <= 620) return "small";
  if (width <= 760) return "compact";
  if (width <= 920) return "stacked";
  return "wide";
}

function densityForHeight(height: number | undefined): ResearchWorkbenchDensity {
  if (!height) return "comfortable";
  if (height <= 560) return "short";
  if (height <= 760) return "compact";
  return "comfortable";
}

const blankBrief = (): ResearchBrief => ({
  objective: "",
  population: "",
  intervention: "",
  primaryOutcome: "",
  constraints: "",
  scaffoldMissing: true,
});

const ACTION_ICONS: Record<ResearchActionId, React.ReactNode> = {
  plan: <ClipboardCheck size={18} strokeWidth={1.6} />,
  literature: <BookOpenCheck size={18} strokeWidth={1.6} />,
  analyze: <LineChart size={18} strokeWidth={1.6} />,
  reproduce: <RefreshCw size={18} strokeWidth={1.6} />,
  audit: <FileSearch size={18} strokeWidth={1.6} />,
  "example-climate": <Globe2 size={18} strokeWidth={1.6} />,
};

/** Research-first replacement for the old chat starter grid. */
export function ResearchWorkbench({
  onLaunch,
  disabled = false,
  initialMissionId,
  availableHeight,
}: {
  onLaunch: (launch: ResearchLaunch) => void;
  disabled?: boolean;
  initialMissionId?: ResearchMissionId;
  availableHeight?: number;
}) {
  const { t } = useTranslation(["session", "common"]);
  const workbenchRef = useRef<HTMLDivElement>(null);
  const [layout, setLayout] = useState<ResearchWorkbenchLayout>("wide");
  const density = densityForHeight(availableHeight);
  const [selectedId, setSelectedId] = useState<ResearchMissionId>(
    initialMissionId ?? "literature",
  );
  const [rigor, setRigor] = useState<RigorLevel>("research");
  const [briefs, setBriefs] = useState<Record<ResearchMissionId, ResearchBrief>>({
    plan: blankBrief(),
    literature: blankBrief(),
    reproduce: blankBrief(),
    audit: blankBrief(),
  });

  useEffect(() => {
    if (initialMissionId) setSelectedId(initialMissionId);
  }, [initialMissionId]);

  useLayoutEffect(() => {
    const workbench = workbenchRef.current;
    if (!workbench) return;
    const measure = () => {
      const width = workbench.getBoundingClientRect().width;
      if (width > 0) setLayout(layoutForWidth(width));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(workbench);
    return () => observer.disconnect();
  }, []);

  const copy: Record<ResearchActionId, { title: string; description: string }> = {
    plan: { title: t("starters.plan.title"), description: t("starters.plan.description") },
    literature: {
      title: t("starters.literature.title"),
      description: t("starters.literature.description"),
    },
    analyze: { title: t("starters.analyze.title"), description: t("starters.analyze.description") },
    reproduce: {
      title: t("starters.reproduce.title"),
      description: t("starters.reproduce.description"),
    },
    audit: { title: t("starters.audit.title"), description: t("starters.audit.description") },
    "example-climate": {
      title: t("starters.example-climate.title"),
      description: t("starters.example-climate.description"),
    },
  };
  const outputs: Record<ResearchMissionId, string[]> = {
    plan: [
      t("starters.plan.outputs.protocol"),
      t("starters.plan.outputs.decisions"),
      t("starters.plan.outputs.gate"),
    ],
    literature: [
      t("starters.literature.outputs.search"),
      t("starters.literature.outputs.evidence"),
      t("starters.literature.outputs.map"),
      t("starters.literature.outputs.ledger"),
      t("starters.literature.outputs.sources"),
    ],
    reproduce: [
      t("starters.reproduce.outputs.environment"),
      t("starters.reproduce.outputs.comparison"),
      t("starters.reproduce.outputs.verdict"),
      t("starters.reproduce.outputs.ledger"),
      t("starters.reproduce.outputs.sources"),
    ],
    audit: [
      t("starters.audit.outputs.findings"),
      t("starters.audit.outputs.checks"),
      t("starters.audit.outputs.repair"),
      t("starters.audit.outputs.ledger"),
      t("starters.audit.outputs.sources"),
    ],
  };
  const rigorCopy: Record<RigorLevel, { title: string; description: string }> = {
    explore: {
      title: t("starters.rigor.explore.title"),
      description: t("starters.rigor.explore.description"),
    },
    research: {
      title: t("starters.rigor.research.title"),
      description: t("starters.rigor.research.description"),
    },
    publication: {
      title: t("starters.rigor.publication.title"),
      description: t("starters.rigor.publication.description"),
    },
  };
  const missions = MISSION_IDS.map((id) => {
    const action = RESEARCH_ACTIONS.find((candidate) => candidate.id === id);
    if (!action || action.kind !== "mission") throw new Error(`Missing research mission: ${id}`);
    return action;
  });
  const quickActions = RESEARCH_ACTIONS.filter(
    (action): action is QuickAction => action.kind === "quick",
  );
  const selected = missions.find((mission) => mission.id === selectedId) ?? missions[0];
  const brief = briefs[selected.id];
  const briefFields =
    selected.id === "plan"
      ? [brief.objective, brief.population, brief.intervention, brief.primaryOutcome, brief.constraints]
      : [brief.objective, brief.constraints];
  const definedCoordinates = briefFields.filter((value) => value.trim()).length;
  const setBrief = (patch: Partial<ResearchBrief>) =>
    setBriefs((current) => ({
      ...current,
      [selected.id]: { ...current[selected.id], ...patch },
    }));

  const launchQuickAction = async (action: QuickAction) => {
    try {
      await action.prepare?.();
    } catch (error) {
      toast.error(
        t("starters.error.setup", {
          message: error instanceof Error ? error.message : String(error),
        }),
      );
      return;
    }
    onLaunch({ kind: "prompt", prompt: action.prompt });
  };

  return (
    <div
      ref={workbenchRef}
      data-layout={layout}
      data-density={density}
      style={availableHeight ? { minHeight: Math.round(availableHeight) } : undefined}
      className="research-workbench relative w-full overflow-hidden rounded-[14px] border border-border shadow-[0_16px_50px_rgba(17,55,49,0.10)] sm:rounded-[22px]"
    >
      <header className="research-workbench-hero relative overflow-hidden px-5 pb-6 pt-5 text-white sm:px-7 sm:pb-7 sm:pt-6 lg:px-9">
        <div className="research-workbench-hero-layout relative z-[1] grid gap-5 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
          <div>
            <div className="font-mono text-[9px] font-medium uppercase tracking-[0.23em] text-white/65">
              {/* eslint-disable-next-line i18next/no-literal-string -- product brand is locale-independent. */}
              {PRODUCT_NAME} · {t("starters.missionControl")}
            </div>
            <h2 className="research-workbench-title mt-3 max-w-[760px] font-serif text-[clamp(2rem,4.2vw,3.45rem)] leading-[0.96] tracking-[-0.04em] text-white">
              {t("starters.heading")}
            </h2>
            <p className="mt-3 max-w-[650px] text-[12.5px] leading-5 text-white/64">
              {t("starters.subheading")}
            </p>
          </div>
          <div className="evidence-spectrum-legend flex flex-wrap gap-x-4 gap-y-2 font-mono text-[8px] uppercase tracking-[0.13em] text-white/70">
            <span className="flex items-center gap-1.5">
              <span className="h-1.5 w-1.5 rounded-full bg-[#65d4bd]" />
              {t("starters.ledger.supports")}
            </span>
            <span className="flex items-center gap-1.5">
              <span className="h-1.5 w-1.5 rounded-full bg-[#ed7777]" />
              {t("starters.ledger.contradicts")}
            </span>
            <span className="flex items-center gap-1.5">
              <span className="h-1.5 w-1.5 rounded-full bg-[#d8ad5d]" />
              {t("starters.ledger.qualifies")}
            </span>
          </div>
        </div>
        <div className="evidence-spectrum-beam absolute inset-x-0 bottom-0 grid h-[3px] grid-cols-[5fr_2fr_3fr]" aria-hidden>
          <span />
          <span />
          <span />
        </div>
      </header>

      <nav className="bg-surface" aria-label={t("starters.missionControl")}>
        <div className="research-workbench-nav-label flex items-center gap-3 border-b border-border px-4 py-2 sm:px-6 lg:px-8">
          <span className="font-mono text-[8px] uppercase tracking-[0.18em] text-muted">
            {t("starters.newSession")}
          </span>
          <span className="h-px flex-1 bg-border-faint" aria-hidden />
        </div>
        <div className="research-mission-grid grid grid-cols-2 gap-px bg-border lg:grid-cols-4">
          {missions.map((mission) => {
            const active = mission.id === selectedId;
            return (
              <button
                key={mission.id}
                type="button"
                aria-pressed={active}
                onClick={() => setSelectedId(mission.id)}
                className={`research-mission-choice group relative flex min-h-[66px] items-center gap-3 bg-surface px-4 py-3 text-left outline-none transition-[background-color,color] duration-200 focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent motion-reduce:transition-none ${
                  active ? "is-active text-text" : "text-muted hover:bg-surface-2 hover:text-text"
                }`}
              >
                <span
                  className={`grid h-8 w-8 shrink-0 place-items-center rounded-full border transition-colors ${
                    active
                      ? "border-accent bg-accent text-accent-fg"
                      : "border-border bg-surface-2 text-muted group-hover:border-accent/50 group-hover:text-accent"
                  }`}
                >
                  {ACTION_ICONS[mission.id]}
                </span>
                <span className="text-[11.5px] font-medium leading-tight">{copy[mission.id].title}</span>
              </button>
            );
          })}
        </div>
      </nav>

      <div className="research-workbench-body grid bg-surface lg:grid-cols-[minmax(0,1fr)_310px]">
        <section className="research-workbench-primary border-b border-border px-5 py-5 sm:px-7 lg:border-b-0 lg:border-r lg:px-8">
          <div key={selected.id} className="research-contract">
            <div className="research-contract-heading flex items-start justify-between gap-5">
              <div>
                <div className="font-mono text-[8px] uppercase tracking-[0.18em] text-muted">
                  {t("starters.brief.contract")}
                </div>
                <h3 className="mt-1.5 font-serif text-[25px] leading-tight tracking-[-0.02em] text-text">
                  {copy[selected.id].title}
                </h3>
                <p className="mt-1 max-w-[620px] text-[11.5px] leading-[1.55] text-muted">
                  {copy[selected.id].description}
                </p>
              </div>
              <span className="grid h-10 w-10 shrink-0 place-items-center rounded-full border border-accent/25 bg-accent/[0.07] text-accent">
                <ShieldCheck size={20} strokeWidth={1.5} />
              </span>
            </div>

            <div className="research-brief-panel mt-4 overflow-hidden rounded-[14px] border border-border bg-surface-2/45">
              <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
                <div className="flex items-center gap-2 font-mono text-[8px] font-medium uppercase tracking-[0.16em] text-accent">
                  <Crosshair size={12} strokeWidth={1.7} />
                  {t("starters.brief.title")}
                </div>
                <span className="font-mono text-[8px] text-muted">
                  {t("starters.brief.defined", {
                    defined: definedCoordinates,
                    total: briefFields.length,
                  })}
                </span>
              </div>
              <div className="research-brief-fields grid gap-3 p-4">
                <label className="grid gap-1.5">
                  <span className="text-[10px] font-medium text-text">
                    {t(`starters.brief.objective.${selected.id}.label`)}
                    <span className="ml-1 text-error" aria-hidden>*</span>
                  </span>
                  <textarea
                    value={brief.objective}
                    onChange={(event) => setBrief({ objective: event.target.value })}
                    rows={2}
                    placeholder={t(`starters.brief.objective.${selected.id}.placeholder`)}
                    className="min-h-[58px] w-full resize-y rounded-[9px] border border-border bg-surface px-3 py-2 text-[11.5px] leading-5 text-text outline-none transition-colors placeholder:text-muted/65 focus:border-accent focus:ring-2 focus:ring-accent/15"
                  />
                </label>
                {selected.id === "plan" && (
                  <div className="research-study-grid grid gap-3 sm:grid-cols-3">
                    {STUDY_BRIEF_FIELDS.map((field) => (
                      <label key={field} className="grid gap-1.5">
                        <span className="text-[10px] font-medium text-text">
                          {t(`starters.brief.${field}.label`)}
                        </span>
                        <input
                          value={brief[field]}
                          onChange={(event) => setBrief({ [field]: event.target.value })}
                          placeholder={t(`starters.brief.${field}.placeholder`)}
                          className="h-9 min-w-0 rounded-[9px] border border-border bg-surface px-3 text-[10.5px] text-text outline-none transition-colors placeholder:text-muted/65 focus:border-accent focus:ring-2 focus:ring-accent/15"
                        />
                      </label>
                    ))}
                  </div>
                )}
                <label className="grid gap-1.5">
                  <span className="text-[10px] font-medium text-text">
                    {t("starters.brief.constraints.label")}
                    <span className="ml-1 font-normal text-muted">
                      {t("starters.brief.optional")}
                    </span>
                  </span>
                  <input
                    value={brief.constraints}
                    onChange={(event) => setBrief({ constraints: event.target.value })}
                    placeholder={t("starters.brief.constraints.placeholder")}
                    className="h-9 min-w-0 rounded-[9px] border border-border bg-surface px-3 text-[10.5px] text-text outline-none transition-colors placeholder:text-muted/65 focus:border-accent focus:ring-2 focus:ring-accent/15"
                  />
                </label>
                <label className="flex cursor-pointer items-start gap-2.5 text-[10px] leading-4 text-muted">
                  <input
                    type="checkbox"
                    checked={brief.scaffoldMissing}
                    onChange={(event) => setBrief({ scaffoldMissing: event.target.checked })}
                    className="mt-0.5 h-3.5 w-3.5 accent-[var(--accent)]"
                  />
                  <span>{t("starters.brief.scaffold")}</span>
                </label>
              </div>
            </div>

          </div>
        </section>

        <aside className="research-workbench-sidebar bg-surface-2/35 px-5 py-5 sm:px-7 lg:px-6">
          <div className="research-rigor-panel">
            <div className="research-rigor-header flex items-center justify-between gap-3">
            <div className="font-mono text-[8px] uppercase tracking-[0.18em] text-muted">
              {t("starters.rigor.label")}
            </div>
            <div className="rounded-full bg-accent/10 px-2 py-1 font-mono text-[8px] font-medium text-accent">
              {rigorCopy[rigor].title}
            </div>
          </div>
            <div className="research-rigor-options mt-3 grid grid-cols-3 gap-1 rounded-[12px] border border-border bg-surface p-1" role="group" aria-label={t("starters.rigor.label")}>
            {RIGOR_LEVELS.map((level) => (
              <button
                key={level}
                type="button"
                aria-pressed={rigor === level}
                onClick={() => setRigor(level)}
                className={`min-h-9 rounded-[8px] px-2 py-2 text-center text-[9.5px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent motion-reduce:transition-none ${
                  rigor === level
                    ? "bg-accent font-semibold text-accent-fg shadow-sm"
                    : "text-muted hover:bg-surface-2 hover:text-text"
                }`}
              >
                {rigorCopy[level].title}
              </button>
            ))}
          </div>
            <p className="research-rigor-description mt-4 min-h-[60px] text-[10.5px] leading-[1.65] text-muted">
              {rigorCopy[rigor].description}
            </p>
            <button
              type="button"
              disabled={disabled || !brief.objective.trim()}
              onClick={() =>
                onLaunch({ kind: "mission", mission: selected.mission, rigor, brief })
              }
              className="research-launch-button group mt-4 flex min-h-11 w-full items-center justify-between rounded-[11px] border border-accent bg-accent px-4 py-3 text-left text-[11px] font-semibold text-accent-fg shadow-[0_8px_22px_color-mix(in_srgb,var(--accent)_22%,transparent)] outline-none transition-[transform,opacity] hover:-translate-y-0.5 hover:opacity-95 focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface disabled:translate-y-0 disabled:cursor-not-allowed disabled:opacity-40 motion-reduce:transition-none"
            >
              {t("starters.launch")}
              <ChevronRight size={15} className="transition-transform group-hover:translate-x-0.5 motion-reduce:transition-none" />
            </button>
            {!brief.objective.trim() && (
              <p className="research-launch-hint mt-2 text-[9.5px] leading-4 text-muted">
                {t("starters.brief.required")}
              </p>
            )}
          </div>

          <div className="research-output-panel mt-5 border-t border-border pt-4">
            <div className="research-deliverables-label font-mono text-[8px] uppercase tracking-[0.18em] text-muted">
              {t("starters.deliverables")}
            </div>
            <ol className="research-deliverables evidence-track mt-3">
              {outputs[selected.id].map((output, index) => (
                <li key={output} className="relative grid grid-cols-[20px_minmax(0,1fr)] gap-3 pb-1.5 last:pb-0">
                  <span className="relative z-[1] grid h-5 w-5 place-items-center rounded-full border border-accent/60 bg-surface text-accent shadow-[0_0_0_3px_var(--surface)]">
                    <Check size={10} strokeWidth={2.2} />
                  </span>
                  <div className="border-b border-border-faint pb-1.5 text-[10.5px] leading-4 text-text/85">
                    <span className="mr-2 font-mono text-[8px] text-muted/70">
                      {String(index + 1).padStart(2, "0")}
                    </span>
                    {output}
                  </div>
                </li>
              ))}
            </ol>

            <div className="research-ledger-note mt-3 grid gap-2 border border-border bg-surface-2/55 px-3 py-2.5">
              <div className="font-mono text-[8px] font-medium uppercase tracking-[0.15em] text-accent">
                {t("starters.ledger.label")}
              </div>
              <p className="text-[9.5px] leading-[1.5] text-muted">
                {t("starters.ledger.description")}
              </p>
            </div>
          </div>
        </aside>
      </div>

      <footer className="research-workbench-footer grid gap-px border-t border-border bg-border sm:grid-cols-[135px_1fr_1fr] sm:items-stretch">
        <div className="bg-surface px-5 py-4 font-mono text-[8px] uppercase tracking-[0.18em] text-muted sm:px-6">
          {t("starters.quickTools")}
        </div>
        {quickActions.map((action) => (
          <button
            key={action.id}
            type="button"
            disabled={disabled}
            onClick={() => void launchQuickAction(action)}
            className="group flex min-w-0 items-center gap-3 bg-surface px-5 py-3.5 text-left outline-none transition-colors hover:bg-surface-2 focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-45 motion-reduce:transition-none"
          >
            <span className="grid h-8 w-8 shrink-0 place-items-center rounded-full border border-border bg-surface-2 text-accent transition-colors group-hover:border-accent/45">
              {ACTION_ICONS[action.id]}
            </span>
            <span className="min-w-0 flex-1">
              <span className="block text-[11px] font-medium text-text">{copy[action.id].title}</span>
              <span className="mt-0.5 block truncate text-[9.5px] text-muted">
                {copy[action.id].description}
              </span>
            </span>
            <ChevronRight size={13} className="shrink-0 text-muted transition-transform group-hover:translate-x-0.5 motion-reduce:transition-none" />
          </button>
        ))}
      </footer>
    </div>
  );
}
