#!/usr/bin/env python3
"""Measure local ExoRoute request-path overhead against a deterministic mock."""

from __future__ import annotations

import argparse
import concurrent.futures
import http.client
import json
import os
import platform
import secrets
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


class MockProvider(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, _format: str, *_args: Any) -> None:
        pass

    def handle(self) -> None:
        try:
            super().handle()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def _respond(self, status: int, payload: dict[str, Any]) -> None:
        encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def do_GET(self) -> None:
        if self.path == "/v1/models":
            self._respond(200, {"data": [{"id": "bench-model", "object": "model"}]})
        else:
            self._respond(404, {"error": "not found"})

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        self.rfile.read(length)
        if self.path != "/v1/chat/completions":
            self._respond(404, {"error": "not found"})
            return
        self._respond(
            200,
            {
                "id": "chatcmpl-benchmark",
                "object": "chat.completion",
                "created": 1,
                "model": "bench-model",
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": "OK"},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            },
        )


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def request_json(
    port: int,
    path: str,
    method: str = "GET",
    body: dict[str, Any] | None = None,
    bearer: str | None = None,
) -> tuple[int, dict[str, Any]]:
    headers = {"Accept": "application/json"}
    encoded = None
    if body is not None:
        encoded = json.dumps(body, separators=(",", ":")).encode("utf-8")
        headers["Content-Type"] = "application/json"
    if bearer:
        headers["Authorization"] = f"Bearer {bearer}"
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    try:
        connection.request(method, path, body=encoded, headers=headers)
        response = connection.getresponse()
        raw = response.read()
        return response.status, json.loads(raw) if raw else {}
    finally:
        connection.close()


def admin_request(
    port: int,
    token: str,
    path: str,
    method: str = "GET",
    body: dict[str, Any] | None = None,
) -> dict[str, Any]:
    status, payload = request_json(port, path, method, body, token)
    if not 200 <= status < 300:
        raise RuntimeError(f"Admin setup failed ({status}) at {path}: {payload}")
    return payload


def wait_for_gateway(port: int, process: subprocess.Popen[bytes], log_path: Path) -> None:
    deadline = time.monotonic() + 30
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"ExoRoute exited during startup. See {log_path}")
        try:
            status, _ = request_json(port, "/health/ready")
            if status == 200:
                return
        except OSError as error:
            last_error = error
        time.sleep(0.1)
    raise RuntimeError(f"ExoRoute did not become ready: {last_error}; see {log_path}")


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, int((len(ordered) - 1) * fraction + 0.5)))
    return ordered[index]


def run_case(
    port: int,
    path: str,
    model: str,
    api_key: str | None,
    concurrency: int,
    requests: int,
) -> dict[str, Any]:
    body = json.dumps(
        {
            "model": model,
            "messages": [{"role": "user", "content": "Reply with OK."}],
            "max_tokens": 1,
            "stream": False,
        },
        separators=(",", ":"),
    )
    headers = {"Content-Type": "application/json", "Accept": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    counts = [requests // concurrency + (index < requests % concurrency) for index in range(concurrency)]
    start_barrier = threading.Barrier(concurrency + 1)

    def worker(count: int) -> tuple[list[float], int]:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        timings: list[float] = []
        errors = 0
        try:
            start_barrier.wait()
            for _ in range(count):
                started = time.perf_counter_ns()
                try:
                    connection.request("POST", path, body=body, headers=headers)
                    response = connection.getresponse()
                    payload = response.read()
                    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
                    if response.status == 200:
                        timings.append(elapsed_ms)
                    else:
                        errors += 1
                    if not payload:
                        errors += 1
                except (OSError, http.client.HTTPException):
                    errors += 1
                    connection.close()
                    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        finally:
            connection.close()
        return timings, errors

    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as executor:
        futures = [executor.submit(worker, count) for count in counts]
        start_barrier.wait()
        started = time.perf_counter()
        results = [future.result() for future in futures]
        elapsed_s = time.perf_counter() - started
    latencies = [latency for worker_timings, _ in results for latency in worker_timings]
    errors = sum(worker_errors for _, worker_errors in results)
    if not latencies:
        raise RuntimeError(f"No successful responses for {path}; errors={errors}")
    return {
        "requests": requests,
        "successes": len(latencies),
        "errors": errors,
        "elapsed_s": elapsed_s,
        "requests_per_second": len(latencies) / elapsed_s,
        "latency_ms": {
            "p50": percentile(latencies, 0.50),
            "p95": percentile(latencies, 0.95),
            "p99": percentile(latencies, 0.99),
            "mean": statistics.fmean(latencies),
        },
    }


def hardware_summary() -> dict[str, Any]:
    cpu = platform.processor() or os.environ.get("PROCESSOR_IDENTIFIER", "unknown")
    if os.name == "nt":
        try:
            import winreg

            with winreg.OpenKey(
                winreg.HKEY_LOCAL_MACHINE,
                r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            ) as key:
                cpu = str(winreg.QueryValueEx(key, "ProcessorNameString")[0]).strip()
        except OSError:
            pass
    return {
        "os": platform.platform(),
        "cpu": cpu,
        "logical_cpus": os.cpu_count(),
        "python": platform.python_version(),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--requests", type=int, default=800, help="Measured requests per endpoint and concurrency")
    parser.add_argument("--repeats", type=int, default=2, help="Complete measurement repeats")
    parser.add_argument("--concurrency", default="1,8", help="Comma-separated concurrency levels (max 32)")
    parser.add_argument("--binary", type=Path, help="Path to the release binary")
    parser.add_argument("--output", type=Path, help="Also write the full JSON report to this file")
    args = parser.parse_args()
    if args.requests < 100 or args.repeats < 1:
        parser.error("--requests must be at least 100 and --repeats must be positive")
    concurrency_levels = [int(value) for value in args.concurrency.split(",")]
    if not concurrency_levels or any(value < 1 or value > 32 for value in concurrency_levels):
        parser.error("concurrency values must be between 1 and 32")
    binary = args.binary or ROOT / "target" / "release" / ("exoroute.exe" if os.name == "nt" else "exoroute")
    if not binary.is_file():
        raise SystemExit(f"Release executable not found: {binary}. Run `cargo build --release` first.")

    mock_server = ThreadingHTTPServer(("127.0.0.1", 0), MockProvider)
    mock_server.daemon_threads = True
    mock_thread = threading.Thread(target=mock_server.serve_forever, daemon=True)
    mock_thread.start()
    provider_port = int(mock_server.server_address[1])
    gateway_port = free_port()
    temp_home = Path(tempfile.mkdtemp(prefix="exoroute-benchmark-"))
    temp_data = temp_home / "data"
    temp_data.mkdir()
    log_path = temp_home / "exoroute.log"
    admin_password = f"ER-{secrets.token_urlsafe(32)}-x9!"
    gateway_process: subprocess.Popen[bytes] | None = None
    log_file = None
    try:
        env = os.environ.copy()
        if os.name == "nt":
            env["USERPROFILE"] = str(temp_home)
        else:
            env["HOME"] = str(temp_home)
        env.update(
            {
                "EXOROUTE_HOST": "127.0.0.1",
                "EXOROUTE_PORT": str(gateway_port),
                "EXOROUTE_DATA_DIR": str(temp_data),
                "EXOROUTE_DATABASE_PATH": str(temp_data / "exoroute.lmdb"),
                "EXOROUTE_ADMIN_KEY": admin_password,
                "EXOROUTE_MASTER_KEY": __import__("base64").b64encode(secrets.token_bytes(32)).decode(),
                "EXOROUTE_ALLOW_PRIVATE_PROVIDER_URLS": "true",
                "CONNECT_TIMEOUT_MS": "1000",
                "REQUEST_TIMEOUT_MS": "10000",
                "RUST_LOG": "error",
            }
        )
        log_file = log_path.open("wb")
        creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0) if os.name == "nt" else 0
        gateway_process = subprocess.Popen(
            [str(binary.resolve())],
            cwd=ROOT,
            env=env,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            creationflags=creationflags,
        )
        wait_for_gateway(gateway_port, gateway_process, log_path)

        status, login = request_json(
            gateway_port,
            "/api/v1/admin/auth/login",
            "POST",
            {"password": admin_password},
        )
        if status != 200 or login.get("must_change_password"):
            raise RuntimeError(f"Benchmark admin login failed ({status}): {login}")
        admin_token = login["access_token"]
        admin_request(
            gateway_port,
            admin_token,
            "/api/v1/admin/providers",
            "POST",
            {
                "id": "bench-provider",
                "name": "Local benchmark mock",
                "base_url": f"http://127.0.0.1:{provider_port}/v1",
                "enabled": True,
                "auth_type": "none",
                "auth_header": None,
                "api_keys": [],
                "preferred_protocol": "chat_completions",
                "supported_protocols": ["chat_completions"],
            },
        )
        admin_request(
            gateway_port,
            admin_token,
            "/api/v1/admin/providers/bench-provider/models/import",
            "POST",
        )
        admin_request(
            gateway_port,
            admin_token,
            "/api/v1/admin/routes",
            "POST",
            {
                "id": "bench-route",
                "name": "Local benchmark route",
                "strategy": "priority",
                "accepted_protocols": ["chat_completions"],
                "enabled": True,
                "targets": [
                    {
                        "provider_id": "bench-provider",
                        "model": "bench-model",
                        "priority": 0,
                        "enabled": True,
                    }
                ],
            },
        )
        client_key = admin_request(
            gateway_port,
            admin_token,
            "/api/v1/admin/api-keys",
            "POST",
            {"name": "Local benchmark client"},
        )["key"]

        warmups = 40
        for _ in range(warmups):
            for port, path, model, key in (
                (provider_port, "/v1/chat/completions", "bench-model", None),
                (gateway_port, "/v1/chat/completions", "bench-route", client_key),
            ):
                result = run_case(port, path, model, key, 1, 1)
                if result["errors"]:
                    raise RuntimeError("Warmup request failed")

        samples: list[dict[str, Any]] = []
        for repeat in range(args.repeats):
            for index, concurrency in enumerate(concurrency_levels):
                cases = ["direct", "exoroute"]
                if (repeat + index) % 2:
                    cases.reverse()
                for case in cases:
                    if case == "direct":
                        port, path, model, key = provider_port, "/v1/chat/completions", "bench-model", None
                    else:
                        port, path, model, key = gateway_port, "/v1/chat/completions", "bench-route", client_key
                    result = run_case(port, path, model, key, concurrency, args.requests)
                    if result["errors"] or result["successes"] != args.requests:
                        raise RuntimeError(f"Benchmark returned errors: {case} concurrency={concurrency}: {result}")
                    samples.append(
                        {
                            "repeat": repeat + 1,
                            "concurrency": concurrency,
                            "path": case,
                            **result,
                        }
                    )

        report = json.dumps(
            {
                "machine": hardware_summary(),
                "binary": str(binary.resolve().relative_to(ROOT.resolve()))
                if binary.resolve().is_relative_to(ROOT.resolve())
                else str(binary.resolve()),
                "binary_bytes": binary.stat().st_size,
                "binary_mib": round(binary.stat().st_size / (1024 * 1024), 2),
                "build": "release",
                "upstream": "deterministic local mock; no external AI calls",
                "payload": "OpenAI Chat Completions; max_tokens=1; non-streaming",
                "warmup_requests_per_endpoint": warmups,
                "samples": samples,
            },
            indent=2,
        )
        if args.output:
            output_path = args.output if args.output.is_absolute() else ROOT / args.output
            output_path.parent.mkdir(parents=True, exist_ok=True)
            output_path.write_text(report + "\n", encoding="utf-8")
        print(report)
    finally:
        if gateway_process is not None:
            gateway_process.terminate()
            try:
                gateway_process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                gateway_process.kill()
                gateway_process.wait(timeout=5)
        if log_file is not None:
            log_file.close()
        mock_server.shutdown()
        mock_server.server_close()
        shutil.rmtree(temp_home, ignore_errors=True)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit("Benchmark interrupted")
