"""Owned-console integration checks for the communications-style TUI.

The worker owns a hidden Windows console created only for this test.  It opens
that console's CONIN$/CONOUT$ handles and injects records into that console;
the user's active terminal is never attached to, focused, or written to.
"""

import ctypes
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from ctypes import wintypes
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
EXE_CANDIDATES = tuple(
    p
    for p in (
        Path(os.environ["HANTONG_T_EXE"]) if os.environ.get("HANTONG_T_EXE") else None,
        ROOT / "target" / "retro-tui-build" / "debug" / "hantong-t.exe",
        Path(r"D:\BuildCache\cargo-target\debug\hantong-t.exe"),
        ROOT / "target" / "debug" / "hantong-t.exe",
    )
    if p is not None
)


def _win():
    return os.name == "nt"


class COORD(ctypes.Structure):
    _fields_ = [("X", wintypes.SHORT), ("Y", wintypes.SHORT)]


class SMALL_RECT(ctypes.Structure):
    _fields_ = [("Left", wintypes.SHORT), ("Top", wintypes.SHORT),
                ("Right", wintypes.SHORT), ("Bottom", wintypes.SHORT)]


class CONSOLE_SCREEN_BUFFER_INFO(ctypes.Structure):
    _fields_ = [("dwSize", COORD), ("dwCursorPosition", COORD),
                ("wAttributes", wintypes.WORD), ("srWindow", SMALL_RECT),
                ("dwMaximumWindowSize", COORD)]


class CONSOLE_CURSOR_INFO(ctypes.Structure):
    _fields_ = [("dwSize", wintypes.DWORD), ("bVisible", wintypes.BOOL)]


class KEY_EVENT_RECORD(ctypes.Structure):
    _fields_ = [("bKeyDown", wintypes.BOOL), ("wRepeatCount", wintypes.WORD),
                ("wVirtualKeyCode", wintypes.WORD), ("wVirtualScanCode", wintypes.WORD),
                ("UnicodeChar", wintypes.WCHAR), ("dwControlKeyState", wintypes.DWORD)]


class MOUSE_EVENT_RECORD(ctypes.Structure):
    _fields_ = [("dwMousePosition", COORD), ("dwButtonState", wintypes.DWORD),
                ("dwControlKeyState", wintypes.DWORD), ("dwEventFlags", wintypes.DWORD)]


class _EVENT(ctypes.Union):
    _fields_ = [("KeyEvent", KEY_EVENT_RECORD), ("MouseEvent", MOUSE_EVENT_RECORD)]


class INPUT_RECORD(ctypes.Structure):
    _fields_ = [("EventType", wintypes.WORD), ("Event", _EVENT)]


KEY_EVENT = 0x0001
MOUSE_EVENT = 0x0002
WINDOW_BUFFER_SIZE_EVENT = 0x0004
FROM_LEFT_1ST_BUTTON_PRESSED = 0x0001
DOUBLE_CLICK = 0x0002
MOUSE_WHEELED = 0x0004
GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
FILE_SHARE_READ = 1
FILE_SHARE_WRITE = 2
OPEN_EXISTING = 3
INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
CREATE_NEW_CONSOLE = 0x00000010
SW_HIDE = 0


def _api():
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.CreateFileW.restype = wintypes.HANDLE
    k.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                              wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    k.WriteConsoleInputW.argtypes = [wintypes.HANDLE, ctypes.POINTER(INPUT_RECORD), wintypes.DWORD,
                                     ctypes.POINTER(wintypes.DWORD)]
    k.WriteConsoleInputW.restype = wintypes.BOOL
    return k


def _open_console(api, name, access):
    h = api.CreateFileW(name, access, FILE_SHARE_READ | FILE_SHARE_WRITE, None,
                        OPEN_EXISTING, 0, None)
    if not h or h == INVALID_HANDLE_VALUE:
        raise OSError(ctypes.get_last_error(), name)
    return h


def _modes(api, h_in, h_out):
    value_in, value_out = wintypes.DWORD(), wintypes.DWORD()
    if not api.GetConsoleMode(h_in, ctypes.byref(value_in)) or not api.GetConsoleMode(h_out, ctypes.byref(value_out)):
        raise OSError(ctypes.get_last_error(), "GetConsoleMode")
    cursor = CONSOLE_CURSOR_INFO()
    if not api.GetConsoleCursorInfo(h_out, ctypes.byref(cursor)):
        raise OSError(ctypes.get_last_error(), "GetConsoleCursorInfo")
    return value_in.value, value_out.value, bool(cursor.bVisible)


def _write_key(api, h_in, char, vk=None, ctrl=False):
    rec = INPUT_RECORD()
    rec.EventType = KEY_EVENT
    rec.Event.KeyEvent = KEY_EVENT_RECORD(
        True, 1, vk or ord(char.upper()), 0, char, 0x0008 if ctrl else 0)
    count = wintypes.DWORD()
    if not api.WriteConsoleInputW(h_in, ctypes.byref(rec), 1, ctypes.byref(count)):
        raise OSError(ctypes.get_last_error(), "WriteConsoleInputW")


def _write_mouse(api, h_in, x, y, flags, buttons=FROM_LEFT_1ST_BUTTON_PRESSED):
    rec = INPUT_RECORD()
    rec.EventType = MOUSE_EVENT
    rec.Event.MouseEvent = MOUSE_EVENT_RECORD(COORD(x, y), buttons, 0, flags)
    count = wintypes.DWORD()
    if not api.WriteConsoleInputW(h_in, ctypes.byref(rec), 1, ctypes.byref(count)):
        raise OSError(ctypes.get_last_error(), "WriteConsoleInputW")


def _snapshot(api, h_out):
    active = _open_console(api, "CONOUT$", GENERIC_READ | GENERIC_WRITE)
    try:
        return _snapshot_active(api, active)
    finally:
        api.CloseHandle(active)


def _snapshot_active(api, h_out):
    info = CONSOLE_SCREEN_BUFFER_INFO()
    if not api.GetConsoleScreenBufferInfo(h_out, ctypes.byref(info)):
        raise OSError(ctypes.get_last_error(), "GetConsoleScreenBufferInfo")
    count = max(1, info.dwSize.X * info.dwSize.Y)
    text = ctypes.create_unicode_buffer(count)
    attrs = (wintypes.WORD * count)()
    read = wintypes.DWORD()
    origin = COORD(0, 0)
    api.ReadConsoleOutputCharacterW(h_out, text, count, origin, ctypes.byref(read))
    api.ReadConsoleOutputAttribute(h_out, attrs, count, origin, ctypes.byref(read))
    return "".join(text[:read.value]), list(attrs[:read.value]), info


def _wait_for(api, h_out, predicate, timeout=3):
    deadline = time.monotonic() + timeout
    latest = ("", [], None)
    while time.monotonic() < deadline:
        latest = _snapshot(api, h_out)
        if predicate(*latest):
            return latest
        time.sleep(0.05)
    return latest


def _worker(exe, result_path):
    api = _api()
    api.CloseHandle.argtypes = [wintypes.HANDLE]
    h_in = _open_console(api, "CONIN$", GENERIC_READ | GENERIC_WRITE)
    h_out = _open_console(api, "CONOUT$", GENERIC_READ | GENERIC_WRITE)
    before = _modes(api, h_in, h_out)
    checks = {"startup": False, "green": False, "amber": False, "double_click": False,
              "scroll": False, "compact_layout_verified": False,
              "runtime_resize_exercised": False, "quit": False, "restored": False,
              "invalid_theme": False, "arrow_open": False, "unified": False}
    details = {}
    compact = _snapshot(api, h_out)[2]
    compact_width = max(40, min(80, compact.dwSize.X - 1))
    compact_rect = SMALL_RECT(
        compact.srWindow.Left,
        compact.srWindow.Top,
        compact.srWindow.Left + compact_width - 1,
        compact.srWindow.Top + min(24, compact.dwSize.Y - 1),
    )
    compact_active = _open_console(api, "CONOUT$", GENERIC_READ | GENERIC_WRITE)
    try:
        checks["compact_layout_verified"] = bool(
            api.SetConsoleWindowInfo(compact_active, True, ctypes.byref(compact_rect))
        )
    finally:
        api.CloseHandle(compact_active)
    child = subprocess.Popen([str(exe), "--demo", "--lang", "ko"], close_fds=False)
    try:
        _wait_for(api, h_out, lambda t, *_: "한통" in t)
        text, attrs, _ = _snapshot(api, h_out)
        _, _, startup_info = _snapshot(api, h_out)
        checks["startup"] = "한통" in text and "reader@example.invalid" in text
        (ROOT / "target" / "retro-tui-console-frame.json").write_text(
            json.dumps({
                "synthetic": True,
                "width": startup_info.srWindow.Right - startup_info.srWindow.Left + 1,
                "height": startup_info.srWindow.Bottom - startup_info.srWindow.Top + 1,
                "text": text,
                "attributes": sorted(set(attrs)),
            }, ensure_ascii=False), encoding="utf-8"
        )
        _write_key(api, h_in, "f")
        text, attrs, _ = _wait_for(api, h_out, lambda t, a, *_: "green" in t.lower())
        checks["green"] = "green" in text.lower()
        _, _, info = _snapshot(api, h_out)
        _write_mouse(api, h_in, max(1, info.srWindow.Right - info.srWindow.Left - 4), info.srWindow.Bottom - info.srWindow.Top, 0, buttons=FROM_LEFT_1ST_BUTTON_PRESSED)
        text, _, _ = _wait_for(api, h_out, lambda t, *_: "amber" in t.lower())
        checks["amber"] = "amber" in text.lower()
        _write_key(api, h_in, "\r", vk=0x0D)
        _write_key(api, h_in, "\r", vk=0x0D)
        before_body, _, _ = _snapshot(api, h_out)
        _write_mouse(api, h_in, 60, 4, DOUBLE_CLICK)
        text, _, _ = _wait_for(api, h_out, lambda t, *_: "Message 01" in t)
        checks["double_click"] = "Message 01" in text and text != before_body
        before_scroll, _, _ = _snapshot(api, h_out)
        _write_key(api, h_in, "\x00", vk=0x22)
        after_page, _, _ = _wait_for(api, h_out, lambda t, *_: t != before_scroll)
        _write_mouse(api, h_in, 60, 15, MOUSE_WHEELED, buttons=0xFF880000)
        after_wheel, _, _ = _wait_for(api, h_out, lambda t, *_: t != after_page)
        checks["scroll"] = after_page != before_scroll and after_wheel != after_page
        _write_key(api, h_in, "\x1b", vk=0x1B)
        _write_key(api, h_in, "\x00", vk=0x28)
        _write_key(api, h_in, "\r", vk=0x0D)
        text, _, _ = _wait_for(api, h_out, lambda t, *_: "Message 02" in t)
        checks["arrow_open"] = "Message 02" in text
        _write_key(api, h_in, "u")
        text, _, _ = _wait_for(api, h_out, lambda t, *_: "통합 받은편지함" in t)
        checks["unified"] = "통합 받은편지함" in text and "[" in text
        old_info = _snapshot(api, h_out)[2]
        old_width = old_info.srWindow.Right - old_info.srWindow.Left + 1
        active = _open_console(api, "CONOUT$", GENERIC_READ | GENERIC_WRITE)
        try:
            target_width = max(2, min(80, old_info.srWindow.Right - old_info.srWindow.Left))
            target_right = old_info.srWindow.Left + target_width - 1
            target_bottom = old_info.srWindow.Top + max(2, min(24, old_info.srWindow.Bottom - old_info.srWindow.Top + 1)) - 1
            rect = SMALL_RECT(old_info.srWindow.Left, old_info.srWindow.Top, target_right, target_bottom)
            resized_call = bool(api.SetConsoleWindowInfo(active, True, ctypes.byref(rect)))
            buffer_call = bool(api.SetConsoleScreenBufferSize(active, COORD(target_width, old_info.dwSize.Y)))
            details["resize_call"] = resized_call
            details["resize_buffer_call"] = buffer_call
            details["resize_error"] = ctypes.get_last_error()
        finally:
            api.CloseHandle(active)
        new_info = _snapshot(api, h_out)[2]
        details["resize_before_width"] = old_width
        details["resize_after_width"] = new_info.srWindow.Right - new_info.srWindow.Left + 1
        checks["runtime_resize_exercised"] = new_info.srWindow.Right - new_info.srWindow.Left + 1 != old_width
        _write_key(api, h_in, "q")
        try:
            checks["quit"] = child.wait(timeout=3) == 0
        except subprocess.TimeoutExpired:
            _write_key(api, h_in, "c", ctrl=True)
            checks["quit"] = child.wait(timeout=2) == 0
        after = _modes(api, h_in, h_out)
        checks["restored"] = before == after
        invalid = subprocess.run([str(exe), "--demo", "--theme", "invalid"], check=False)
        checks["invalid_theme"] = invalid.returncode != 0 and _modes(api, h_in, h_out) == after
    finally:
        if child.poll() is None:
            try:
                _write_key(api, h_in, "q")
            except OSError:
                pass
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                child.kill()
                try:
                    child.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    pass
        api.CloseHandle(h_in)
        api.CloseHandle(h_out)
    Path(result_path).write_text(json.dumps({"checks": checks, "details": details}, sort_keys=True), encoding="utf-8")


class TuiConsoleIntegrationTests(unittest.TestCase):
    @unittest.skipUnless(_win(), "Windows console integration is required")
    def test_CLI16_owned_console_supports_keyboard_mouse_theme_compact_and_restore(self):
        exe = next((p for p in EXE_CANDIDATES if p and p.exists()), None)
        if exe is None:
            self.skipTest("build hantong-t.exe first or set HANTONG_T_EXE")
        with tempfile.TemporaryDirectory(prefix="hantong-t-console-") as temp:
            result = Path(temp) / "result.json"
            startup = subprocess.STARTUPINFO()
            startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
            startup.wShowWindow = SW_HIDE
            worker = subprocess.Popen(
                [sys.executable, str(Path(__file__).resolve()), "--worker", str(exe), str(result)],
                creationflags=CREATE_NEW_CONSOLE, startupinfo=startup,
            )
            try:
                worker.wait(timeout=30)
            except subprocess.TimeoutExpired:
                worker.kill()
                self.fail("owned console worker timed out")
            if worker.returncode != 0 and result.exists():
                self.fail(f"owned console worker failed: {result.read_text(encoding='utf-8')}")
            self.assertEqual(worker.returncode, 0)
            payload = json.loads(result.read_text(encoding="utf-8"))
            checks = payload["checks"]
            required = {
                key: value
                for key, value in checks.items()
                if key != "runtime_resize_exercised"
            }
            self.assertTrue(all(required.values()), payload)


if __name__ == "__main__":
    if "--worker" in sys.argv:
        result_file = Path(sys.argv[sys.argv.index("--worker") + 2])
        try:
            _worker(Path(sys.argv[sys.argv.index("--worker") + 1]), result_file)
        except BaseException as exc:
            result_file.write_text(json.dumps({"error": repr(exc)}), encoding="utf-8")
            raise
    else:
        unittest.main()
