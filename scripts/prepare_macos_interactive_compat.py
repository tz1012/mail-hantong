#!/usr/bin/env python3
"""Read-only preparation for a separate interactive synthetic Mac test.

This is not an executor or an authorization token. It never creates/selects/
unlocks a Keychain, launches a test/product, changes permissions, or cleans up.
The existing CI-only entry points retain their restrictions.
"""
from __future__ import annotations

import argparse
import configparser
import ctypes
from dataclasses import dataclass
import errno
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import sys

from traceability import source_hash

USER = "hantong-compat"
HOME = Path("/Users") / USER
OLD_COMMIT = "5d1e4734dfe1475c951f8d052079212e1437dc50"
OLD_SHA = {
    "macos-aarch64": "b43ee9bae6b078ad6f21c6731c08c25bd67e1b8383fea34fa83df54c55ae74be",
    "macos-x86_64": "dc46e62b04c229c7a1ba08587104eb3f274c15bae0b2d5d70b7513117f42bc7e",
}
PHASES = (
    "official-old-db-seed", "official-candidate-db-read", "official-candidate-db-restart",
    "source-old-imap-smtp-seed", "source-candidate-read-preserve-update",
    "source-candidate-reopen", "cancel-denial-timeout-observation", "verified-restore",
)


class IsolationError(RuntimeError):
    """Only finite codes reach the terminal; never include subprocess output."""


@dataclass(frozen=True)
class Session:
    system: str
    architecture: str
    uid: int
    euid: int
    groups: tuple[int, ...]
    admin_gid: int
    username: str
    home: Path
    console_uid: int
    tty_input: bool
    tty_output: bool
    remote: bool
    ci: bool
    gui_domain: bool


def require(condition: bool, code: str) -> None:
    if not condition:
        raise IsolationError(code)


def validate_session(session: Session) -> None:
    require(session.system == "Darwin", "platform")
    require(session.architecture in OLD_SHA, "architecture")
    require(session.uid >= 501 and session.euid == session.uid, "identity")
    require(session.admin_gid >= 0 and session.admin_gid not in session.groups, "admin")
    require(session.username == USER and session.home == HOME, "dedicated_user")
    require(session.console_uid == session.uid, "console")
    require(session.gui_domain, "gui_domain")
    require(session.tty_input and session.tty_output and not session.remote, "interactive")
    require(not session.ci, "separate_lane")


def readonly_environment() -> dict[str, str]:
    return {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(HOME), "LC_ALL": "C",
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull}


def command(argv: list[str]) -> str:
    # Fail closed even if a future caller accidentally supplies a modifying verb.
    allowed = (
        argv in [["/usr/bin/security", verb, "-d", "user"] for verb in ("default-keychain", "list-keychains")]
        or argv in [["/usr/bin/id", "-G", USER], ["/usr/bin/getconf", "DARWIN_USER_TEMP_DIR"]]
        or (len(argv) == 3 and argv[:2] == ["/bin/ls", "-lde"])
        or (len(argv) >= 7 and argv[:5] == ["/usr/bin/git", "--no-optional-locks", "-c", "core.fsmonitor=false", "-C"]
            and argv[6:] in [["rev-parse", "--show-toplevel"], ["rev-parse", "--verify", "HEAD"],
                             ["status", "--porcelain=v1", "--untracked-files=all", "--ignored=matching"]])
    )
    require(allowed, "command_denied")
    result = subprocess.run(argv, env=readonly_environment(), stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, timeout=10, check=True)
    require(len(result.stdout) <= 65536, "command_output")
    return result.stdout


def native_architecture() -> str:
    # Apple's documented read-only Rosetta query. ENOENT means native on Intel;
    # other errors do not silently become a native-architecture claim.
    require(platform.system() == "Darwin", "platform")
    library = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
    query = library.sysctlbyname
    query.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p, ctypes.c_size_t]
    query.restype = ctypes.c_int
    translated = ctypes.c_int(0)
    size = ctypes.c_size_t(ctypes.sizeof(translated))
    ctypes.set_errno(0)
    result = query(b"sysctl.proc_translated", ctypes.byref(translated), ctypes.byref(size), None, 0)
    if result == -1:
        require(ctypes.get_errno() == errno.ENOENT, "architecture")
    else:
        require(result == 0 and size.value == ctypes.sizeof(translated)
                and translated.value in {0, 1}, "architecture")
        require(translated.value == 0, "translated_process")
    machine = platform.machine()
    require(machine in {"arm64", "x86_64"}, "architecture")
    return "macos-aarch64" if machine == "arm64" else "macos-x86_64"


def collect_session() -> Session:
    require(platform.system() == "Darwin", "platform")
    import grp
    import pwd

    uid, euid = os.getuid(), os.geteuid()
    account = pwd.getpwuid(uid)
    require(uid >= 501 and euid == uid, "identity")
    require(account.pw_name == USER and Path(account.pw_dir) == HOME, "dedicated_user")
    # Directory-service membership also detects admin membership absent from a
    # stale supplementary-group list. Use the OS account, never HOME/USER env.
    membership = command(["/usr/bin/id", "-G", account.pw_name]).split()
    require(bool(membership) and all(x.isdecimal() for x in membership), "groups")
    groups = tuple(set(map(int, membership)) | set(os.getgroups()) | {os.getgid(), os.getegid()})
    architecture = native_architecture()
    gui = subprocess.run(["/bin/launchctl", "print", f"gui/{uid}"],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, env=readonly_environment(),
                         timeout=5, check=False).returncode == 0
    forbidden = ("CI", "GITHUB_ACTIONS", "RUNNER_TEMP", "GITHUB_WORKSPACE",
                 "HANTONG_TEST_KEYCHAIN", "HANTONG_COMPAT_ROOT", "HANTONG_COMPAT_PHASE",
                 "CARGO_TARGET_X86_64_APPLE_DARWIN_RUNNER", "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER")
    return Session("Darwin", architecture, uid, euid, groups, grp.getgrnam("admin").gr_gid,
                   account.pw_name, Path(account.pw_dir), os.stat("/dev/console").st_uid,
                   sys.stdin.isatty(), sys.stdout.isatty(),
                   any(os.environ.get(k) for k in ("SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY")),
                   any(k in os.environ for k in forbidden), gui)


def snapshot(info: os.stat_result) -> tuple[int, ...]:
    return (info.st_dev, info.st_ino, info.st_uid, info.st_gid, info.st_mode,
            info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def no_extended_acl(path: Path) -> None:
    # Reject rather than attempt to normalize or repair a private test path.
    first = command(["/bin/ls", "-lde", str(path)]).splitlines()
    require(bool(first) and bool(first[0].split()), "acl_unknown")
    require("+" not in first[0].split()[0], "extended_acl")


def private_path(path: Path, home: Path, uid: int, *, file: bool = False) -> os.stat_result:
    require(path.is_absolute() and home.is_absolute(), "path")
    require(".." not in path.parts and "." not in path.parts, "path")
    try:
        relative = path.relative_to(home)
    except ValueError:
        raise IsolationError("path") from None
    current = home
    for part in (None, *relative.parts):
        if part is not None:
            current /= part
        info = current.lstat()
        leaf_file = current == path and file
        require(not stat.S_ISLNK(info.st_mode), "symlink")
        require(stat.S_ISREG(info.st_mode) if leaf_file else stat.S_ISDIR(info.st_mode), "type")
        require(info.st_uid == uid, "owner")
        require(stat.S_IMODE(info.st_mode) == (0o600 if leaf_file else 0o700), "mode")
        if leaf_file:
            require(info.st_nlink == 1, "hardlink")
        no_extended_acl(current)
    return info


def archive_hash(path: Path, expected: str, home: Path, uid: int) -> str:
    before = private_path(path, home, uid, file=True)
    require(0 < before.st_size <= 2 * 1024**3, "archive_size")
    digest = hashlib.sha256()
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
    with os.fdopen(os.open(path, flags), "rb") as stream:
        require(snapshot(os.fstat(stream.fileno())) == snapshot(before), "replaced")
        remaining = before.st_size
        while remaining:
            block = stream.read(min(1024**2, remaining))
            require(bool(block), "replaced")
            digest.update(block)
            remaining -= len(block)
        require(not stream.read(1), "replaced")
        require(snapshot(os.fstat(stream.fileno())) == snapshot(before), "replaced")
    require(snapshot(path.lstat()) == snapshot(before), "replaced")
    require(digest.hexdigest() == expected, "archive_hash")
    return digest.hexdigest()


def absent(path: Path) -> None:
    try:
        path.lstat()
    except FileNotFoundError:
        return
    raise IsolationError("existing_product_state")


def _baseline(home: Path, uid: int) -> tuple[Path, tuple[Path, ...]]:
    def paths(verb: str) -> tuple[Path, ...]:
        output = command(["/usr/bin/security", verb, "-d", "user"])
        lines = [line.strip() for line in output.splitlines() if line.strip()]
        require(0 < len(lines) <= 16, "keychain_selection")
        values = []
        for line in lines:
            if line.startswith('"') and line.endswith('"'):
                line = line[1:-1]
            require(not any(c in line for c in '\\"\r\n\t'), "keychain_selection")
            path = Path(line)
            # Validate lexical containment before inspecting any Keychain path.
            private_path(path, home, uid, file=True)
            values.append(path)
        require(len(set(values)) == len(values), "keychain_selection")
        return tuple(values)
    default, listed = paths("default-keychain"), paths("list-keychains")
    require(len(default) == 1 and default[0] in listed, "keychain_selection")
    return default[0], listed


def identity(value: os.stat_result) -> list[int]:
    return list(snapshot(value)[:5])


def run_root(session: Session, run_id: str) -> Path:
    validate_session(session)
    require(re.fullmatch(r"[0-9a-f]{32}", run_id) is not None, "run_id")
    root = session.home / "HantongCompat" / run_id
    private_path(root, session.home, session.uid)
    return root


def root_entries(root: Path, session: Session, stage: str) -> None:
    require(stage in {"baseline", "prepared"}, "stage")
    expected = {} if stage == "baseline" else {
        "baseline.json": True, "synthetic.keychain-db": True,
        "old-package.tar.gz": True, "candidate-package.tar.gz": True,
        "old-source": False, "candidate-source": False,
    }
    entries = list(root.iterdir())
    require({p.name for p in entries} == set(expected), "root_entries")
    for path in entries:
        private_path(path, session.home, session.uid, file=expected[path.name])


def product_absent(session: Session, *, restored: bool = False) -> None:
    if not restored:
        absent(session.home / "Library/Application Support/mail-hantong")
    runtime = Path(command(["/usr/bin/getconf", "DARWIN_USER_TEMP_DIR"]).strip())
    require(runtime.is_absolute() and runtime.is_dir(), "runtime")
    require(runtime.stat().st_uid == session.uid and runtime.stat().st_mode & 0o077 == 0, "runtime")
    absent(runtime / "mail-hantong")


def capture_baseline(session: Session, run_id: str) -> dict:
    root = run_root(session, run_id)
    root_entries(root, session, "baseline")
    before = root.lstat()
    absent(root / "synthetic.keychain-db")
    absent(root / "baseline.json")
    product_absent(session)
    default, listed = _baseline(session.home, session.uid)
    keys = [{"path": str(path), "identity": identity(private_path(path, session.home, session.uid, file=True))}
            for path in listed]
    require(_baseline(session.home, session.uid) == (default, listed), "keychain_selection")
    require(snapshot(root.lstat()) == snapshot(before), "replaced")
    for item, path in zip(keys, listed):
        require(item["identity"] == identity(private_path(path, session.home, session.uid, file=True)), "replaced")
    root_entries(root, session, "baseline")
    product_absent(session)
    return {"schema": "hantong-interactive-mac-baseline-v1", "uid": session.uid,
            "root_identity": identity(root.lstat()), "run_id": run_id,
            "default": str(default), "search": list(map(str, listed)), "keys": keys,
            "operator_attestation": "disposable-no-real-data"}


def load_baseline(session: Session, run_id: str, root: Path) -> tuple[Path, tuple[Path, ...]]:
    path = root / "baseline.json"
    before = private_path(path, session.home, session.uid, file=True)
    require(0 < before.st_size <= 32768, "baseline_record")
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as stream:
        require(snapshot(os.fstat(stream.fileno())) == snapshot(before), "replaced")
        data = stream.read(32769)
        require(len(data) == before.st_size, "replaced")
        require(snapshot(os.fstat(stream.fileno())) == snapshot(before), "replaced")
    require(snapshot(path.lstat()) == snapshot(before), "replaced")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "baseline_record")
            result[key] = value
        return result
    try:
        record = json.loads(data, object_pairs_hook=unique)
        require(isinstance(record, dict) and set(record) == {
            "schema", "uid", "root_identity", "run_id", "default", "search", "keys", "operator_attestation"
        }, "baseline_record")
        require(record["schema"] == "hantong-interactive-mac-baseline-v1"
                and record["operator_attestation"] == "disposable-no-real-data"
                and record["uid"] == session.uid and record["run_id"] == run_id
                and record["root_identity"] == identity(root.lstat()), "baseline_binding")
        require(isinstance(record["search"], list) and 0 < len(record["search"]) <= 16
                and all(isinstance(p, str) for p in record["search"])
                and isinstance(record["default"], str), "baseline_record")
        default, listed = Path(record["default"]), tuple(map(Path, record["search"]))
        require(default in listed and len(set(listed)) == len(listed)
                and root / "synthetic.keychain-db" not in listed, "baseline_record")
        require(isinstance(record["keys"], list) and len(record["keys"]) == len(listed), "baseline_record")
        for item, keychain in zip(record["keys"], listed):
            require(isinstance(item, dict) and set(item) == {"path", "identity"}
                    and item["path"] == str(keychain), "baseline_record")
            require(item["identity"] == identity(private_path(keychain, session.home, session.uid, file=True)), "baseline_binding")
    except (TypeError, ValueError, KeyError, UnicodeError, RecursionError):
        raise IsolationError("baseline_record") from None
    return default, listed


def verify_restoration(session: Session, run_id: str) -> dict:
    root = run_root(session, run_id)
    expected = load_baseline(session, run_id, root)
    product_absent(session, restored=True)
    require(_baseline(session.home, session.uid) == expected, "restore_selection")
    require(load_baseline(session, run_id, root) == expected, "baseline_binding")
    return {"schema": "hantong-interactive-mac-restore-check-v1", "selection_restored": True,
            "executed": False, "cleanup_authorized": False, "process_shutdown_verified": False}


def source_tree(source: Path, uid: int) -> tuple[str, os.stat_result]:
    """Scan without following links before Git can read any repository metadata."""
    pending = [source]
    rows = []
    total = 0
    config = None
    while pending:
        directory = pending.pop()
        with os.scandir(directory) as entries:
            for entry in entries:
                path = directory / entry.name
                relative = path.relative_to(source).as_posix()
                require(relative not in {".gitmodules", ".git/commondir", ".git/gitdir", ".git/config.worktree",
                                         ".git/objects/info/alternates", ".git/objects/info/http-alternates"}
                        and not any(relative == p or relative.startswith(p + "/")
                                    for p in (".git/modules", ".git/worktrees", ".git/refs/replace")), "source_reference")
                value = entry.stat(follow_symlinks=False)
                require(value.st_uid == uid, "owner")
                require(value.st_mode & 0o7022 == 0, "mode")
                if stat.S_ISDIR(value.st_mode):
                    pending.append(path)
                else:
                    require(stat.S_ISREG(value.st_mode), "source_type")
                    require(value.st_nlink == 1, "hardlink")
                    require(value.st_size <= 1024**3, "source_size")
                    total += value.st_size
                    require(total <= 8 * 1024**3, "source_size")
                rows.append((relative, snapshot(value)))
                require(len(rows) <= 100000, "source_size")
                if relative == ".git/config":
                    require(stat.S_ISREG(value.st_mode), "source_type")
                    config = value
    require(config is not None, "git_config")
    digest = hashlib.sha256(json.dumps(sorted(rows), ensure_ascii=True).encode()).hexdigest()
    return digest, config


def validate_git_config(text: str) -> None:
    config = configparser.RawConfigParser(strict=True)
    try:
        config.read_string(text)
        require(not config.defaults() and "core" in config, "git_config")
        for section in config.sections():
            values = dict(config.items(section))
            if section == "core":
                booleans = {"filemode", "logallrefupdates", "ignorecase", "precomposeunicode", "symlinks",
                            "protecthfs", "protectntfs"}
                require(set(values) <= booleans | {"repositoryformatversion", "bare"}
                        and values.get("repositoryformatversion") == "0" and values.get("bare") == "false", "git_config")
                require(all(value in {"true", "false"} for key, value in values.items() if key in booleans), "git_config")
            elif section == 'remote "origin"':
                require(set(values) <= {"url", "fetch"}
                        and values.get("url") in {"https://github.com/tz1012/mail-hantong", "https://github.com/tz1012/mail-hantong.git"}
                        and values.get("fetch") == "+refs/heads/*:refs/remotes/origin/*", "git_config")
            elif re.fullmatch(r'branch "[A-Za-z0-9._/-]+"', section):
                require(set(values) == {"remote", "merge"} and values["remote"] == "origin"
                        and re.fullmatch(r"refs/heads/[A-Za-z0-9._/-]+", values["merge"]) is not None, "git_config")
            else:
                raise IsolationError("git_config")
    except configparser.Error:
        raise IsolationError("git_config") from None


def git_config_file(source: Path, before: os.stat_result) -> None:
    path = source / ".git/config"
    require(0 < before.st_size <= 65536, "git_config")
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as stream:
        require(snapshot(os.fstat(stream.fileno())) == snapshot(before), "replaced")
        data = stream.read(65537)
        require(len(data) == before.st_size and snapshot(os.fstat(stream.fileno())) == snapshot(before), "replaced")
    require(snapshot(path.lstat()) == snapshot(before), "replaced")
    try:
        validate_git_config(data.decode("utf-8"))
    except UnicodeError:
        raise IsolationError("git_config") from None


def source_revision(source: Path, commit: str, home: Path, uid: int) -> str:
    private_path(source, home, uid)
    private_path(source / ".git", home, uid)
    before, config = source_tree(source, uid)
    git_config_file(source, config)
    git = ["/usr/bin/git", "--no-optional-locks", "-c", "core.fsmonitor=false", "-C", str(source)]
    require(command([*git, "rev-parse", "--show-toplevel"]).strip() == str(source), "source_root")
    require(command([*git, "rev-parse", "--verify", "HEAD"]).strip() == commit, "source_commit")
    require(not command([*git, "status", "--porcelain=v1", "--untracked-files=all", "--ignored=matching"]).strip(), "source_dirty")
    require(source_tree(source, uid)[0] == before, "source_changed")
    return before


def prepare(session: Session, run_id: str, candidate_commit: str, candidate_sha: str) -> dict:
    root = run_root(session, run_id)
    root_entries(root, session, "prepared")
    require(re.fullmatch(r"[0-9a-f]{40}", candidate_commit) is not None, "candidate_commit")
    require(re.fullmatch(r"[0-9a-f]{64}", candidate_sha) is not None, "candidate_hash")
    home = session.home
    load_baseline(session, run_id, root)
    root_before = private_path(root, home, session.uid)
    keychain = root / "synthetic.keychain-db"
    key_before = private_path(keychain, home, session.uid, file=True)
    expected_baseline = (keychain, (keychain,))
    require(_baseline(home, session.uid) == expected_baseline, "keychain_selection")
    # Merely inspect metadata. Never open the product DB, keys, or socket.
    product_absent(session)
    old_binding = source_revision(root / "old-source", OLD_COMMIT, home, session.uid)
    candidate_binding = source_revision(root / "candidate-source", candidate_commit, home, session.uid)
    old_sha = archive_hash(root / "old-package.tar.gz", OLD_SHA[session.architecture], home, session.uid)
    candidate_sha = archive_hash(root / "candidate-package.tar.gz", candidate_sha, home, session.uid)
    candidate_source_hash = source_hash(root / "candidate-source")
    require(source_revision(root / "old-source", OLD_COMMIT, home, session.uid) == old_binding, "source_changed")
    require(source_revision(root / "candidate-source", candidate_commit, home, session.uid) == candidate_binding, "source_changed")
    load_baseline(session, run_id, root)
    require(_baseline(home, session.uid) == expected_baseline, "keychain_selection")
    require(snapshot(keychain.lstat()) == snapshot(key_before), "replaced")
    require(snapshot(root.lstat()) == snapshot(root_before), "replaced")
    root_entries(root, session, "prepared")
    product_absent(session)
    return {
        "schema": "hantong-interactive-mac-preparation-v1", "mode": "prepare-only",
        "executed": False, "execution_authorized": False, "architecture": session.architecture,
        "old_commit": OLD_COMMIT, "candidate_commit": candidate_commit,
        "candidate_source_hash": candidate_source_hash,
        "old_archive_sha256": old_sha, "candidate_archive_sha256": candidate_sha,
        "phases": list(PHASES), "result": "preflight-passed-execution-not-performed",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", choices=("baseline", "prepared", "restore-check"), required=True)
    parser.add_argument("--disposable-user-confirmed", action="store_true")
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--candidate-commit")
    parser.add_argument("--candidate-sha256")
    args = parser.parse_args()
    try:
        require(args.disposable_user_confirmed, "operator_attestation")
        current = collect_session()
        if args.stage == "baseline":
            result = capture_baseline(current, args.run_id)
        elif args.stage == "restore-check":
            result = verify_restoration(current, args.run_id)
        else:
            result = prepare(current, args.run_id, args.candidate_commit or "", args.candidate_sha256 or "")
    except IsolationError as error:
        print(f"HANTONG_INTERACTIVE_PREP result=denied reason={error}", file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("HANTONG_INTERACTIVE_PREP result=cancelled", file=sys.stderr)
        return 130
    except (OSError, KeyError, AttributeError, UnicodeError, RecursionError, subprocess.SubprocessError):
        print("HANTONG_INTERACTIVE_PREP result=denied reason=observation_failed", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
