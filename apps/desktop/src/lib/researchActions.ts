/** Owns research action identities and launch contracts independently of any executor UI. */
import type { MissionKind, RigorLevel } from "./missions";
import { installExample, isTauri } from "./tauri";

export type ResearchMissionId = "plan" | "literature" | "reproduce" | "audit";
export type ResearchActionId = ResearchMissionId | "analyze" | "example-climate";

export interface MissionAction {
  id: ResearchMissionId;
  kind: "mission";
  mission: MissionKind;
}

export interface QuickAction {
  id: Exclude<ResearchActionId, ResearchMissionId>;
  kind: "quick";
  prompt: string;
  prepare?: () => Promise<void>;
}

export type ResearchAction = MissionAction | QuickAction;
export interface ResearchBrief {
  objective: string;
  population: string;
  intervention: string;
  primaryOutcome: string;
  constraints: string;
  scaffoldMissing: boolean;
}
export type ResearchLaunch =
  | { kind: "mission"; mission: MissionKind; rigor: RigorLevel; brief: ResearchBrief }
  | { kind: "prompt"; prompt: string };

/** The single catalogue for actions exposed by every research entry point. */
export const RESEARCH_ACTIONS: ResearchAction[] = [
  { id: "plan", kind: "mission", mission: "study-launch" },
  { id: "literature", kind: "mission", mission: "evidence-sprint" },
  {
    id: "analyze",
    kind: "quick",
    prompt:
      "Analyze the data file I added to the workspace end to end: explore it, run the analysis in code, " +
      "save at least one figure as a PNG, and write report.md with the findings — every number traced to " +
      "the code that produced it. Ask me which file to use if there is more than one candidate.",
  },
  { id: "reproduce", kind: "mission", mission: "reproduction-challenge" },
  { id: "audit", kind: "mission", mission: "manuscript-stress-test" },
  {
    id: "example-climate",
    kind: "quick",
    prompt:
      "Analyze the real climate dataset at climate-trends/data/gistemp_global_means.csv " +
      "(NASA GISTEMP v4 global land–ocean temperature anomalies in °C vs the 1951–1980 mean; " +
      "the header is on line 2 and missing values are `***` — see climate-trends/README.md). " +
      "Load the annual J-D series, quantify the warming rate (°C/decade) over the full record and " +
      "over 1975–present, compare decadal means, save one publication-quality figure as " +
      "climate-trends/warming_trend.png, and write climate-trends/report.md citing the dataset " +
      "source — every number must come from the code you ran.",
    prepare: async () => {
      if (isTauri) await installExample("climate-trends");
    },
  },
];

export function researchLaunchFor(
  id: ResearchActionId,
  rigor: RigorLevel = "research",
  brief?: ResearchBrief,
): ResearchLaunch {
  const action = RESEARCH_ACTIONS.find((candidate) => candidate.id === id);
  if (!action) throw new Error(`Unknown research action: ${id}`);
  if (action.kind === "mission") {
    if (!brief?.objective.trim()) throw new Error("A research brief is required for this mission");
    return { kind: "mission", mission: action.mission, rigor, brief };
  }
  return { kind: "prompt", prompt: action.prompt };
}

/** The single recovery instruction used when a mission turn was cut off by a runtime restart. */
export function missionResumePrompt(missionId: string): string {
  return (
    `Continue Happy Science mission \`${missionId}\` from the interrupted step. ` +
    "Re-read the current workspace state, preserve completed work, avoid repeating finished steps, " +
    "and stop only when you need a research decision or approval from me."
  );
}

/** Compile researcher-authored coordinates into every mission prompt. The brief
 * is required at the product boundary so the executor never starts by asking
 * what job it was launched to do. */
export function missionPromptWithBrief(basePrompt: string, brief: ResearchBrief): string {
  const field = (value: string) => value.trim() || "[TBD — propose options before proceeding]";
  return (
    `${basePrompt}\n\nResearch Brief — researcher-authored and authoritative:\n` +
    `- Objective / research question: ${field(brief.objective)}\n` +
    `- Population / sample: ${field(brief.population)}\n` +
    `- Intervention / exposure / comparison: ${field(brief.intervention)}\n` +
    `- Primary outcome: ${field(brief.primaryOutcome)}\n` +
    `- Practical constraints and context: ${field(brief.constraints)}\n\n` +
    (brief.scaffoldMissing
      ? "Draft a useful first version now. Keep every missing field as an explicit [TBD], propose concrete options, and ask me to resolve only the choices that block approval."
      : "Ask me to resolve every missing field before drafting the deliverables.")
  );
}
