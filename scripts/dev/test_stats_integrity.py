#!/usr/bin/env python3
"""Tests for the core-integrity review adapter.

Run: python scripts/dev/test_stats_integrity.py
Stdlib unittest only; detection rules are tested in `osd-core`.
"""
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True

_MOD = (
    Path(__file__).resolve().parents[2]
    / "runtime/skills/core/stats-integrity/stats_integrity_check.py"
)
_spec = importlib.util.spec_from_file_location("stats_integrity_check", _MOD)
assert _spec and _spec.loader
si = importlib.util.module_from_spec(_spec)
sys.modules["stats_integrity_check"] = si
_spec.loader.exec_module(si)


class Adapter(unittest.TestCase):
    def test_renders_the_latest_persisted_result(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / ".openscience"
            store.mkdir()
            old = {"runId": "old", "integrity": {"findings": []}}
            finding = {
                "level": "warn",
                "tag": "stats · prereg",
                "title": "Predictor not in the research plan",
                "evidence": "analysis.py:2",
                "kind": "unregistered-predictor",
                "path": "analysis.py",
                "line": 2,
            }
            latest = {"runId": "new", "integrity": {"findings": [finding]}}
            (store / "runs.jsonl").write_text(
                json.dumps(old) + "\n" + json.dumps(latest) + "\n",
                encoding="utf-8",
            )
            result = si.run(tmp)
            self.assertEqual(result["findings"][0]["check"], "integrity")
            self.assertEqual(result["findings"][0]["tag"], "stats · prereg")
            self.assertIn("specific risks", result["note"])

    def test_missing_store_is_an_empty_review(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(si.run(tmp)["findings"], [])

    def test_skips_malformed_trailing_records(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / ".openscience"
            store.mkdir()
            record = {
                "integrity": {
                    "findings": [{"title": "Seed", "tag": "stats · seed"}]
                }
            }
            (store / "runs.jsonl").write_text(
                json.dumps(record) + "\n{bad\n", encoding="utf-8"
            )
            self.assertEqual(si.run(tmp)["findings"][0]["title"], "Seed")


if __name__ == "__main__":
    unittest.main(verbosity=2)
