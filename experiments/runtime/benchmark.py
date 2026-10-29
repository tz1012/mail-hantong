"""Reproducible Windows-only runtime microbenchmark; no product/mail access."""

import argparse
from collections import defaultdict
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import random
import statistics
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parent


def summarize(values):
    if not values:
        raise ValueError("No samples")
    ordered = sorted(values)
    return {"samples": len(values), "median": statistics.median(values),
            "p95": ordered[math.ceil(len(values) * .95) - 1],
            "min": ordered[0], "max": ordered[-1]}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def generate_fixture(directory, count):
    directory.mkdir(parents=True, exist_ok=True)
    rows = []
    for identifier in range(1, count + 1):
        subject = ("invoice" if identifier % 7 == 0 else "회의" if identifier % 11 == 0 else "Status message")
        rows.append(f"{identifier}\t{(identifier - 1) % 5 + 1}\t{1700000000 + identifier // 3}\t{identifier % 2}\t{subject} {identifier:06d}\tuser{identifier % 101}@example.invalid\t프로젝트 소식 - quarterly update {identifier % 37}\n")
    random.Random(41029).shuffle(rows)
    cache = directory / "cache.tsv"
    cache.write_bytes("".join(rows).encode("utf-8"))
    line = "Synthetic cached body. 한통 메일 성능 실험. No real email.\n".encode("utf-8")
    size = 128 * 1024
    body = directory / "body.txt"
    body.write_bytes(line * (size // len(line)) + b"x" * (size % len(line)))
    return {"version": 1, "records": count, "accounts": 5, "seed": 41029,
            "cache_bytes": cache.stat().st_size, "body_bytes": body.stat().st_size,
            "cache_sha256": sha256(cache), "body_sha256": sha256(body)}


def make_oracle(directory):
    rows = [row.split("\t") for row in (directory / "cache.tsv").read_text(encoding="utf-8").splitlines()]
    rows.sort(key=lambda row: (int(row[2]), int(row[0])), reverse=True)
    body = (directory / "body.txt").read_bytes()
    memo = {}

    def expected(command):
        if command in memo:
            return memo[command]
        parts = command.split("\t")
        if parts[0] == "LIST":
            identifiers = [row[0] for row in rows if row[1] == parts[1]][:100]
            value = f"LIST\t{len(identifiers)}\t{','.join(identifiers)}"
        elif parts[0] == "SEARCH":
            identifiers = [int(row[0]) for row in rows if row[1] == parts[1] and (parts[2] in row[4] or parts[2] in row[6])]
            value = f"SEARCH\t{len(identifiers)}\t{sum(identifiers)}"
        elif parts[0] == "BODY":
            value = f"BODY\t{len(body)}\t{sum(body)}"
        elif command == "PING":
            value = "PONG"
        elif command == "QUIT":
            value = "BYE"
        else:
            raise ValueError("Unknown oracle command")
        memo[command] = value
        return value
    return expected


class Client:
    def __init__(self, executable, directory):
        self.started = time.perf_counter_ns()
        self.process = subprocess.Popen([str(executable), "--serve", str(directory / "cache.tsv"), str(directory / "body.txt")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)
        self.lines = queue.Queue()
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self):
        try:
            for line in self.process.stdout:
                self.lines.put(line.rstrip("\r\n"))
        finally:
            self.lines.put(None)

    def receive(self):
        try:
            response = self.lines.get(timeout=15)
        except queue.Empty as exc:
            raise RuntimeError("Prototype response timed out") from exc
        if response is None:
            raise RuntimeError("Prototype exited before response")
        return response

    def request(self, command):
        started = time.perf_counter_ns()
        self.process.stdin.write(command + "\n")
        self.process.stdin.flush()
        response = self.receive()
        return response, (time.perf_counter_ns() - started) / 1e6

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=10)
        self.reader.join(timeout=5)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            stream.close()


def process_metrics(process):
    class Memory(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("faults", wintypes.DWORD)] + [(name, ctypes.c_size_t) for name in ("peak_ws", "ws", "peak_paged", "paged", "peak_nonpaged", "nonpaged", "pagefile", "peak_pagefile", "private")]

    handle = wintypes.HANDLE(int(process._handle))
    memory = Memory()
    memory.cb = ctypes.sizeof(memory)
    get_memory = ctypes.WinDLL("psapi", use_last_error=True).GetProcessMemoryInfo
    get_memory.argtypes = [wintypes.HANDLE, ctypes.POINTER(Memory), wintypes.DWORD]
    get_memory.restype = wintypes.BOOL
    if not get_memory(handle, ctypes.byref(memory), memory.cb):
        raise ctypes.WinError(ctypes.get_last_error())
    times = [wintypes.FILETIME() for _ in range(4)]
    get_times = ctypes.WinDLL("kernel32", use_last_error=True).GetProcessTimes
    get_times.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    get_times.restype = wintypes.BOOL
    if not get_times(handle, *(ctypes.byref(value) for value in times)):
        raise ctypes.WinError(ctypes.get_last_error())
    cpu_ms = sum((value.dwHighDateTime << 32) + value.dwLowDateTime for value in times[2:]) / 10000
    return {"working_set_mib": memory.ws / 2**20, "peak_working_set_mib": memory.peak_ws / 2**20, "private_mib": memory.private / 2**20, "cpu_ms": cpu_ms}


def environment():
    command = "$p=Get-CimInstance Win32_Processor; $c=Get-CimInstance Win32_ComputerSystem; $o=Get-CimInstance Win32_OperatingSystem; [pscustomobject]@{cpu=$p.Name;cores=$p.NumberOfCores;logical_processors=$p.NumberOfLogicalProcessors;ram_bytes=$c.TotalPhysicalMemory;os=$o.Caption;build=$o.BuildNumber} | ConvertTo-Json -Compress"
    info = json.loads(subprocess.check_output(["powershell", "-NoProfile", "-Command", command], text=True, encoding="utf-8", timeout=30))
    info.update({"python": platform.python_version(), "platform": platform.platform(), "power_scheme": subprocess.check_output(["powercfg", "/getactivescheme"], text=True, errors="replace").strip(), "antivirus": "unchanged; exclusions not added", "file_cache": "OS cache warmed, not flushed; disk-cold startup not measured", "timing": "perf_counter_ns; host pipe round-trip includes Python/Windows scheduling", "cpu_resolution": "Windows GetProcessTimes; short requests may have zero CPU delta"})
    return info


def measure_candidate(executable, directory, count, expected, commands):
    samples = defaultdict(list)
    start = time.perf_counter_ns()
    ping = subprocess.run([str(executable), "--ping"], capture_output=True, text=True, encoding="utf-8", timeout=15)
    samples["launch_ping_exit_ms"].append((time.perf_counter_ns() - start) / 1e6)
    if (ping.returncode, ping.stdout.strip(), ping.stderr) != (0, "PONG", ""):
        raise RuntimeError("Ping output differs")
    client = Client(executable, directory)
    try:
        ready = client.receive()
        samples["load_ready_ms"].append((time.perf_counter_ns() - client.started) / 1e6)
        if ready != f"READY\t{count}":
            raise RuntimeError("Loaded count differs")
        before = process_metrics(client.process)
        for metric, value in before.items():
            samples["ready_" + metric].append(value)
        for label, command in commands:
            response, elapsed = client.request(command)
            if response != expected(command):
                raise RuntimeError(f"Response differs for {label}")
            samples[label + "_ms"].append(elapsed)
        after = process_metrics(client.process)
        for metric, value in after.items():
            samples["after_" + metric].append(value)
        samples["resident_commands_cpu_ms"].append(after["cpu_ms"] - before["cpu_ms"])
        if client.request("QUIT")[0] != "BYE":
            raise RuntimeError("Quit output differs")
        if client.process.wait(timeout=10) != 0 or client.process.stderr.read():
            raise RuntimeError("Prototype failed or wrote diagnostics")
    finally:
        client.close()
    return dict(samples)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rounds", type=int, default=30)
    parser.add_argument("--warmups", type=int, default=3)
    parser.add_argument("--records", type=int, default=100000)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--environment", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("Live memory instrumentation currently requires Windows")
    if args.rounds < 10 or args.warmups < 1 or args.records < 5:
        parser.error("Require rounds >= 10, warmups >= 1 and records >= 5")
    if args.output.exists():
        parser.error("Choose a new output path to preserve prior samples")
    programs = {"rust": ROOT / "rust/target/release/hantong-runtime-rust.exe", "dotnet_aot": ROOT / "dotnet/bin/publish/hantong-runtime-dotnet.exe"}
    for executable in programs.values():
        if not executable.is_file():
            parser.error(f"Missing executable: {executable}")
    directory = ROOT / "data"
    fixture = generate_fixture(directory, args.records)
    expected = make_oracle(directory)
    commands = []
    for account in range(1, 6):
        commands.extend([("list", f"LIST\t{account}"), ("search_invoice", f"SEARCH\t{account}\tinvoice"), ("search_korean", f"SEARCH\t{account}\t회의"), ("search_snippet", f"SEARCH\t{account}\tquarterly"), ("search_missing", f"SEARCH\t{account}\tNONEXISTENT_QUERY"), ("body", "BODY")])
    # Precompute all oracle responses before any measured request.
    for _, command in commands:
        expected(command)
    env = environment()
    env["build_details"] = json.loads(args.environment.read_text(encoding="utf-8-sig"))
    if not env["build_details"].get("dotnet_native_aot") or env["build_details"].get("rust_profile") != "release":
        parser.error("Build manifest must confirm .NET Native AOT and Rust release")
    raw = []
    warmups = []
    for round_index in range(-args.warmups, args.rounds):
        order = list(programs)
        if round_index % 2:
            order.reverse()
        offset = (round_index % 5) * 6
        rotated = commands[offset:] + commands[:offset]
        for name in order:
            samples = measure_candidate(programs[name], directory, args.records, expected, rotated)
            row = {"round": round_index, "runtime": name, "samples": samples}
            (warmups if round_index < 0 else raw).append(row)
        print(f"Round {round_index + 1}/{args.rounds}", flush=True)
    aggregate = {}
    for name in programs:
        metrics = defaultdict(list)
        for row in raw:
            if row["runtime"] == name:
                for metric, values in row["samples"].items():
                    metrics[metric].extend(values)
        aggregate[name] = {metric: summarize(values) for metric, values in metrics.items()}
    result = {"schema": 1, "measured_utc": datetime.now(timezone.utc).isoformat(), "environment": env, "fixture": fixture, "rounds": args.rounds, "warmups": args.warmups, "correctness": "every response checked against independent Python oracle", "artifacts": {name: {"path": str(path.relative_to(ROOT)), "bytes": path.stat().st_size, "sha256": sha256(path)} for name, path in programs.items()}, "summary": aggregate, "raw": raw, "warmup_samples": warmups}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(aggregate, indent=2))


if __name__ == "__main__":
    main()
