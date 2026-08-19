import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunRecord } from "@ai4s/shared";
import type { RunPage, RunQuery } from "@/lib/runs";
import { useRuntimeStore } from "@/lib/runtime";
import { RunsPage } from "./RunsPage";

const queryRuns = vi.fn();
const readRunLog = vi.fn();
const prepareReproduction = vi.fn();
vi.mock("@/lib/runs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/runs")>()),
  queryRuns: (q: RunQuery) => queryRuns(q),
  readRunLog: (hash: string) => readRunLog(hash),
  prepareReproduction: (runId: string) => prepareReproduction(runId),
}));

const openArtifactExternally = vi.fn();
vi.mock("@/lib/artifactFile", () => ({
  openArtifactExternally: (path: string, root?: string) => openArtifactExternally(path, root),
}));

const run: RunRecord = {
  runId: "run_ab12cd34",
  ts: 1751500000,
  sessionId: "ses_1",
  command: "python train.py --lr 3e-4",
  status: "ok",
  wallMs: 8000,
  logHash: "cafe1234",
  code: [{ path: "train.py", hash: "aaaa", size: 512 }],
  outputs: [{ path: "output/metrics.json", hash: "bbbb", size: 64 }],
  env: {
    python: "3.11.4",
    platform: "linux-x86_64",
    app: "0.1.6",
    packages: { count: 51, hash: "deadbeef" },
    hardware: { cpu: "AMD EPYC 7742", cores: 64, memGb: 512, gpu: ["NVIDIA A100-SXM4-40GB"], accelerator: "cuda" },
  },
};

/** A stand-in index: filters a fixed dataset by the query, like the real backend. */
function serve(dataset: RunRecord[]) {
  queryRuns.mockImplementation((q: RunQuery): Promise<RunPage> => {
    let rows = dataset;
    if (q.status) rows = rows.filter((r) => r.status === q.status);
    if (q.surface) rows = rows.filter((r) => (r.surface ?? "local") === q.surface);
    if (q.search) {
      const s = q.search.toLowerCase();
      rows = rows.filter(
        (r) => r.command.toLowerCase().includes(s) || (r.outputs ?? []).some((o) => o.path.toLowerCase().includes(s)),
      );
    }
    const facets = {
      status: [
        { value: "ok", count: dataset.filter((r) => r.status === "ok").length },
        { value: "failed", count: dataset.filter((r) => r.status === "failed").length },
      ].filter((f) => f.count > 0),
      surface: [] as { value: string; count: number }[],
    };
    return Promise.resolve({ rows, total: rows.length, facets });
  });
}

const renderPage = (entry = "/runs") =>
  render(
    <MemoryRouter initialEntries={[entry]}>
      <RunsPage />
    </MemoryRouter>,
  );

describe("RunsPage", () => {
  const runShell = vi.fn();

  beforeEach(() => {
    queryRuns.mockReset();
    readRunLog.mockReset();
    openArtifactExternally.mockReset();
    prepareReproduction.mockReset();
    runShell.mockReset();
    runShell.mockResolvedValue("ses_1");
    useRuntimeStore.setState({ status: "ready", runShell });
    serve([run]);
  });

  it("lists runs with their command and expands the newest to show the recipe", async () => {
    renderPage();
    expect(await screen.findByText("python train.py --lr 3e-4")).toBeInTheDocument();
    expect(screen.getByText(/NVIDIA A100-SXM4-40GB/)).toBeInTheDocument();
    expect(screen.getByText("output/metrics.json")).toBeInTheDocument();
    expect(screen.getByText(/3.11.4/)).toBeInTheDocument();
  });

  it("surfaces automatic plan deviations on the run", async () => {
    serve([
      {
        ...run,
        integrity: {
          schemaVersion: 1,
          status: "attention",
          planPaths: ["research/protocol.md"],
          findings: [
            {
              kind: "unregistered-predictor",
              level: "warn",
              tag: "stats · prereg",
              title: "Predictor not in the research plan",
              evidence: "analysis.py:2  y ~ x + gender",
              path: "analysis.py",
              line: 2,
            },
          ],
        },
      },
    ]);
    renderPage();
    expect(await screen.findByText("1 warning")).toBeInTheDocument();
    expect(screen.getByText("Predictor not in the research plan")).toBeInTheDocument();
    expect(screen.getByText(/research\/protocol\.md/)).toBeInTheDocument();
  });

  it("shows the persisted reproduction comparison by dimension", async () => {
    const comparison = {
      matched: [],
      changed: ["train.py"],
      missing: [],
      added: [],
      unverifiable: [],
    };
    serve([
      {
        ...run,
        reproduction: {
          requestId: "hsr_1",
          baselineRunId: "run_baseline",
          outcome: "different",
          inputs: { ...comparison, changed: [], matched: ["data.csv"] },
          code: comparison,
          environment: { matches: false, changes: ["packages"] },
          outputs: { ...comparison, changed: ["output/metrics.json"] },
        },
      },
    ]);
    renderPage();
    expect(await screen.findByText("Differences found")).toBeInTheDocument();
    expect(screen.getByText(/baseline run_baseline/)).toBeInTheDocument();
    expect(screen.getByText("packages")).toBeInTheDocument();
  });

  it("prepares and runs an exact one-click reproduction", async () => {
    prepareReproduction.mockResolvedValue({
      requestId: "hsr_1",
      baselineRunId: run.runId,
      command: run.command,
      sessionId: run.sessionId,
      preflight: {},
    });
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: /Reproduce/ }));
    await waitFor(() => expect(runShell).toHaveBeenCalledWith(run.command, run.sessionId));
    expect(prepareReproduction).toHaveBeenCalledWith(run.runId);
  });

  it("loads the captured log on demand", async () => {
    readRunLog.mockResolvedValue("epoch 1\naccuracy 0.93\n");
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: /Log/ }));
    expect(readRunLog).toHaveBeenCalledWith("cafe1234");
    expect(await screen.findByText(/accuracy 0.93/)).toBeInTheDocument();
  });

  it("filters by search over command and output paths", async () => {
    serve([
      { ...run, runId: "run_train", command: "python train.py" },
      { ...run, runId: "run_eval", command: "python evaluate.py", outputs: [] },
    ]);
    renderPage();
    await screen.findByText("python train.py");
    await userEvent.type(screen.getByPlaceholderText(/search/i), "evaluate");
    // Wait for the debounced query to drop the non-matching run.
    await waitFor(() => expect(screen.queryByText("python train.py")).not.toBeInTheDocument());
    expect(screen.getByText("python evaluate.py")).toBeInTheDocument();
  });

  it("filters by status via a facet chip", async () => {
    serve([
      { ...run, runId: "run_ok", command: "python ok.py", status: "ok" },
      { ...run, runId: "run_bad", command: "python bad.py", status: "failed" },
    ]);
    renderPage();
    await screen.findByText("python ok.py");
    await userEvent.click(screen.getByRole("button", { name: /Failed/ }));
    await waitFor(() => expect(screen.queryByText("python ok.py")).not.toBeInTheDocument());
    expect(screen.getByText("python bad.py")).toBeInTheDocument();
  });

  it("opens an output file in the OS when clicked", async () => {
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: /output\/metrics\.json/ }));
    expect(openArtifactExternally).toHaveBeenCalledWith("output/metrics.json", "workspace");
  });

  it("expands the run named in the ?run= query param (deep link)", async () => {
    const older: RunRecord = {
      ...run,
      runId: "run_older",
      command: "python prep.py",
      code: [{ path: "prep.py", hash: "eeee", size: 30 }],
      outputs: [],
      logHash: undefined,
    };
    serve([run, older]);
    renderPage("/runs?run=run_older");
    expect(await screen.findByText("prep.py")).toBeInTheDocument();
    expect(screen.queryByText("output/metrics.json")).not.toBeInTheDocument();
  });

  it("renders a run whose code/outputs arrays were omitted by the store", async () => {
    serve([{ ...run, code: undefined, outputs: undefined, logHash: undefined } as unknown as RunRecord]);
    renderPage();
    expect(await screen.findByText("python train.py --lr 3e-4")).toBeInTheDocument();
  });

  it("explains the empty state", async () => {
    serve([]);
    renderPage();
    expect(await screen.findByText(/No runs recorded yet/)).toBeInTheDocument();
  });
});
