import { screen, within } from "@testing-library/react";
import { describe, expect, it, beforeEach } from "vitest";
import { useUiStore } from "@/lib/store";
import { renderAt } from "@/test/render";

const base = "/example";

describe("SessionPage", () => {
  beforeEach(() => useUiStore.setState({ inspectorOpen: true }));

  it("renders the Happy Science manuscript stress test with its blocking finding", () => {
    renderAt(`${base}/happy-manuscript-audit`);
    expect(screen.getAllByText("Manuscript Stress Test — battery cycle life").length).toBeGreaterThan(0);
    expect(screen.getByText(/Figure 2 reports 84% retention/)).toBeInTheDocument();
    const inspector = document.querySelector('[data-variant="pdf"]');
    expect(inspector).toBeInTheDocument();
    expect(within(inspector as HTMLElement).getByText("stress-test.pdf")).toBeInTheDocument();
  });

  it("renders the Happy Science evidence sprint with linked evidence and review", () => {
    renderAt(`${base}/happy-evidence-sprint`);
    expect(screen.getAllByText("Evidence Sprint — urban cooling claim").length).toBeGreaterThan(0);
    expect(screen.getByText("HS-DEMO-003")).toBeInTheDocument();
    expect(screen.getByText(/Four records measure modeled temperature/)).toBeInTheDocument();
  });

  it("renders the Happy Science reproduction with a figure and artifact inspector", () => {
    renderAt(`${base}/happy-reproduction`);
    expect(screen.getAllByText("Reproduction Challenge — dose response").length).toBeGreaterThan(0);
    expect(screen.getByText("reproduction-convergence.svg")).toBeInTheDocument();
    expect(document.querySelector('[data-variant="artifact"]')).toBeInTheDocument();
    expect(screen.getByText("Download script")).toBeInTheDocument();
  });

  it("uses the Happy Science preregistration example as a governed launch contract", () => {
    renderAt(`${base}/happy-preregistration`);
    expect(screen.getAllByText("Research Launch — seed germination").length).toBeGreaterThan(0);
    expect(screen.getByText(/All consequential choices were fixed/)).toBeInTheDocument();
    expect(screen.getByText("research/protocol.md")).toBeInTheDocument();
  });

  it("shows a not-found state for an unknown session", () => {
    renderAt(`${base}/nope`);
    expect(screen.getByText("Session not found")).toBeInTheDocument();
  });
});
