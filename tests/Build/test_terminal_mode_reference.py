from __future__ import annotations

import copy
import importlib.util
from pathlib import Path
import sys
import types
import unittest
from unittest import mock


def load_harness():
    # Pure comparator tests run on Windows too; no PTY or native API is called.
    modules = {}
    if sys.platform == "win32":
        modules = {name: types.ModuleType(name) for name in ("fcntl", "termios")}
        for name, value in {"IXON": 1, "ICANON": 2, "ECHO": 4,
                            "ISIG": 8, "IEXTEN": 16}.items():
            setattr(modules["termios"], name, value)
    path = Path(__file__).parents[2] / "scripts/test_unix_terminal.py"
    spec = importlib.util.spec_from_file_location("terminal_reference_harness", path)
    module = importlib.util.module_from_spec(spec)
    with mock.patch.dict(sys.modules, modules):
        spec.loader.exec_module(module)
    return module


harness = load_harness()


class TerminalReferenceTests(unittest.TestCase):
    def setUp(self):
        self.mode = [harness.termios.IXON, 0, 0,
                     harness.termios.ICANON | harness.termios.ECHO |
                     harness.termios.ISIG | harness.termios.IEXTEN,
                     9600, 9600, [b"\x00"] * 20]
        self.dynamic = 1 << 29

    def test_zero_reference_requires_exact_original(self):
        self.assertEqual(harness.expected_mode(self.mode, self.mode, self.mode), self.mode)

    def test_only_observed_native_transition_is_derived_without_mutation(self):
        after = copy.deepcopy(self.mode)
        after[3] |= self.dynamic
        with mock.patch.object(harness, "_DYNAMIC_LFLAG", self.dynamic):
            self.assertEqual(harness.expected_mode(self.mode, self.mode, after), after)
        self.assertFalse(self.mode[3] & self.dynamic)

    def test_dynamic_bit_is_rejected_when_not_supported_by_platform(self):
        after = copy.deepcopy(self.mode)
        after[3] |= self.dynamic
        with mock.patch.object(harness, "_DYNAMIC_LFLAG", 0), self.assertRaises(AssertionError):
            harness.expected_mode(self.mode, self.mode, after)

    def test_reference_cannot_change_any_controlled_or_unknown_flag(self):
        for index, bit in ((0, harness.termios.IXON), (3, harness.termios.ICANON),
                           (3, harness.termios.ECHO), (3, harness.termios.ISIG),
                           (3, harness.termios.IEXTEN), (0, 1 << 28),
                           (1, 1 << 28), (2, 1 << 28), (3, 1 << 28)):
            with self.subTest(index=index, bit=bit):
                after = copy.deepcopy(self.mode)
                after[index] ^= bit
                with self.assertRaises(AssertionError):
                    harness.expected_mode(self.mode, self.mode, after)

    def test_reference_cannot_change_either_speed_or_any_control_character(self):
        for index in (4, 5, *range(6, 26)):
            with self.subTest(index=index):
                after = copy.deepcopy(self.mode)
                if index < 6:
                    after[index] += 1
                else:
                    after[6][index - 6] = b"\x01"
                with self.assertRaises(AssertionError):
                    harness.expected_mode(self.mode, self.mode, after)

    def test_real_assertion_rejects_every_extra_flag_speed_and_cc_change(self):
        after = copy.deepcopy(self.mode)
        after[3] |= self.dynamic
        reference = (self.mode, after)
        for index in range(26):
            with self.subTest(index=index):
                current = copy.deepcopy(after)
                if index < 4:
                    current[index] ^= 1 << 28
                elif index < 6:
                    current[index] += 1
                else:
                    current[6][index - 6] = b"\x01"
                with mock.patch.object(harness, "_DYNAMIC_LFLAG", self.dynamic), \
                     mock.patch.object(harness.termios, "tcgetattr", return_value=current, create=True), \
                     self.assertRaises(AssertionError):
                    harness.assert_mode_restored(0, self.mode, "synthetic", reference)

    def test_dynamic_bit_is_not_blanket_ignored(self):
        current = copy.deepcopy(self.mode)
        current[3] |= self.dynamic
        with mock.patch.object(harness, "_DYNAMIC_LFLAG", self.dynamic), \
             mock.patch.object(harness.termios, "tcgetattr", return_value=current, create=True), \
             self.assertRaises(AssertionError):
            harness.assert_mode_restored(0, self.mode, "synthetic", (self.mode, self.mode))


if __name__ == "__main__":
    unittest.main()
