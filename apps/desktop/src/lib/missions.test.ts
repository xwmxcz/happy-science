/** Verifies the browser adapter preserves the mission-kernel HTTP contract. */
import { beforeEach, describe, expect, it, vi } from "vitest";

const gateway = vi.hoisted(() => ({ get: vi.fn(), post: vi.fn() }));

vi.mock("./webMode", () => ({
  isGatewayWeb: true,
  gatewayGet: gateway.get,
  gatewayPost: gateway.post,
}));
vi.mock("./tauri", () => ({ isTauri: false }));

import {
  checkMission,
  captureLiterature,
  createResearchRelease,
  decideEvidence,
  importResearchRelease,
  listMissions,
  planMission,
  recordResearchDecision,
  searchLiterature,
  startMission,
  transitionMission,
  verifyResearchRelease,
} from "./missions";

describe("mission kernel gateway adapter", () => {
  beforeEach(() => {
    gateway.get.mockReset();
    gateway.post.mockReset();
  });

  it("plans with only the typed kind and rigor contract", async () => {
    const plan = { mission: { missionId: "hsm_1" }, prompt: "compiled by core" };
    gateway.post.mockResolvedValue(plan);

    await expect(planMission("evidence-sprint", "publication")).resolves.toBe(plan);
    expect(gateway.post).toHaveBeenCalledWith("/v1/missions", {
      kind: "evidence-sprint",
      rigor: "publication",
    });
  });

  it("binds, checks, records decisions, reviews, releases, and lists missions through kernel routes", async () => {
    const decisionResult = { review: { accepted: 1 }, claimPassports: [{ claimId: "cl_1" }] };
    const decisionLog = { records: 1, decisions: [{ decisionId: "hsd_1" }] };
    const release = { path: "releases/happy-science.zip", fingerprint: "abc" };
    gateway.post
      .mockResolvedValueOnce({ missionId: "hsm_1" })
      .mockResolvedValueOnce({ missionId: "hsm_1", readyForReview: true })
      .mockResolvedValueOnce(decisionResult)
      .mockResolvedValueOnce(decisionLog)
      .mockResolvedValueOnce(release);
    gateway.get.mockResolvedValue([]);

    await startMission("hsm_1", "ses_1");
    await checkMission("hsm_1");
    await expect(
      decideEvidence("hsm_1", "ev_1", "needs-review", "Check the sample definition"),
    ).resolves.toBe(decisionResult);
    const newDecision = {
      title: "Primary estimator",
      choice: "Use weighting",
      rationale: "Matches the estimand",
      alternatives: ["Complete cases"],
    };
    await expect(recordResearchDecision("hsm_1", newDecision)).resolves.toBe(decisionLog);
    await expect(createResearchRelease("hsm_1")).resolves.toBe(release);
    await expect(listMissions()).resolves.toEqual([]);

    expect(gateway.post).toHaveBeenNthCalledWith(1, "/v1/missions/hsm_1/start", {
      sessionId: "ses_1",
    });
    expect(gateway.post).toHaveBeenNthCalledWith(2, "/v1/missions/hsm_1/check", {});
    expect(gateway.post).toHaveBeenNthCalledWith(
      3,
      "/v1/missions/hsm_1/evidence-decisions",
      {
        evidenceId: "ev_1",
        verdict: "needs-review",
        note: "Check the sample definition",
      },
    );
    expect(gateway.post).toHaveBeenNthCalledWith(
      4,
      "/v1/missions/hsm_1/decisions",
      newDecision,
    );
    expect(gateway.post).toHaveBeenNthCalledWith(5, "/v1/missions/hsm_1/release", {});
    expect(gateway.get).toHaveBeenCalledWith("/v1/missions");
  });

  it("searches and captures literature through mission-scoped routes", async () => {
    const search = { provider: "crossref", works: [{ doi: "10.1000/test" }] };
    const captured = { added: true, corpus: { records: 1 } };
    const work = {
      doi: "10.1000/test",
      title: "Study",
      authors: [],
      landingUrl: "https://doi.org/10.1000/test",
      fullTextUrls: [],
    };
    gateway.post.mockResolvedValueOnce(search).mockResolvedValueOnce(captured);

    await expect(searchLiterature("hsm_1", "causal inference", 12)).resolves.toBe(search);
    await expect(captureLiterature("hsm_1", work)).resolves.toBe(captured);

    expect(gateway.post).toHaveBeenNthCalledWith(
      1,
      "/v1/missions/hsm_1/literature/search",
      { query: "causal inference", limit: 12 },
    );
    expect(gateway.post).toHaveBeenNthCalledWith(
      2,
      "/v1/missions/hsm_1/literature/capture",
      { work },
    );
  });

  it("transitions a mission through the kernel-owned lifecycle route", async () => {
    const paused = {
      missionId: "hsm_1",
      status: "paused",
      statusReason: "Researcher paused the mission",
    };
    gateway.post.mockResolvedValue(paused);

    await expect(
      transitionMission("hsm_1", "pause", "Researcher paused the mission"),
    ).resolves.toBe(paused);
    expect(gateway.post).toHaveBeenCalledWith("/v1/missions/hsm_1/transition", {
      action: "pause",
      reason: "Researcher paused the mission",
    });
  });

  it("verifies and safely imports release packages through workspace-scoped routes", async () => {
    const verification = { valid: true, path: "releases/happy-science.zip", issues: [] };
    const imported = { destinationPath: "imports/happy-science-hsm_1-abc-1234" };
    gateway.post.mockResolvedValueOnce(verification).mockResolvedValueOnce(imported);

    await expect(verifyResearchRelease("releases/happy-science.zip")).resolves.toBe(verification);
    await expect(importResearchRelease("releases/happy-science.zip")).resolves.toBe(imported);

    expect(gateway.post).toHaveBeenNthCalledWith(1, "/v1/releases/verify", {
      path: "releases/happy-science.zip",
    });
    expect(gateway.post).toHaveBeenNthCalledWith(2, "/v1/releases/import", {
      path: "releases/happy-science.zip",
    });
  });
});
