"""Bounded, read-only macOS process diagnostics for compatibility CI.

Only fixed category tags are emitted.  No command lines, paths, sample data,
or process output is returned.
"""
from __future__ import annotations

import ctypes
import os
from pathlib import Path
import subprocess
import tempfile
import re


PHASES = {"old", "candidate", "seed", "upgrade", "reopen"}
SAMPLE_SYMBOLS = {
    "keychain": r"\b(?:SecKeychain[A-Za-z0-9_]*|SecItemCopyMatching)\b",
    "security": r"\b(?:Security::[A-Za-z0-9_:]+|securityd_client[A-Za-z0-9_]*)\b",
    "xpc": r"\b(?:xpc_connection_send_message_with_reply_sync|_xpc_connection_wait_for_reply)\b",
    "socket": r"\b(?:connect|recv|recvfrom|read|poll|select|kevent)(?:\$[A-Za-z0-9_]+)?\b",
    "wait": r"\b(?:mach_msg[0-9_a-z]*|__psynch_mutexwait|__ulock_wait[0-9_a-z]*|wait4)\b",
}


def _classify_sample(data: bytes) -> str:
    # Classify stack symbols only, never process headers or arbitrary paths.
    lines = [line for line in data.decode("utf-8", errors="replace").splitlines()
             if re.match(r"^\s*[+!:| ]*\d+\s+", line) and "(in " in line]
    return ",".join(tag for tag, pattern in SAMPLE_SYMBOLS.items()
                    if any(re.search(pattern, line) for line in lines)) or "unknown"


def _allowed_image(image: str, allowed_images: dict[str, Path], test_target: Path | None) -> bool:
    candidate = Path(image)
    if not candidate.is_absolute():
        return False
    try:
        resolved = candidate.resolve(strict=True)
    except OSError:
        return False
    if test_target is not None and resolved.name.startswith("hantong_secrets-"):
        try:
            resolved.relative_to(test_target.resolve(strict=True))
            return True
        except (OSError, ValueError):
            return False
    return any(resolved == expected.resolve(strict=True) for expected in allowed_images.values())


def _is_test_image(image: str, test_target: Path | None) -> bool:
    if test_target is None or not Path(image).is_absolute():
        return False
    try:
        resolved = Path(image).resolve(strict=True)
        resolved.relative_to(test_target.resolve(strict=True))
        return resolved.name.startswith("hantong_secrets-")
    except (OSError, ValueError):
        return False


def _image_path(pid: int) -> str:
    # CI-only best-effort kernel path query. Never use argv[0] as fallback.
    try:
        library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        query = library.proc_pidpath
        query.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
        query.restype = ctypes.c_int
        buffer = ctypes.create_string_buffer(4096)
        count = query(pid, buffer, len(buffer))
        if count <= 0 or count >= len(buffer):
            return ""
        value = buffer.value.decode("utf-8", errors="strict")
        return value if Path(value).is_absolute() else ""
    except (OSError, AttributeError, UnicodeError):
        return ""


def _snapshot() -> dict[int, tuple[int, str, str]]:
    result = subprocess.run(
        ["/bin/ps", "-ww", "-axo", "pid=,ppid=,lstart=,comm="],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        timeout=3, check=True,
    )
    rows: dict[int, tuple[int, str, str]] = {}
    for line in result.stdout.splitlines():
        fields = line.strip().split(None, 7)
        if len(fields) != 8:
            continue
        try:
            pid, ppid = int(fields[0]), int(fields[1])
        except ValueError:
            continue
        rows[pid] = (ppid, " ".join(fields[2:7]), _image_path(pid))
    return rows


def diagnose_process_tree(
    root_pid: int,
    allowed_images: dict[str, Path],
    *,
    phase: str,
    test_target: Path | None = None,
) -> None:
    """Sample an owned process tree and emit fixed diagnostic categories.

    The function refuses invalid/unowned image declarations and never prints
    process arguments or raw `/bin/ps`/`sample` output.
    """
    if phase not in PHASES:
        print("HANTONG_PROCESS_DIAG result=invalid-phase", flush=True)
        return
    if not isinstance(root_pid, int) or root_pid <= 0:
        print(f"HANTONG_PROCESS_DIAG phase={phase} result=invalid-root", flush=True)
        return
    if not phase or any(not isinstance(path, Path) or not path.is_absolute() or path.is_symlink() for path in allowed_images.values()):
        print(f"HANTONG_PROCESS_DIAG phase={phase} result=invalid-allowlist", flush=True)
        return
    try:
        rows = _snapshot()
        if root_pid not in rows:
            print(f"HANTONG_PROCESS_DIAG phase={phase} result=root-missing", flush=True)
            return
        descendants = {root_pid}
        changed = True
        while changed:
            changed = False
            for pid, (ppid, _, _) in rows.items():
                if ppid in descendants and pid not in descendants:
                    descendants.add(pid)
                    changed = True
        if not rows[root_pid][2]:
            print(f"HANTONG_PROCESS_DIAG phase={phase} result=image-path-unavailable", flush=True)
            return
        if not _allowed_image(rows[root_pid][2], allowed_images, None):
            print(f"HANTONG_PROCESS_DIAG phase={phase} result=image-mismatch", flush=True)
            return
        if test_target is not None:
            relevant = sorted(pid for pid in descendants if _is_test_image(rows[pid][2], test_target))
        else:
            relevant = sorted(pid for pid in descendants if _allowed_image(rows[pid][2], allowed_images, None))
        if not relevant:
            print(f"HANTONG_PROCESS_DIAG phase={phase} result=test-missing", flush=True)
            return
        if len(relevant) > 4:
            print(f"HANTONG_PROCESS_DIAG phase={phase} result=tree-limit", flush=True)
            return
        sample = Path("/usr/bin/sample")
        for index, pid in enumerate(relevant):
            before = _snapshot()
            if before.get(root_pid) != rows[root_pid] or before.get(pid) != rows[pid]:
                print(f"HANTONG_PROCESS_DIAG phase={phase} node={index} result=identity-changed", flush=True)
                continue
            if not sample.is_file():
                print(f"HANTONG_PROCESS_DIAG phase={phase} node={index} result=sample-unavailable", flush=True)
                continue
            with tempfile.NamedTemporaryFile(prefix="hantong-sample-", dir=os.environ.get("RUNNER_TEMP"), mode="w+b") as output:
                sampled = subprocess.run([str(sample), str(pid), "1"], stdout=output, stderr=subprocess.DEVNULL, timeout=3, check=False)
                output.flush()
                output.seek(0)
                category = _classify_sample(output.read(1024 * 1024))
            after = _snapshot()
            if after.get(root_pid) != rows[root_pid] or after.get(pid) != rows[pid]:
                print(f"HANTONG_PROCESS_DIAG phase={phase} node={index} result=identity-changed", flush=True)
                continue
            result = "sampled" if sampled.returncode == 0 else "sample-failed"
            role = "test" if test_target is not None else next(key for key, path in allowed_images.items() if Path(rows[pid][2]).resolve() == path.resolve())
            # role is selected only from the caller's finite declaration.
            role = role if role in {"test", "cli", "core", "cargo"} else "owned"
            print(f"HANTONG_PROCESS_DIAG phase={phase} node={index} role={role} result={result} wait={category}", flush=True)
    except (OSError, subprocess.SubprocessError):
        print(f"HANTONG_PROCESS_DIAG phase={phase} result=probe-error", flush=True)
