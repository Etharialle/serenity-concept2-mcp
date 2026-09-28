#!/usr/bin/env python3
"""Exercise a built server over stdio without a Concept2 account or API request."""

import argparse
import json
import os
from pathlib import Path
import queue
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


def smoke(binary: Path, expected_version: str | None = None) -> None:
    binary = binary.resolve(strict=True)
    version = subprocess.run(
        [str(binary), "--version"], check=True, capture_output=True, text=True, timeout=10
    )
    if "serenity-concept2-mcp" not in version.stdout:
        raise RuntimeError("The executable returned an unexpected version banner")
    if expected_version and version.stdout.strip() != f"serenity-concept2-mcp {expected_version}":
        raise RuntimeError("The executable version does not match the package version")

    env = os.environ.copy()
    env["CONCEPT2_ACCESS_TOKEN"] = "synthetic-smoke-token-never-valid"
    # All smoke calls must be rejected or answered locally. A dead proxy also
    # prevents accidental API access through reqwest's environment proxy support.
    for name in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
        env[name] = "http://127.0.0.1:9"
    env["NO_PROXY"] = ""
    env["no_proxy"] = ""
    process = subprocess.Popen(
        [str(binary)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        env=env,
    )
    messages: queue.Queue = queue.Queue()
    errors = []

    def read_stdout():
        try:
            for line in process.stdout:
                messages.put(json.loads(line))
        except Exception as error:
            messages.put(error)
        finally:
            messages.put(EOFError("Server stdout closed"))

    def read_stderr():
        for line in process.stderr:
            errors.append(line)

    stdout_thread = threading.Thread(target=read_stdout, daemon=True)
    stderr_thread = threading.Thread(target=read_stderr, daemon=True)
    stdout_thread.start()
    stderr_thread.start()

    def send(message):
        process.stdin.write(json.dumps(message) + "\n")
        process.stdin.flush()

    def receive(request_id):
        # Notifications cannot extend the deadline for the requested reply.
        deadline = time.monotonic() + 5
        for _ in range(20):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("MCP reply timed out")
            message = messages.get(timeout=remaining)
            if isinstance(message, Exception):
                raise message
            if message.get("id") == request_id:
                return message
        raise RuntimeError("Too many unsolicited protocol messages")

    try:
        send({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "serenity-package-smoke", "version": "0.1.0"},
            },
        })
        initialized = receive(1)
        if "result" not in initialized:
            raise RuntimeError(f"MCP initialization failed: {initialized}")
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
        listed = receive(2)
        found = listed.get("result", {}).get("tools", [])
        if {tool["name"] for tool in found} != EXPECTED_TOOLS:
            raise RuntimeError("The packaged server did not advertise the five expected tools")
        for tool in found:
            if not tool.get("annotations", {}).get("readOnlyHint"):
                raise RuntimeError(f"Missing read-only annotation: {tool['name']}")
        send({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {
                "name": "concept2_list_workouts",
                "arguments": {"page": 0},
            },
        })
        invalid = receive(3)
        if "error" not in invalid and not invalid.get("result", {}).get("isError"):
            raise RuntimeError("Invalid page 0 was accepted")
        encoded_error = json.dumps(invalid).lower()
        if "page" not in encoded_error:
            raise RuntimeError("Invalid input failed for a reason other than page validation")
        process.stdin.close()
        process.wait(timeout=10)
        stderr_thread.join(timeout=5)
        if process.returncode != 0:
            raise RuntimeError(f"Server exited with {process.returncode}")
        if env["CONCEPT2_ACCESS_TOKEN"] in "".join(errors):
            raise RuntimeError("The server leaked a token to stderr")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        stdout_thread.join(timeout=5)
        stderr_thread.join(timeout=5)
        for stream in (process.stdout, process.stderr):
            stream.close()
    print(f"Package smoke passed: {version.stdout.strip()}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    smoke(parser.parse_args().binary)
