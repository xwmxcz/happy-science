import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderAt } from "@/test/render";
import { useUiStore } from "@/lib/store";
import { Composer } from "./Composer";
import { ResearchWorkbench } from "@/components/research/ResearchWorkbench";

// COPYCAT RULE: useUiStore is module-global; reset the locale after each test
// so this suite never bleeds a non-English locale into other test files.
afterEach(() => useUiStore.getState().setLocale("en"));

describe("Composer strings (i18n)", () => {
  it("renders the default placeholder and the approval-mode switch in English", () => {
    render(<Composer onSend={() => {}} approvalMode="approve" onApprovalModeChange={() => {}} />);
    expect(screen.getByPlaceholderText("Ask anything")).toBeInTheDocument();
    expect(screen.getByLabelText("Approval mode")).toHaveTextContent("Approve for me");
  });
});

describe("ResearchWorkbench strings (i18n)", () => {
  it("renders the research brief and a mission's title/description in English", () => {
    render(<ResearchWorkbench onLaunch={() => {}} />);
    expect(screen.getByText("Define the mission. Set the proof standard.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Study launch/ })).toBeInTheDocument();
    expect(
      screen.getByText(
        "Map consensus, contradictions, and research gaps with a verified search trail.",
      ),
    ).toBeInTheDocument();
  });
});

describe("LiveSessionPage strings (i18n)", () => {
  it("keeps the research workbench primary while the executor is disconnected", async () => {
    renderAt("/live");
    expect(await screen.findByText("Define the mission. Set the proof standard.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Launch research mission" })).toBeDisabled();
    expect(screen.queryByText("OpenCode runtime")).not.toBeInTheDocument();
  });
});
