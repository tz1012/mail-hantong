#!/usr/bin/env python3
"""Validate Rust package boundaries from Cargo metadata without modifying the tree."""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGES = {
    "hantong-contracts", "hantong-platform", "hantong-domain", "hantong-policy", "hantong-approval", "hantong-approval-windows",
    "hantong-application", "hantong-outbox", "hantong-ipc-client", "hantong-ipc-server",
    "hantong-store", "hantong-secrets", "hantong-adapters", "hantong-cli", "mail-hantong-core",
    "hantong-desktop", "mail-hantong-gui",
}
TEST_SUPPORT = "hantong-test-support"
CLIENTS = {"hantong-cli", "mail-hantong-gui", "hantong-desktop"}
FORBIDDEN_CLIENT = {"hantong-store", "hantong-secrets", "hantong-adapters", "hantong-ipc-server", "hantong-outbox", "hantong-approval-windows"}
ALLOWED = {
    "hantong-contracts": set(),
    "hantong-platform": {"hantong-contracts"},
    "hantong-domain": set(),
    "hantong-policy": {"hantong-contracts", "hantong-domain", "hantong-approval"},
    "hantong-approval": {"hantong-contracts", "hantong-domain"},
    "hantong-approval-windows": {"hantong-contracts", "hantong-domain", "hantong-approval", "hantong-platform", "hantong-secrets"},
    "hantong-application": {"hantong-contracts", "hantong-domain", "hantong-policy", "hantong-approval", "hantong-outbox"},
    "hantong-outbox": {"hantong-contracts", "hantong-domain"},
    "hantong-ipc-client": {"hantong-contracts", "hantong-platform"},
    "hantong-ipc-server": {"hantong-contracts", "hantong-platform", "hantong-application"},
    "hantong-store": {"hantong-contracts", "hantong-domain", "hantong-secrets", "hantong-platform"},
    "hantong-secrets": {"hantong-contracts", "hantong-domain"},
    "hantong-adapters": {"hantong-domain"},
    "hantong-cli": {"hantong-contracts", "hantong-ipc-client"},
    "hantong-desktop": {"hantong-contracts"},
    "mail-hantong-gui": {"hantong-contracts", "hantong-ipc-client", "hantong-desktop"},
    "mail-hantong-core": PACKAGES - {"mail-hantong-core", "mail-hantong-gui", "hantong-desktop"},
}


def cargo_metadata(root: Path = ROOT) -> dict:
    completed = subprocess.run(
        ["cargo", "metadata", "--offline", "--locked", "--format-version", "1", "--no-deps"],
        cwd=root, capture_output=True, text=True, check=False,
    )
    if completed.returncode:
        raise RuntimeError(completed.stderr.strip() or f"cargo metadata exited {completed.returncode}")
    return json.loads(completed.stdout)


def validate_metadata(data: dict) -> list[str]:
    errors = []
    packages = {p.get("name"): p for p in data.get("packages", [])}
    members = data.get("workspace_members", [])
    member_names = {p.get("name") for p in data.get("packages", []) if p.get("id") in members}
    allowed_members = PACKAGES | {TEST_SUPPORT}
    for name in sorted(member_names - allowed_members):
        errors.append(f"unknown workspace package: {name}")
    for name in sorted(PACKAGES - member_names):
        errors.append(f"missing workspace package: {name}")
    graph = {name: set() for name in member_names}
    for name in sorted(member_names & PACKAGES):
        for dep in packages[name].get("dependencies", []):
            target = dep.get("package") or dep.get("name")
            kind = dep.get("kind") or "normal"
            if target == TEST_SUPPORT:
                if kind != "dev":
                    errors.append(f"test-support dependency must be dev-only: {name} -> {target}")
                continue
            if target not in PACKAGES:
                if target in member_names or dep.get("source") is None:
                    errors.append(f"unmapped local dependency: {name} -> {target}")
                continue
            if kind != "normal":
                errors.append(f"dev-dependency is not allowed in product graph: {name} -> {target} ({kind})")
                continue
            graph[name].add(target)
            if target not in ALLOWED.get(name, set()):
                errors.append(f"forbidden dependency: {name} -> {target}")
    for name in sorted(CLIENTS & member_names):
        stack = [(name, [])]
        visited = set()
        while stack:
            current, path = stack.pop()
            if current in visited:
                continue
            visited.add(current)
            for target in sorted(graph.get(current, set())):
                next_path = path + [target]
                if target in FORBIDDEN_CLIENT:
                    errors.append(f"transitive client dependency: {name} -> {' -> '.join(next_path)}")
                stack.append((target, next_path))
    return sorted(set(errors))


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    try:
        errors = validate_metadata(cargo_metadata(args.root.resolve()))
        result = {"errors": errors, "valid": not errors}
    except (OSError, RuntimeError, json.JSONDecodeError) as exc:
        result = {"errors": [f"metadata check failed: {exc}"], "valid": False}
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0 if result["valid"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
