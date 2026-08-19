import { tool } from "@opencode-ai/plugin";
import { stat } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";

type DisplayMode = "inline" | "panel";
type Placement = "right" | "bottom";
type Target = "current-screen" | "new-screen" | "new-session";

function workspacePath(directory: string, requestedPath: string): string {
  if (isAbsolute(requestedPath)) {
    throw new Error("path must be relative to the current workspace");
  }
  const root = resolve(directory);
  const target = resolve(root, requestedPath);
  const rel = relative(root, target);
  if (!rel || rel === ".." || rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`)) {
    throw new Error("path must name a file inside the current workspace");
  }
  return target;
}

export default tool({
  description:
    "Actually display an existing workspace artifact in the Happy Science UI. You MUST call this tool when the user asks to show, display, render, preview, or open a file in the conversation or a Screen panel; reading the file or mentioning its path does not display it. Use inline for the conversation. For panel display, target can use the current Screen, create a new Screen with the source conversation, or create a real dedicated Agent Session in a new Screen. The file must already exist, and you must not claim it is displayed unless this call succeeds.",
  args: {
    path: tool.schema
      .string()
      .min(1)
      .describe("Workspace-relative path of the existing image, table, HTML, PDF, document, or other previewable file."),
    display: tool.schema
      .enum(["inline", "panel"])
      .describe("inline renders at this point in the conversation; panel creates a dedicated artifact panel."),
    placement: tool.schema
      .enum(["right", "bottom"])
      .optional()
      .describe(
        "Where the panel goes relative to the conversation: right (side by side) or bottom (below it). Pass bottom whenever the user asks for the artifact below, underneath, or in a bottom panel; an already open panel moves to the side you pass. Ignored for inline display. Defaults to right.",
      ),
    target: tool.schema
      .enum(["current-screen", "new-screen", "new-session"])
      .optional()
      .describe("Panel destination: use the current Screen, create a new Screen, or create a dedicated Agent Session in a new Screen. Ignored for inline display. Defaults to current-screen."),
    title: tool.schema
      .string()
      .min(1)
      .max(160)
      .optional()
      .describe("Optional concise title shown above the artifact. Defaults to the filename."),
  },
  async execute(args, context) {
    const target = workspacePath(context.directory, args.path);
    const metadata = await stat(target);
    if (!metadata.isFile()) {
      throw new Error("path must name an existing file");
    }
    const display = args.display as DisplayMode;
    const placement = (args.placement ?? "right") as Placement;
    const presentationTarget = (args.target ?? "current-screen") as Target;
    return JSON.stringify({
      presented: true,
      path: args.path,
      display,
      ...(display === "panel" ? { placement, target: presentationTarget } : {}),
      ...(args.title ? { title: args.title } : {}),
    });
  },
});
