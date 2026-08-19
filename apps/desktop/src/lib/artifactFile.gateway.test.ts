/** Verifies gateway downloads exchange the bearer token for a one-file capability. */
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./tauri", () => ({ isTauri: false }));
vi.mock("./webMode", () => ({
  isGatewayWeb: true,
  gatewayToken: () => "gateway-secret",
  gatewayOrigin: () => "https://research.local",
}));

const { presentArtifact } = await import("./artifactFile");

describe("gateway artifact delivery", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("downloads through a one-file ticket without putting the gateway token in the URL", async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ticket: "one-file-ticket" }),
    });
    vi.stubGlobal("fetch", fetch);
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});

    await expect(
      presentArtifact("releases/happy science.zip", "happy-science.zip"),
    ).resolves.toBe(true);

    expect(fetch).toHaveBeenCalledWith(
      "https://research.local/v1/fs/ticket?path=releases%2Fhappy%20science.zip",
      { headers: { authorization: "Bearer gateway-secret" } },
    );
    expect(click).toHaveBeenCalledTimes(1);
    const link = click.mock.instances[0] as unknown as HTMLAnchorElement;
    expect(link.href).toBe("https://research.local/v1/fs/read?ticket=one-file-ticket");
    expect(link.href).not.toContain("gateway-secret");
    expect(link.download).toBe("happy-science.zip");
  });
});
