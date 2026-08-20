# Happy Science vs OpenScience: two open-source Claude Science alternatives

Happy Science and Synthetic Sciences OpenScience are both open-source
AI-for-science workbenches. The names are similar, but the product focus is
different enough that researchers can choose by workflow.

## Short version

- **Choose Happy Science** if you want a local-first desktop app for
  Windows and Linux, with local files, notebook-style work, artifact provenance, and
  reproducible run records.
- **Choose Synthetic Sciences OpenScience** if you want a browser workspace and a
  CLI-driven research loop with its `openscience` command, TypeScript SDK, agents,
  and built-in science skills.

## Comparison

| Dimension | Happy Science | Synthetic Sciences OpenScience |
| --- | --- | --- |
| Positioning | Local-first desktop research workbench | Browser workspace / research-agent workflow |
| Primary surface | Windows and Linux desktop app | Browser workspace and `openscience` CLI |
| Technical stack | Tauri 2, React, MCP, OpenCode sidecar, agent skills, local provenance | TypeScript workspace, agents, skills, MCP/plugins, TypeScript SDK |
| Data/workspace model | Local workspace folders, local notebooks, files, previews, provenance, and run records | Browser workspace with file tree, editor, terminal, session history, and scientific renderers |
| Reproducibility focus | Artifact provenance, append-only run logs, SQLite run index, local/remote run records | End-to-end research loop: literature, hypothesis, code, experiments, analysis, write-up |
| Platform fit | Researchers who want a desktop app, local files, private workspaces, and Windows/Linux support | Researchers who want browser workspace and CLI-first extensibility |
| License | MIT | Apache-2.0 |

## Notes

This is a neutral comparison, not an endorsement or criticism of either project.
Both projects are independent and open source. The practical difference is product
surface: Open Science Desktop emphasizes a desktop, local-first workflow; Synthetic
Sciences OpenScience emphasizes a browser workspace and CLI-driven research loop.

## Sources

- Happy Science GitHub: <https://github.com/xwmxcz/happy-science>
- Synthetic Sciences OpenScience GitHub: <https://github.com/synthetic-sciences/openscience>
- Synthetic Sciences OpenScience docs repository: <https://github.com/synthetic-sciences/docs>
