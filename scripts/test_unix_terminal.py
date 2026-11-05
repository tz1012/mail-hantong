#!/usr/bin/env python3
"""Exercise only synthetic TUI data in a newly owned Unix pseudo-terminal."""
from __future__ import annotations

import argparse
import copy
import fcntl
import json
import os
from pathlib import Path
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time


def mode_difference(original: list, current: list) -> dict:
    """Only synthetic terminal state, never input bytes or environment paths."""
    names = ("iflag", "oflag", "cflag", "lflag", "ispeed", "ospeed", "cc")
    return {
        "changed_fields": [name for index, name in enumerate(names)
                           if original[index] != current[index]],
        "flag_xor": {name: original[index] ^ current[index]
                     for index, name in enumerate(names[:4])
                     if original[index] != current[index]},
        "cc_indices": [index for index, (before, after)
                       in enumerate(zip(original[6], current[6])) if before != after],
    }


def assert_mode_restored(slave: int, original: list, operation: str) -> None:
    current = termios.tcgetattr(slave)
    assert current == original, json.dumps({
        "synthetic_terminal": True, "operation": operation,
        **mode_difference(original, current),
    }, sort_keys=True)


def report_native_mode_roundtrip() -> None:
    """Report the OS's own raw-to-canonical transition before testing the app."""
    master, slave = os.openpty()
    original = termios.tcgetattr(slave)
    try:
        mode = copy.deepcopy(original)
        mode[0] &= ~termios.IXON
        mode[3] &= ~(termios.ICANON | termios.ECHO | termios.ISIG | termios.IEXTEN)
        mode[6][termios.VMIN] = 0
        mode[6][termios.VTIME] = 1
        termios.tcsetattr(slave, termios.TCSANOW, mode)
        termios.tcsetattr(slave, termios.TCSANOW, original)
        print(json.dumps({"synthetic_native_roundtrip": True,
                          **mode_difference(original, termios.tcgetattr(slave))},
                         sort_keys=True), flush=True)
    finally:
        os.close(master)
        os.close(slave)


def wait_for_exit(child: subprocess.Popen[bytes], master: int, timeout: float = 8) -> bytes:
    """Drain the PTY while waiting so a verbose TUI cannot block on output."""
    deadline = time.monotonic() + timeout
    output = bytearray()
    while time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            output.extend(os.read(master, 65536))
        elif child.poll() is not None:
            child.wait(timeout=1)
            return bytes(output)
    raise AssertionError("synthetic terminal child did not exit with drained output")


def exercise(program: Path, exit_key: bytes) -> None:
    master, slave = os.openpty()
    original = termios.tcgetattr(slave)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
    child = None
    with tempfile.TemporaryDirectory(prefix="hantong-demo-") as home:
        env = dict(os.environ, HOME=home, XDG_CONFIG_HOME=home, XDG_DATA_HOME=home,
                   TERM="xterm-256color")
        try:
            child = subprocess.Popen([str(program), "--demo", "--theme", "bbs8"],
                                     stdin=slave, stdout=slave, stderr=slave,
                                     env=env, start_new_session=True)

            def read_until(expected: bytes, timeout: float = 8) -> bytes:
                output = bytearray()
                deadline = time.monotonic() + timeout
                while time.monotonic() < deadline:
                    if select.select([master], [], [], 0.1)[0]:
                        output.extend(os.read(master, 65536))
                        if expected in output:
                            return bytes(output)
                    if child.poll() is not None:
                        break
                raise AssertionError(f"synthetic TUI did not display {expected!r}")

            first = read_until(b"> Welcome to HANTONG / terminal letter 01")
            assert b"\x1b[?1049h" in first and b"\x1b[?1006h" in first
            os.write(master, b"\x1b[B")
            read_until(b"> Welcome to HANTONG / terminal letter 02")
            os.write(master, b"\r")
            read_until(b"BODY")
            os.write(master, b"\x1b")
            read_until(b"MESSAGES")
            # Terminal resize is detected without borrowing the user's terminal.
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 100, 0, 0))
            read_until(b"\x1b[2J")
            os.write(master, exit_key)
            wait_for_exit(child, master)
            assert child.returncode == 0, child.returncode
            assert_mode_restored(slave, original, "quit" if exit_key == b"q" else "ctrl_c")
        finally:
            if child is not None and child.poll() is None:
                child.kill()
                child.wait(timeout=5)
            os.close(master)
            os.close(slave)


def exercise_output_burst() -> None:
    """Prove shutdown drains output larger than a typical PTY buffer."""
    master, slave = os.openpty()
    child = subprocess.Popen(
        [sys.executable, "-c", "import sys; sys.stdout.write('x' * 524288); sys.stdout.flush()"],
        stdin=subprocess.DEVNULL, stdout=slave, stderr=slave, start_new_session=True,
    )
    try:
        output = wait_for_exit(child, master)
        assert len(output) >= 524288
        assert child.returncode == 0
    finally:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=5)
        os.close(master)
        os.close(slave)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("program", type=Path)
    parser.add_argument("--input-probe", type=Path)
    args = parser.parse_args()
    program = args.program.resolve(strict=True)
    report_native_mode_roundtrip()
    for key in (b"q", b"\x03"):
        exercise(program, key)
    exercise_output_burst()
    headless = subprocess.run([str(program), "--demo"], input=b"", capture_output=True,
                              timeout=8, check=False)
    assert headless.returncode != 0
    assert b"\x1b[?1049h" not in headless.stdout
    if args.input_probe:
        probe = args.input_probe.resolve(strict=True)
        for mode, initial, keys, expected in (
            ("choice", b"Synthetic first", b"\x1b[B\r", b"PROBE_RESULT=second"),
            ("select", b"Synthetic first", b"\x1b[B\r", b"PROBE_RESULT=1"),
            ("select", b"Synthetic first", b"\x1b", b"PROBE_RESULT=cancelled"),
            ("prompt", b"Mail >", b"/", b"  /help"),
        ):
            master, slave = os.openpty()
            original = termios.tcgetattr(slave)
            child = subprocess.Popen([str(probe), mode], stdin=slave, stdout=slave, stderr=slave)
            output = bytearray()

            def wait_for(value: bytes) -> None:
                deadline = time.monotonic() + 8
                while value not in output and time.monotonic() < deadline:
                    if select.select([master], [], [], 0.1)[0]:
                        output.extend(os.read(master, 65536))
                assert value in output, (mode, value)

            try:
                wait_for(initial)
                os.write(master, keys)
                wait_for(expected)
                if mode == "prompt":
                    os.write(master, b"\x1b[B\r")
                    wait_for(b"PROBE_RESULT=/help")
                output.extend(wait_for_exit(child, master))
                assert child.returncode == 0
                assert b"\x1b[?1049h" not in output, "inline editor replaced the screen"
                assert_mode_restored(slave, original, "input_probe_" + mode)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=5)
                os.close(master)
                os.close(slave)
    print(json.dumps({"synthetic_only": True, "keyboard_open_back": True,
                      "resize": True, "quit_and_ctrl_c_restore": True,
                      "headless_rejected_before_effect": True,
                      "inline_input_checked": bool(args.input_probe)}))


if __name__ == "__main__":
    main()
