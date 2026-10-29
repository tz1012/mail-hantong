#!/usr/bin/env python3
"""Run Cargo with verified workspace-local Windows native build tools."""
from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys

import prepare_native_tools

ROOT = Path(__file__).resolve().parents[1]


def build_environment(root=ROOT, base=None, platform=None):
    env = dict(os.environ if base is None else base)
    if (os.name if platform is None else platform) != "nt":
        return env
    specs = prepare_native_tools.load_manifest(Path(root) / "scripts/native-tools.json")
    paths = {}
    for name in ("nasm", "strawberry-perl"):
        spec = specs[name]
        paths[name] = prepare_native_tools.ready_tool(name,Path(root)/".tools/native",spec)
    env["PATH"] = os.pathsep.join([str(paths["nasm"].parent), str(paths["strawberry-perl"].parent), env.get("PATH", "")])
    env["OPENSSL_SRC_PERL"] = paths["strawberry-perl"].as_posix()
    env["OPENSSL_NO_VENDOR"] = "0"
    # Native Perl does not recognize the MSYS C.UTF8 locale spelling.
    for name in ("LC_ALL", "LC_CTYPE", "LANG"):
        env[name] = "C"
    return env


def main(argv=None):
    try:
        env = build_environment()
    except (OSError, ValueError, KeyError):
        print("error: verified native tools are required; run python scripts/prepare_native_tools.py", file=sys.stderr)
        return 2
    try:
        return subprocess.run(["cargo", *(sys.argv[1:] if argv is None else argv)], cwd=ROOT, env=env, check=False).returncode
    except OSError:
        print("error: unable to start cargo", file=sys.stderr)
        return 127


if __name__ == "__main__":
    raise SystemExit(main())
