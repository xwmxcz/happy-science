/** Verifies the research-first workspace emits typed mission and quick-action launches. */
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { missionPromptWithBrief, RESEARCH_ACTIONS } from "@/lib/researchActions";
import { ResearchWorkbench } from "./ResearchWorkbench";

const installCalls: string[] = [];
let failInstall = false;
vi.mock("@/lib/tauri", () => ({
  isTauri: true,
  installExample: async (name: string) => {
    installCalls.push(name);
    if (failInstall) throw new Error("resource missing");
    return name;
  },
}));

describe("ResearchWorkbench", () => {
  beforeEach(() => {
    installCalls.length = 0;
    failInstall = false;
  });

  it("compiles the authored coordinates and explicit TBD policy into the mission prompt", () => {
    const prompt = missionPromptWithBrief("base", {
      objective: "Does X improve Y?",
      population: "Adults with Z",
      intervention: "X versus placebo",
      primaryOutcome: "Y at week 12",
      constraints: "No outcome inspection before approval",
      scaffoldMissing: true,
    });
    expect(prompt).toContain("Objective / research question: Does X improve Y?");
    expect(prompt).toContain("Population / sample: Adults with Z");
    expect(prompt).toContain("Draft a useful first version now");
  });

  it("renders the research missions as an evidence workbench", () => {
    render(<ResearchWorkbench onLaunch={() => {}} />);

    expect(screen.getByText("Happy Science · Mission Control")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Study launch/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Evidence sprint/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: /Reproduction challenge/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Manuscript stress test/ })).toBeInTheDocument();
    expect(screen.getByText("Claim–Evidence Graph")).toBeInTheDocument();
    expect(screen.getByText("SHA-256 source snapshots and retrieval manifest")).toBeInTheDocument();
    expect(RESEARCH_ACTIONS.filter((action) => action.kind === "mission")).toHaveLength(4);
    expect(RESEARCH_ACTIONS.filter((action) => action.kind === "quick")).toHaveLength(2);
  });

  it("opens on the mission selected by an external entry point", () => {
    render(<ResearchWorkbench initialMissionId="plan" onLaunch={() => {}} />);

    expect(screen.getByRole("button", { name: /Study launch/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByText("Research question / hypothesis")).toBeInTheDocument();
    expect(screen.queryByText("Review question / topic")).not.toBeInTheDocument();
  });

  it("derives responsive layout from its own pane width", () => {
    const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      width: 520,
      height: 700,
      top: 0,
      right: 520,
      bottom: 700,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
    try {
      const { container } = render(<ResearchWorkbench onLaunch={() => {}} />);
      expect(container.firstElementChild).toHaveAttribute("data-layout", "small");
    } finally {
      rect.mockRestore();
    }
  });

  it("fills a tall pane without scaling the instrument", () => {
    const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      width: 1200,
      height: 800,
      top: 0,
      right: 1200,
      bottom: 800,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
    try {
      const { container, rerender } = render(
        <ResearchWorkbench availableHeight={800} onLaunch={() => {}} />,
      );
      const instrument = container.firstElementChild as HTMLElement;
      expect(instrument.style.minHeight).toBe("800px");
      expect(instrument.style.transform).toBe("");
      expect(instrument.style.width).toBe("");
      expect(instrument).toHaveAttribute("data-density", "comfortable");

      rerender(<ResearchWorkbench availableHeight={700} onLaunch={() => {}} />);
      expect(instrument).toHaveAttribute("data-density", "compact");

      rerender(<ResearchWorkbench availableHeight={500} onLaunch={() => {}} />);
      expect(instrument).toHaveAttribute("data-density", "short");
    } finally {
      rect.mockRestore();
    }
  });

  it("switches the visible evidence contract with the mission", async () => {
    render(<ResearchWorkbench onLaunch={() => {}} />);

    expect(screen.getByText("Exact search queries and retrieval dates")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Study launch/ }));
    expect(screen.getByText("Preregistration-style research protocol")).toBeInTheDocument();
    expect(screen.queryByText("Exact search queries and retrieval dates")).not.toBeInTheDocument();
  });

  it("launches the selected mission through the typed kernel contract", async () => {
    const onLaunch = vi.fn();
    render(<ResearchWorkbench onLaunch={onLaunch} />);

    await userEvent.click(screen.getByRole("button", { name: /Study launch/ }));
    await userEvent.type(
      screen.getByRole("textbox", { name: /Research question \/ hypothesis/ }),
      "Does treatment X improve outcome Y?",
    );
    await userEvent.click(screen.getByRole("button", { name: "Launch research mission" }));

    expect(onLaunch).toHaveBeenCalledWith({
      kind: "mission",
      mission: "study-launch",
      rigor: "research",
      brief: {
        objective: "Does treatment X improve outcome Y?",
        population: "",
        intervention: "",
        primaryOutcome: "",
        constraints: "",
        scaffoldMissing: true,
      },
    });
  });

  it("passes publication-grade to the mission kernel", async () => {
    const onLaunch = vi.fn();
    render(<ResearchWorkbench onLaunch={onLaunch} />);

    const launch = screen.getByRole("button", { name: "Launch research mission" });
    expect(launch).toBeDisabled();
    await userEvent.type(
      screen.getByRole("textbox", { name: /Review question \/ topic/ }),
      "How robust is the evidence for intervention X?",
    );
    await userEvent.click(screen.getByRole("button", { name: "Publication-grade" }));
    await userEvent.click(launch);

    expect(onLaunch).toHaveBeenCalledWith({
      kind: "mission",
      mission: "evidence-sprint",
      rigor: "publication",
      brief: expect.objectContaining({
        objective: "How robust is the evidence for intervention X?",
      }),
    });
  });

  it("prepares the climate benchmark before launching it", async () => {
    const onLaunch = vi.fn();
    render(<ResearchWorkbench onLaunch={onLaunch} />);

    await userEvent.click(screen.getByRole("button", { name: /Run the NASA climate benchmark/ }));
    await waitFor(() => expect(onLaunch).toHaveBeenCalledTimes(1));
    expect(installCalls).toEqual(["climate-trends"]);
    expect(onLaunch.mock.calls[0][0]).toEqual({
      kind: "prompt",
      prompt: expect.stringContaining("gistemp_global_means.csv"),
    });
  });

  it("does not launch when example preparation fails", async () => {
    failInstall = true;
    const onLaunch = vi.fn();
    render(<ResearchWorkbench onLaunch={onLaunch} />);

    await userEvent.click(screen.getByRole("button", { name: /Run the NASA climate benchmark/ }));
    await waitFor(() => expect(installCalls).toHaveLength(1));
    expect(onLaunch).not.toHaveBeenCalled();
  });
});
