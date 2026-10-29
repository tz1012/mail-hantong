import os
from pathlib import Path
import sys
import unittest
from unittest import mock

sys.path.insert(0,str(Path(__file__).resolve().parents[2] / "scripts"))
import cargo_native


class CargoNativeTests(unittest.TestCase):
    def test_M104_non_windows_keeps_environment_and_does_not_prepare_tools(self):
        original = {"PATH":"existing", "LANG":"original"}
        with mock.patch.object(cargo_native.prepare_native_tools,"load_manifest") as load:
            self.assertEqual(cargo_native.build_environment(base=original,platform="posix"), original)
        load.assert_not_called()

    def test_M104_windows_only_changes_child_environment_and_requires_both_hashes(self):
        original = {"PATH":"existing", "LANG":"original"}
        specs = {name:{"version":"1"} for name in ("nasm","strawberry-perl")}
        with mock.patch.object(cargo_native.prepare_native_tools,"load_manifest",return_value=specs), mock.patch.object(cargo_native.prepare_native_tools,"ready_tool",side_effect=[Path("tools/nasm/nasm.exe"),Path("tools/perl/perl.exe")]) as validate:
            env = cargo_native.build_environment(base=original,platform="nt")
        self.assertEqual(validate.call_count,2)
        self.assertEqual(original,{"PATH":"existing","LANG":"original"})
        self.assertEqual(env["PATH"],os.pathsep.join([str(Path("tools/nasm")),str(Path("tools/perl")),"existing"]))
        self.assertEqual(env["LANG"],"C")
        self.assertEqual(env["OPENSSL_NO_VENDOR"],"0")

    def test_M104_invalid_tool_never_launches_cargo(self):
        with mock.patch.object(cargo_native,"build_environment",side_effect=ValueError("private-path")), mock.patch.object(cargo_native.subprocess,"run") as run, mock.patch("sys.stderr") as stderr:
            self.assertEqual(cargo_native.main(["build"]),2)
            self.assertNotIn("private-path",str(stderr.mock_calls))
        run.assert_not_called()
