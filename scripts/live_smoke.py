#!/usr/bin/env python3
"""Opt-in, private, read-only acceptance checks against a Concept2 account.

Requires Python 3.10+. The token file must be outside this repository. Account
responses remain in memory; output contains only fixed check names and counts.
Only the supplied server executable makes network requests. This script writes
no reports, fixtures, logs, or credential files.
"""

import argparse
from datetime import date, datetime
import json
import math
import os
from pathlib import Path
import queue
import re
import subprocess
import threading
import time


EXPECTED_TOOLS = {
    "concept2_get_profile",
    "concept2_list_workouts",
    "concept2_get_workout",
    "concept2_get_strokes",
    "concept2_summarize_workouts",
}
MAX_MESSAGE_BYTES = 4 * 1024 * 1024
MAX_STDOUT_BYTES = 32 * 1024 * 1024
MAX_STDERR_BYTES = 256 * 1024
MAX_TOKEN_BYTES = 16 * 1024
OVERALL_SECONDS = 180
CALL_SECONDS = 35


class CheckFailure(Exception):
    """A fixed public check name, never an upstream exception or response."""


def require(condition, check):
    if not condition:
        raise CheckFailure(check)


def announce(status, check, **counts):
    suffix = "".join(f" {key}={value}" for key, value in counts.items())
    print(f"{status} {check}{suffix}", flush=True)


def integer(value):
    return isinstance(value, int) and not isinstance(value, bool)


def schema_matches(value, schema, root=None, depth=0):
    """Validate the structural JSON Schema vocabulary emitted by this server.

    This intentionally fails closed on unsupported assertion keywords. It is
    not a general JSON Schema library; semantic checks below cover units,
    coverage, IDs, timestamps, and summary arithmetic independently.
    """
    if isinstance(schema, bool):
        return schema
    if not isinstance(schema, dict) or depth > 64:
        return False
    root = schema if root is None else root
    known = {
        "$schema", "$id", "$defs", "definitions", "$ref", "title", "description",
        "default", "examples", "deprecated", "readOnly", "writeOnly", "format",
        "type", "properties", "required", "additionalProperties", "items", "enum",
        "const", "anyOf", "allOf", "oneOf", "minimum", "maximum",
        "exclusiveMinimum", "exclusiveMaximum", "minLength", "maxLength", "pattern",
        "minItems", "maxItems", "uniqueItems", "minProperties", "maxProperties",
    }
    if set(schema) - known:
        return False
    if "$ref" in schema:
        reference = schema["$ref"]
        if not isinstance(reference, str) or not reference.startswith("#/"):
            return False
        target = root
        for part in reference[2:].split("/"):
            if not isinstance(target, dict):
                return False
            target = target.get(part.replace("~1", "/").replace("~0", "~"))
        if not schema_matches(value, target, root, depth + 1):
            return False
    for keyword, predicate in (("anyOf", any), ("allOf", all)):
        if keyword in schema and not predicate(
            schema_matches(value, branch, root, depth + 1) for branch in schema[keyword]
        ):
            return False
    if "oneOf" in schema and sum(
        schema_matches(value, branch, root, depth + 1) for branch in schema["oneOf"]
    ) != 1:
        return False
    type_checks = {
        "null": value is None,
        "boolean": isinstance(value, bool),
        "integer": integer(value),
        "number": isinstance(value, (int, float)) and not isinstance(value, bool)
        and math.isfinite(value),
        "string": isinstance(value, str),
        "array": isinstance(value, list),
        "object": isinstance(value, dict),
    }
    if "type" in schema:
        allowed = schema["type"]
        allowed = [allowed] if isinstance(allowed, str) else allowed
        if not any(type_checks.get(kind, False) for kind in allowed):
            return False
    if "enum" in schema and value not in schema["enum"]:
        return False
    if "const" in schema and value != schema["const"]:
        return False
    if isinstance(value, dict):
        if not set(schema.get("required", [])).issubset(value):
            return False
        if len(value) < schema.get("minProperties", 0) or len(value) > schema.get("maxProperties", math.inf):
            return False
        properties = schema.get("properties", {})
        for key, item in value.items():
            item_schema = properties.get(key, schema.get("additionalProperties", True))
            if not schema_matches(item, item_schema, root, depth + 1):
                return False
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0) or len(value) > schema.get("maxItems", math.inf):
            return False
        if "items" in schema and not all(
            schema_matches(item, schema["items"], root, depth + 1) for item in value
        ):
            return False
        if schema.get("uniqueItems") and len({json.dumps(item, sort_keys=True) for item in value}) != len(value):
            return False
    if isinstance(value, str):
        if len(value) < schema.get("minLength", 0) or len(value) > schema.get("maxLength", math.inf):
            return False
        if "pattern" in schema and re.search(schema["pattern"], value) is None:
            return False
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if value < schema.get("minimum", -math.inf) or value > schema.get("maximum", math.inf):
            return False
        if "exclusiveMinimum" in schema and value <= schema["exclusiveMinimum"]:
            return False
        if "exclusiveMaximum" in schema and value >= schema["exclusiveMaximum"]:
            return False
    return True


class Session:
    def __init__(self, binary, token):
        environment = os.environ.copy()
        environment["CONCEPT2_ACCESS_TOKEN"] = token
        self.process = subprocess.Popen(
            [str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, env=environment,
        )
        self.messages = queue.Queue(maxsize=4)
        self.failure = threading.Event()
        self.token_leak = threading.Event()
        self.deadline = time.monotonic() + OVERALL_SECONDS
        self.next_id = 0
        self.schemas = {}
        self.token_bytes = token.encode("ascii")
        self.threads = [
            threading.Thread(target=self._stdout, daemon=True),
            threading.Thread(target=self._stderr, daemon=True),
        ]
        for thread in self.threads:
            thread.start()

    def _stdout(self):
        total = 0
        try:
            while True:
                line = self.process.stdout.readline(MAX_MESSAGE_BYTES + 1)
                if not line:
                    return
                total += len(line)
                if len(line) > MAX_MESSAGE_BYTES or total > MAX_STDOUT_BYTES or not line.endswith(b"\n"):
                    self.failure.set()
                    return
                if self.token_bytes in line:
                    self.token_leak.set()
                value = json.loads(line)
                if not isinstance(value, dict):
                    self.failure.set()
                    return
                self.messages.put(value, timeout=1)
        except Exception:
            # JSON parser errors can contain input fragments; never retain them.
            self.failure.set()

    def _stderr(self):
        total = 0
        tail = b""
        try:
            while True:
                chunk = self.process.stderr.read(4096)
                if not chunk:
                    return
                total += len(chunk)
                if self.token_bytes in tail + chunk:
                    self.token_leak.set()
                tail = (tail + chunk)[-max(0, len(self.token_bytes) - 1):]
                if total > MAX_STDERR_BYTES:
                    self.failure.set()
                    return
        except Exception:
            self.failure.set()

    def send(self, value):
        require(time.monotonic() < self.deadline, "overall_deadline")
        try:
            self.process.stdin.write(json.dumps(value, separators=(",", ":")).encode("utf-8") + b"\n")
            self.process.stdin.flush()
        except Exception:
            raise CheckFailure("protocol_write") from None

    def request(self, method, params):
        self.next_id += 1
        request_id = self.next_id
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = min(self.deadline, time.monotonic() + CALL_SECONDS)
        for _ in range(20):
            while True:
                require(not self.failure.is_set(), "protocol_stream")
                require(not self.token_leak.is_set(), "credential_redaction")
                remaining = deadline - time.monotonic()
                require(remaining > 0, "request_deadline")
                try:
                    value = self.messages.get(timeout=min(remaining, 0.25))
                    break
                except queue.Empty:
                    require(self.process.poll() is None, "server_stopped")
            if value.get("id") == request_id:
                require("error" not in value, "protocol_response")
                require(isinstance(value.get("result"), dict), "protocol_response")
                return value["result"]
        raise CheckFailure("protocol_notifications")

    def connect(self):
        initialized = self.request("initialize", {
            "protocolVersion": "2025-11-25", "capabilities": {},
            "clientInfo": {"name": "serenity-private-live-smoke", "version": "0.1.0"},
        })
        require(initialized.get("serverInfo", {}).get("name") == "serenity-concept2-mcp", "server_identity")
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        tools = self.request("tools/list", {}).get("tools")
        require(isinstance(tools, list), "tool_inventory")
        require({tool.get("name") for tool in tools} == EXPECTED_TOOLS, "tool_inventory")
        for tool in tools:
            require(tool.get("annotations", {}).get("readOnlyHint") is True, "read_only_annotations")
            require(tool.get("annotations", {}).get("destructiveHint") is False, "read_only_annotations")
            require(isinstance(tool.get("outputSchema"), dict), "output_schema_advertised")
            self.schemas[tool["name"]] = tool["outputSchema"]
        announce("PASS", "protocol_and_read_only_tools", tools=len(tools))

    def call(self, name, arguments, check):
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        require(result.get("isError") is not True, check)
        value = result.get("structuredContent")
        require(schema_matches(value, self.schemas[name]), "output_schema")
        require(isinstance(value, dict) and isinstance(value.get("data"), dict), "result_envelope")
        try:
            timestamp = datetime.fromisoformat(value["fetched_at"].replace("Z", "+00:00"))
            require(timestamp.tzinfo is not None, "fetch_timestamp")
        except (TypeError, ValueError, KeyError):
            raise CheckFailure("fetch_timestamp") from None
        require(isinstance(result.get("content"), list) and bool(result["content"]), "text_fallback")
        return value

    def close(self):
        clean = True
        try:
            if self.process.stdin and not self.process.stdin.closed:
                self.process.stdin.close()
            self.process.wait(timeout=5)
        except Exception:
            clean = False
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait(timeout=5)
        finally:
            for thread in self.threads:
                thread.join(timeout=2)
            for stream in (self.process.stdout, self.process.stderr):
                stream.close()
        require(clean and self.process.returncode == 0, "process_shutdown")
        require(not self.failure.is_set(), "protocol_stream")
        require(not self.token_leak.is_set(), "credential_redaction")


def source_totals(workouts):
    """Independent arithmetic over exact source-unit integers in list results."""
    rests = [row["rest_distance_m"] for row in workouts if row.get("rest_distance_m") is not None]
    rest_times = [row["rest_duration_tenths"] for row in workouts if row.get("rest_duration_tenths") is not None]
    work_tenths = sum(row["duration_tenths"] for row in workouts)
    return {
        "session_count": len(workouts),
        "distance_m": sum(row["distance_m"] for row in workouts),
        "work_duration_tenths": work_tenths,
        "work_duration_seconds": work_tenths / 10,
        "known_rest_distance_m": sum(rests) if rests else None,
        "known_rest_duration_tenths": sum(rest_times) if rest_times else None,
        "known_rest_duration_seconds": sum(rest_times) / 10 if rest_times else None,
        "records_with_rest_distance": len(rests),
        "records_with_rest_duration": len(rest_times),
    }


def equal_totals(actual, expected):
    for key, value in expected.items():
        if isinstance(value, float):
            require(isinstance(actual.get(key), (int, float)) and math.isclose(actual[key], value, rel_tol=1e-12, abs_tol=1e-9), "independent_totals")
        else:
            require(actual.get(key) == value, "independent_totals")


def check_summary(session, workouts, reference_complete):
    # A complete reference contains at most two pages / 500 records. Verify its
    # entire date range; otherwise use one day without claiming reconciliation.
    days = [row["date"][:10] for row in workouts]
    try:
        require(all(date.fromisoformat(day).isoformat() == day for day in days), "workout_date")
    except (TypeError, ValueError):
        raise CheckFailure("workout_date") from None
    start = min(days) if reference_complete else days[0]
    end = max(days) if reference_complete else days[0]
    result = session.call("concept2_summarize_workouts", {
        "from": start, "to": end, "group_by": "day",
    }, "summary_read")
    payload = result["data"]
    coverage = payload["coverage"]
    summary = payload["summary"]
    require(payload["filters"]["from"] == start and payload["filters"]["to"] == end, "summary_filters")
    require(payload["date_basis"] == "recorded_local_workout_date", "summary_date_basis")
    require(coverage["snapshot_guaranteed"] is False, "summary_coverage")
    require(0 <= coverage["pages_fetched"] <= 20 and 0 <= coverage["records_included"] <= 5000, "summary_budgets")
    require(coverage["records_included"] == summary["records_included"], "summary_coverage")
    if coverage["complete"]:
        require(coverage["reason"] is None and coverage["reported_total"] == coverage["records_included"], "summary_coverage")
    else:
        require(isinstance(coverage["reason"], str) and bool(coverage["reason"]), "summary_partial_reason")
        require(any("PARTIAL" in warning for warning in result["warnings"]), "summary_partial_warning")
    announce("PASS", "summary_schema_and_coverage", records=coverage["records_included"], pages=coverage["pages_fetched"])
    if not coverage["complete"]:
        announce("SKIP", "independent_totals_partial_summary")
        return
    if not reference_complete:
        announce("SKIP", "independent_totals_reference_exceeds_two_pages_or_changed")
        return
    selected = [row for row in workouts if start <= row["date"][:10] <= end]
    require(len({row["id"] for row in selected}) == len(selected), "independent_reference_ids")
    equal_totals(summary["totals"], source_totals(selected))
    expected_equipment = {row["equipment"] for row in selected}
    equipment = {entry["equipment"]: entry["totals"] for entry in summary["by_equipment"]}
    require(len(equipment) == len(summary["by_equipment"]), "independent_equipment_groups")
    require(set(equipment) == expected_equipment, "independent_equipment_groups")
    for name in expected_equipment:
        equal_totals(equipment[name], source_totals([row for row in selected if row["equipment"] == name]))
    groups = {(entry["period"], entry["equipment"]): entry["totals"] for entry in summary["groups"]}
    expected_groups = {(row["date"][:10], row["equipment"]) for row in selected}
    require(len(groups) == len(summary["groups"]) and set(groups) == expected_groups, "independent_date_groups")
    for day, name in expected_groups:
        equal_totals(groups[(day, name)], source_totals([
            row for row in selected if row["date"][:10] == day and row["equipment"] == name
        ]))
    announce("PASS", "independent_source_integer_totals", records=len(selected), equipment_groups=len(equipment), date_groups=len(groups))


def checks(session):
    session.connect()
    profile = session.call("concept2_get_profile", {}, "profile_read")["data"]
    require(not {"email", "dob", "first_name", "last_name"}.intersection(profile), "profile_privacy")
    announce("PASS", "profile_schema_and_privacy")
    workouts = []
    totals = []
    pages = 0
    next_page = 1
    for _ in range(2):
        if next_page is None:
            break
        result = session.call("concept2_list_workouts", {"page": next_page, "page_size": 250}, "workouts_read")["data"]
        require(result["filters"]["page"] == next_page and result["filters"]["page_size"] == 250, "list_filters")
        require(len(result["workouts"]) <= 250, "list_page_limit")
        require(result["next_page"] is None or result["next_page"] == next_page + 1, "list_pagination")
        workouts.extend(result["workouts"])
        totals.append(result["total"])
        pages += 1
        next_page = result["next_page"]
    announce("PASS", "workout_list_schema_and_pagination", pages=pages, records=len(workouts))
    if pages == 1:
        announce("SKIP", "second_page_unavailable")
    if not workouts:
        announce("SKIP", "detail_strokes_and_summary_empty_history")
        return
    first = workouts[0]
    detail = session.call("concept2_get_workout", {"workout_id": first["id"]}, "workout_detail_read")["data"]
    require(detail["id"] == first["id"], "workout_detail_identity")
    require(math.isclose(detail["duration_seconds"], detail["duration_tenths"] / 10, rel_tol=1e-12), "workout_duration_units")
    announce("PASS", "workout_detail_schema_and_units")
    strokes = session.call("concept2_get_strokes", {"workout_id": first["id"], "offset": 0, "limit": 10}, "stroke_read")["data"]
    require(strokes["workout_id"] == first["id"], "stroke_identity")
    if not strokes["available"]:
        require(strokes["window"] is None, "stroke_availability")
        announce("SKIP", "stroke_data_unavailable")
    else:
        window = strokes["window"]
        require(window["returned_count"] == len(window["strokes"]) <= 10, "stroke_window")
        for stroke in window["strokes"]:
            for source, normalized in (("elapsed_tenths", "elapsed_seconds"), ("distance_decimeters", "distance_m"), ("pace_tenths", "pace_seconds")):
                if stroke[source] is not None:
                    require(math.isclose(stroke[normalized], stroke[source] / 10, rel_tol=1e-12), "stroke_units")
        announce("PASS", "stroke_schema_window_and_units", records=window["returned_count"])
    reference_complete = (
        next_page is None and len(set(totals)) == 1 and totals[0] == len(workouts)
        and len({row["id"] for row in workouts}) == len(workouts)
    )
    check_summary(session, workouts, reference_complete)


def read_token(path):
    try:
        path = path.resolve(strict=True)
        repository = Path(__file__).resolve().parents[1]
        require(not path.is_relative_to(repository), "token_file_must_be_outside_repository")
        with path.open("rb") as token_file:
            data = token_file.read(MAX_TOKEN_BYTES + 1)
        require(len(data) <= MAX_TOKEN_BYTES, "token_file_size")
        token = data.decode("utf-8-sig").strip()
        require(token and all(33 <= ord(character) <= 126 for character in token), "token_file_format")
        return token
    except CheckFailure:
        raise
    except Exception:
        raise CheckFailure("token_file_unreadable") from None


def run(binary, token_file):
    try:
        binary = binary.resolve(strict=True)
        require(binary.is_file(), "binary_not_found")
    except CheckFailure:
        raise
    except Exception:
        raise CheckFailure("binary_not_found") from None
    token = read_token(token_file)
    session = None
    failure = None
    try:
        session = Session(binary, token)
        checks(session)
    except CheckFailure as error:
        failure = error
    except KeyboardInterrupt:
        failure = CheckFailure("interrupted")
    except Exception:
        failure = CheckFailure("unexpected_check_failure")
    finally:
        if session is not None:
            try:
                session.close()
            except CheckFailure as error:
                failure = failure or error
            except Exception:
                failure = failure or CheckFailure("process_cleanup")
    if failure:
        raise failure
    announce("PASS", "private_live_acceptance")


class PrivateArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        # Do not echo accidentally supplied secrets or paths in parser errors.
        self.exit(2, "FAIL command_arguments\n")


if __name__ == "__main__":
    parser = PrivateArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="Built server executable")
    parser.add_argument("--token-file", type=Path, required=True, help="Private token file outside this repository")
    arguments = parser.parse_args()
    try:
        run(arguments.binary, arguments.token_file)
    except CheckFailure as error:
        announce("FAIL", str(error))
        raise SystemExit(1) from None
    except Exception:
        announce("FAIL", "preflight")
        raise SystemExit(1) from None
