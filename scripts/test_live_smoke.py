#!/usr/bin/env python3
"""Offline tests for the private checker; never contact Concept2 or load a real token."""

import contextlib
import copy
import io
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import live_smoke


class LiveSmokeTests(unittest.TestCase):
    @staticmethod
    def summary_fixture(rows, start, end):
        equipment = sorted({row["equipment"] for row in rows})
        groups = sorted({(row["date"][:10], row["equipment"]) for row in rows})
        return {"warnings": [], "data": {
            "filters": {"from": start, "to": end, "group_by": "day"},
            "date_basis": "recorded_local_workout_date",
            "coverage": {"complete": True, "pages_fetched": 2, "records_included": len(rows), "reported_total": len(rows), "reason": None, "snapshot_guaranteed": False},
            "summary": {
                "records_included": len(rows),
                "totals": live_smoke.source_totals(rows),
                "by_equipment": [{"equipment": name, "totals": live_smoke.source_totals([row for row in rows if row["equipment"] == name])} for name in equipment],
                "groups": [{"period": day, "equipment": name, "totals": live_smoke.source_totals([row for row in rows if row["date"][:10] == day and row["equipment"] == name])} for day, name in groups],
            },
        }}

    @staticmethod
    def range_rows():
        return [
            {"id": 3, "date": "2026-09-02 10:00:00", "equipment": "rower", "distance_m": 2000, "duration_tenths": 4500},
            {"id": 1, "date": "2026-09-01 10:00:00", "equipment": "rower", "distance_m": 1000, "duration_tenths": 2400},
            {"id": 2, "date": "2026-09-02 12:00:00", "equipment": "bike", "distance_m": 3000, "duration_tenths": 3000},
        ]

    def test_complete_reference_checks_its_full_range_and_each_day_equipment_group(self):
        rows = self.range_rows()
        session = mock.Mock()
        session.call.return_value = self.summary_fixture(rows, "2026-09-01", "2026-09-02")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            live_smoke.check_summary(session, rows, True)
        session.call.assert_called_once_with("concept2_summarize_workouts", {"from": "2026-09-01", "to": "2026-09-02", "group_by": "day"}, "summary_read")
        self.assertIn("records=3 equipment_groups=2 date_groups=3", output.getvalue())
        self.assertNotIn("2026-09", output.getvalue())

    def test_complete_reference_detects_a_wrong_day_group_or_duplicate_group(self):
        rows = self.range_rows()
        result = self.summary_fixture(rows, "2026-09-01", "2026-09-02")
        for duplicate in (False, True):
            invalid = copy.deepcopy(result)
            groups = invalid["data"]["summary"]["groups"]
            if duplicate:
                groups.append(copy.deepcopy(groups[0]))
            else:
                groups[0]["period"] = "2026-09-03"
            session = mock.Mock()
            session.call.return_value = invalid
            with contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(live_smoke.CheckFailure, "^independent_date_groups$"):
                live_smoke.check_summary(session, rows, True)

    def test_incomplete_reference_keeps_one_day_and_skips_reconciliation(self):
        rows = self.range_rows()
        first_day_rows = [row for row in rows if row["date"].startswith("2026-09-02")]
        session = mock.Mock()
        session.call.return_value = self.summary_fixture(first_day_rows, "2026-09-02", "2026-09-02")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            live_smoke.check_summary(session, rows, False)
        session.call.assert_called_once_with("concept2_summarize_workouts", {"from": "2026-09-02", "to": "2026-09-02", "group_by": "day"}, "summary_read")
        self.assertIn("SKIP independent_totals_reference_exceeds_two_pages_or_changed", output.getvalue())
        self.assertNotIn("PASS independent_source_integer_totals", output.getvalue())

    def test_independent_arithmetic_preserves_zero_and_missing_rest(self):
        rows = [
            {"distance_m": 2000, "duration_tenths": 4501, "rest_distance_m": 0, "rest_duration_tenths": 101},
            {"distance_m": 1000, "duration_tenths": 2402, "rest_distance_m": None, "rest_duration_tenths": None},
        ]
        totals = live_smoke.source_totals(rows)
        self.assertEqual(totals["session_count"], 2)
        self.assertEqual(totals["distance_m"], 3000)
        self.assertEqual(totals["work_duration_tenths"], 6903)
        self.assertEqual(totals["work_duration_seconds"], 690.3)
        self.assertEqual(totals["known_rest_distance_m"], 0)
        self.assertEqual(totals["known_rest_duration_tenths"], 101)
        self.assertEqual(totals["records_with_rest_distance"], 1)
        self.assertIsNone(live_smoke.source_totals([])["known_rest_distance_m"])

    def test_independent_totals_detect_a_real_mismatch(self):
        live_smoke.equal_totals({"work_duration_seconds": 0.30000000000000004}, {"work_duration_seconds": 0.3})
        with self.assertRaisesRegex(live_smoke.CheckFailure, "^independent_totals$"):
            live_smoke.equal_totals({"distance_m": 1999}, {"distance_m": 2000})

    def test_schema_structure_references_and_required_fields(self):
        schema = {
            "$ref": "#/$defs/Record",
            "$defs": {"Record": {
                "type": "object", "required": ["count", "optional"],
                "properties": {
                    "count": {"type": "integer", "minimum": 0},
                    "optional": {"anyOf": [{"type": "string"}, {"type": "null"}]},
                },
                "additionalProperties": False,
            }},
        }
        self.assertTrue(live_smoke.schema_matches({"count": 0, "optional": None}, schema))
        for invalid in ({"count": True, "optional": None}, {"count": -1, "optional": None}, {"count": 0}, {"count": 0, "optional": None, "extra": 1}):
            self.assertFalse(live_smoke.schema_matches(invalid, schema))
        self.assertFalse(live_smoke.schema_matches({}, {"$ref": "https://example.invalid/schema"}))
        self.assertFalse(live_smoke.schema_matches({}, {"unsupportedAssertion": True}))

    def test_tool_failure_does_not_expose_response_text(self):
        session = live_smoke.Session.__new__(live_smoke.Session)
        session.request = lambda *_: {
            "isError": True,
            "content": [{"type": "text", "text": "PRIVATE_SYNTHETIC_RESPONSE"}],
        }
        with self.assertRaises(live_smoke.CheckFailure) as raised:
            session.call("concept2_get_profile", {}, "profile_read")
        self.assertEqual(str(raised.exception), "profile_read")
        self.assertNotIn("PRIVATE", str(raised.exception))

    def test_unexpected_exception_is_sanitized_and_process_is_closed(self):
        session = mock.Mock()
        output = io.StringIO()
        with mock.patch.object(live_smoke, "read_token", return_value="SYNTHETIC_SECRET"), mock.patch.object(live_smoke, "Session", return_value=session), mock.patch.object(live_smoke, "checks", side_effect=RuntimeError("PRIVATE_RESPONSE SYNTHETIC_SECRET")), contextlib.redirect_stdout(output):
            with self.assertRaises(live_smoke.CheckFailure) as raised:
                live_smoke.run(Path(__file__), Path("unused-private-path"))
        self.assertEqual(str(raised.exception), "unexpected_check_failure")
        self.assertNotIn("PRIVATE", output.getvalue())
        self.assertNotIn("SYNTHETIC_SECRET", output.getvalue())
        session.close.assert_called_once()

    def test_token_file_must_be_private_and_is_never_printed(self):
        with self.assertRaisesRegex(live_smoke.CheckFailure, "^token_file_must_be_outside_repository$"):
            live_smoke.read_token(Path(__file__))
        output = io.StringIO()
        with tempfile.TemporaryDirectory(prefix="serenity-synthetic-token-test-") as directory:
            path = Path(directory) / "synthetic-token.txt"
            path.write_text("SYNTHETIC_TOKEN_FOR_TEST_ONLY\n", encoding="utf-8")
            with contextlib.redirect_stdout(output):
                self.assertEqual(live_smoke.read_token(path), "SYNTHETIC_TOKEN_FOR_TEST_ONLY")
        self.assertEqual(output.getvalue(), "")

    def test_parser_failure_does_not_echo_arguments(self):
        parser = live_smoke.PrivateArgumentParser()
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr), self.assertRaises(SystemExit) as raised:
            parser.parse_args(["--unknown", "SYNTHETIC_SECRET_ARGUMENT"])
        self.assertEqual(raised.exception.code, 2)
        self.assertEqual(stderr.getvalue(), "FAIL command_arguments\n")


if __name__ == "__main__":
    unittest.main()
