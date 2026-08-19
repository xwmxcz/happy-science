#!/usr/bin/env python3
"""Render the latest core-owned automatic integrity result as a review block.

Detection lives in `osd-core::research_integrity` and is persisted on each
local run. This adapter deliberately owns no rules; it only turns that single
contract into the structured review output understood by the conversation UI.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path


NOTE = (
    "Automatic run-integrity result — flags plan deviations, missing seeds, "
    "and causal overreach. It checks these specific risks only; absence of "
    "findings does not certify the analysis or its conclusions."
)


def _latest_integrity(root: Path) -> dict | None:
    store = root / ".openscience" / "runs.jsonl"
    try:
        lines = store.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return None
    for line in reversed(lines):
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        integrity = record.get("integrity")
        if isinstance(integrity, dict):
            return integrity
    return None


def run(root: str | None = None) -> dict:
    integrity = _latest_integrity(Path(root) if root else Path.cwd())
    findings = integrity.get("findings", []) if integrity else []
    return {
        "findings": [
            {
                "level": finding.get("level", "warn"),
                "check": "integrity",
                "tag": finding.get("tag", "integrity"),
                "title": finding.get("title", "Integrity finding"),
                "evidence": finding.get("evidence", ""),
            }
            for finding in findings
            if isinstance(finding, dict)
        ],
        "note": NOTE,
    }


def main(argv: list[str]) -> int:
    root = argv[1] if len(argv) > 1 else None
    print("```review")
    print(json.dumps(run(root), ensure_ascii=False))
    print("```")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
