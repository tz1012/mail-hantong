#!/usr/bin/env python3
"""Run the repository's documented checks from any working directory."""
from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

from cargo_native import build_environment

ROOT = Path(__file__).resolve().parents[1]


def command_exists(name: str) -> bool:
    return shutil.which(name) is not None


def run_checks(root: Path = ROOT) -> int:
    if not command_exists("cargo"):
        print("error: cargo is required for the Rust workspace checks", file=sys.stderr)
        return 127

    python = sys.executable
    commands = [
        [python, "-X", "utf8", "-m", "unittest", "discover", "-s", "tests/Docs", "-q"],
        [python, "-X", "utf8", "-m", "unittest", "discover", "-s", "tests/Build", "-q"],
        [python, "-X", "utf8", "scripts/check_requirements.py"],
        [python, "-X", "utf8", "scripts/prepare_requirements.py", "--check"],
        [python, "-X", "utf8", "scripts/check_architecture.py"],
        [python, "-X", "utf8", "scripts/traceability.py", "--check"],
        ["cargo", "fmt", "--all", "--", "--check"],
        ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"],
        ["cargo", "build", "--workspace", "--all-targets", "--locked", "--offline"],
        ["cargo", "test", "--workspace", "--all-targets", "--locked", "--offline"],
    ]
    try:
        env = build_environment(root)
    except (OSError, ValueError, KeyError):
        print("error: verified native tools are required; run python scripts/prepare_native_tools.py", file=sys.stderr)
        return 2
    env["HANTONG_CHECK_ALL"] = "1"
    for command in commands:
        print("+", " ".join(command), flush=True)
        timeout = 1800 if command[:2] == ["cargo", "build"] else 600
        try:
            completed = subprocess.run(command, cwd=root, env=env, check=False, timeout=timeout)
        except subprocess.TimeoutExpired:
            print(f"error: check exceeded its {timeout}-second limit", file=sys.stderr)
            return 124
        except OSError:
            print("error: unable to start a required check", file=sys.stderr)
            return 127
        if completed.returncode:
            print(f"error: command failed with exit code {completed.returncode}", file=sys.stderr)
            return completed.returncode
    print("note: these checks cover the implemented boundaries only; full product security and release approval remain pending.")
    return 0


if __name__ == "__main__":
    raise SystemExit(run_checks())
