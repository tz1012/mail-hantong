import hashlib, json, tempfile, unittest, zipfile
from types import SimpleNamespace
from unittest import mock
from pathlib import Path
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import prepare_native_tools as native

class NativeToolTests(unittest.TestCase):
    def spec(self):
        return {"version":"x", "url":"fixture://tool", "sha256":"", "binary_candidates":["bin/tool.exe"]}

    def archive(self, root, files):
        path = root / "tool.zip"
        with zipfile.ZipFile(path, "w") as z:
            for name, data in files.items(): z.writestr(name, data)
        return path

    def test_hash_mismatch_does_not_extract(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td); archive=self.archive(root,{"bin/tool.exe":b"x"}); spec=self.spec(); spec["sha256"]="0"*64
            downloader=mock.Mock(side_effect=lambda _u,d: Path(d).write_bytes(archive.read_bytes()))
            with mock.patch.object(native,"ROOT",root), self.assertRaisesRegex(ValueError,"archive hash mismatch"):
                native.prepare("tool",root / "tools",spec,downloader)
            downloader.assert_called_once()
            self.assertEqual(list((root / "tools").glob("tool-x*")),[])

    def test_zip_traversal_is_rejected_without_final_directory(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td); archive=self.archive(root,{"../escape.exe":b"x"}); spec=self.spec(); spec["sha256"]=hashlib.sha256(archive.read_bytes()).hexdigest()
            with mock.patch.object(native,"ROOT",root), self.assertRaisesRegex(ValueError,"unsafe archive path"):
                native.prepare("tool",root / "tools",spec,lambda _u,d: Path(d).write_bytes(archive.read_bytes()))
            self.assertFalse((root / "escape.exe").exists())

    def test_prepare_is_idempotent_and_partial_directory_fails_closed(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td); archive=self.archive(root,{"bin/tool.exe":b"x"}); spec=self.spec(); spec["sha256"]=hashlib.sha256(archive.read_bytes()).hexdigest()
            with mock.patch.object(native,"ROOT",root):
                first=native.prepare("tool",root / "tools",spec,lambda _u,d: Path(d).write_bytes(archive.read_bytes()))
                second=native.prepare("tool",root / "tools",spec,lambda *_: self.fail("downloaded twice"))
                self.assertEqual(first,second)
                (root / "tools" / "tool-y").mkdir(parents=True)
                with self.assertRaisesRegex(FileExistsError,"partial native-tool directory"):
                    native.prepare("tool",root / "tools",{**spec,"version":"y"},lambda *_: self.fail("downloaded"))

    def test_completion_marker_and_binary_tamper_are_rejected(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td); archive=self.archive(root,{"bin/tool.exe":b"x"}); spec=self.spec(); spec["sha256"]=hashlib.sha256(archive.read_bytes()).hexdigest()
            with mock.patch.object(native,"ROOT",root):
                native.prepare("tool",root / "tools",spec,lambda _u,d: Path(d).write_bytes(archive.read_bytes()))
                final=root / "tools" / "tool-x"; marker=final / ".complete.json"
                data=json.loads(marker.read_text()); data["binary_sha256"]="0"*64; marker.write_text(json.dumps(data))
                with self.assertRaisesRegex(ValueError,"missing or modified"):
                    native.prepare("tool",root / "tools",spec,lambda *_: self.fail("downloaded"))

    def test_absolute_drive_and_backslash_members_are_rejected(self):
        for name in ["C:/evil", "\\evil", "a\\b", "../x"]:
            with self.assertRaises(ValueError): native.safe_member(name)

    def test_source_build_records_provenance_and_requires_version_smoke(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td); archive = self.archive(root, {"nasm-commit/README": b"source"})
            install = root / "VS"; (install / "VC/Auxiliary/Build").mkdir(parents=True)
            (install / "VC/Auxiliary/Build/vcvars64.bat").write_text("", encoding="utf-8")
            spec = {"version": "3.02", "source_url": "fixture://source",
                    "source_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                    "source_commit": "0123456789abcdef", "build_recipe": ["nmake", "/f", "Mkfiles/msvc.mak"],
                    "source_binary_candidates": ["bin/tool.exe"]}
            def runner(args, **kwargs):
                if args[0].lower().endswith("vswhere.exe"): return SimpleNamespace(stdout=str(install), stderr="", returncode=0)
                if args[0].lower().endswith("cmd.exe"): return SimpleNamespace(stdout="PATH=fixture\n", stderr="", returncode=0)
                if args[0].lower().endswith("nmake.exe"):
                    (Path(kwargs["cwd"]) / "bin").mkdir(); (Path(kwargs["cwd"]) / "bin/tool.exe").write_bytes(b"built")
                    return SimpleNamespace(stdout="", stderr="", returncode=0)
                if "-f" in args:
                    Path(args[args.index("-o") + 1]).write_bytes(b"\x64\x86fixture")
                    return SimpleNamespace(stdout="", stderr="", returncode=0)
                return SimpleNamespace(stdout="NASM version 3.02", stderr="", returncode=0)
            with mock.patch.object(native, "ROOT", root), mock.patch.object(native.shutil,"which",side_effect=lambda name,**kw: "C:/fixture/nmake.exe" if name=="nmake" else None):
                result = native.prepare_source("nasm", root / "tools", spec,
                    lambda _u, d: Path(d).write_bytes(archive.read_bytes()), runner)
                marker = json.loads((Path(result["root"]) / ".complete.json").read_text())
                self.assertEqual(marker["source_commit"], spec["source_commit"])
                self.assertEqual(marker["build_recipe"], spec["build_recipe"])
                self.assertEqual(native.prepare_source("nasm", root / "tools", spec, runner=runner), result)

    def test_source_build_rejects_empty_version_smoke(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td); archive = self.archive(root, {"src/README": b"source"})
            install = root / "VS"; (install / "VC/Auxiliary/Build").mkdir(parents=True)
            (install / "VC/Auxiliary/Build/vcvars64.bat").write_text("", encoding="utf-8")
            spec = {"version": "3.02", "source_url": "fixture://source",
                    "source_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                    "source_commit": "0123456789abcdef", "build_recipe": ["nmake"],
                    "source_binary_candidates": ["bin/tool.exe"]}
            def runner(args, **kwargs):
                if args[0].lower().endswith("vswhere.exe"): return SimpleNamespace(stdout=str(install), returncode=0)
                if args[0].lower().endswith("cmd.exe"): return SimpleNamespace(stdout="PATH=x\n", returncode=0)
                if args[0].lower().endswith("nmake.exe"):
                    (Path(kwargs["cwd"]) / "bin").mkdir(); (Path(kwargs["cwd"]) / "bin/tool.exe").write_bytes(b"built")
                    return SimpleNamespace(stdout="", returncode=0)
                return SimpleNamespace(stdout="", stderr="", returncode=0)
            with mock.patch.object(native, "ROOT", root), mock.patch.object(native.shutil,"which",side_effect=lambda name,**kw: "C:/fixture/nmake.exe" if name=="nmake" else None), self.assertRaisesRegex(ValueError, "version smoke"):
                native.prepare_source("nasm", root / "tools", spec,
                    lambda _u, d: Path(d).write_bytes(archive.read_bytes()), runner)

if __name__ == "__main__": unittest.main()
