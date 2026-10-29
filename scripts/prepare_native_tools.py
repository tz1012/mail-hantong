#!/usr/bin/env python3
"""Prepare ignored, workspace-local NASM and native Perl archives safely."""
from __future__ import annotations
import argparse, hashlib, json, os, re, shutil, subprocess, sys, tempfile, urllib.request, zipfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
MAX_DOWNLOAD, MAX_UNPACKED = 1 << 30, 2 << 30
MANIFEST = ROOT / "scripts" / "native-tools.json"

def load_manifest(path=MANIFEST):
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if data.get("schema_version") != 1 or not isinstance(data.get("tools"), dict): raise ValueError("unsupported native-tools manifest")
    return data["tools"]

def _inside(root, candidate):
    root, candidate = Path(root).resolve(), Path(candidate).resolve()
    if not candidate.is_relative_to(root): raise ValueError("path escapes tools root")
    return candidate

def safe_member(name):
    if not name or "\\" in name or "\0" in name: raise ValueError("unsafe archive path")
    path = PurePosixPath(name)
    reserved = {"CON", "PRN", "AUX", "NUL"} | {f"COM{i}" for i in range(1, 10)} | {f"LPT{i}" for i in range(1, 10)}
    if path.is_absolute() or any(":" in part or part.endswith((".", " ")) or part.split(".")[0].upper() in reserved for part in path.parts) or ".." in path.parts: raise ValueError("unsafe archive path")
    return path

def _is_link(info): return ((info.external_attr >> 16) & 0o170000) == 0o120000

def extract_safe(archive, destination):
    destination = Path(destination); destination.mkdir(parents=True, exist_ok=False); total = 0
    with zipfile.ZipFile(archive) as zf:
        for info in zf.infolist():
            member = safe_member(info.filename)
            if _is_link(info): raise ValueError("archive links are not allowed")
            total += info.file_size
            if total > MAX_UNPACKED: raise ValueError("archive exceeds unpacked size limit")
            target = _inside(destination, destination.joinpath(*member.parts))
            if info.is_dir(): target.mkdir(parents=True, exist_ok=True); continue
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists(): raise ValueError("duplicate archive path")
            with zf.open(info) as source, target.open("xb") as output: shutil.copyfileobj(source, output, 1024 * 1024)

def download(url, destination):
    with urllib.request.urlopen(url, timeout=30) as response, Path(destination).open("xb") as output:
        total = 0
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk: break
            total += len(chunk)
            if total > MAX_DOWNLOAD: raise ValueError("download exceeds size limit")
            output.write(chunk)

def _sha256_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""): digest.update(chunk)
    return digest.hexdigest()

def _find_binary(root, spec):
    for relative in spec.get("binary_candidates", []):
        candidate = _inside(root, Path(root) / relative)
        if candidate.is_file(): return candidate
    raise FileNotFoundError("expected native tool executable not found")

def _msvc_environment(base=None, runner=subprocess.run):
    """Return a per-process MSVC x64 environment; never changes global state."""
    env = {key.upper():value for key,value in (os.environ if base is None else base).items()}
    vswhere = shutil.which("vswhere.exe") or next((str(p) for p in (
        Path(os.environ.get("ProgramFiles(x86)", "")) / "Microsoft Visual Studio/Installer/vswhere.exe",
        Path(os.environ.get("ProgramFiles", "")) / "Microsoft Visual Studio/Installer/vswhere.exe") if p.is_file()), "vswhere.exe")
    found = runner([vswhere, "-latest", "-products", "*", "-requires",
                    "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                    "-property", "installationPath"], env=env,
                   capture_output=True, text=True, errors="replace", timeout=30, check=True)
    installations = found.stdout.strip().splitlines()
    if installations:
        vcvars = Path(installations[0]) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    else:
        roots = [Path(os.environ.get("ProgramFiles(x86)", "")) / "Microsoft Visual Studio",
                 Path(os.environ.get("ProgramFiles", "")) / "Microsoft Visual Studio"]
        candidates = [p for root in roots if root.is_dir() for p in root.glob("*/BuildTools/VC/Auxiliary/Build/vcvars64.bat")]
        if not candidates: raise FileNotFoundError("MSVC installation not found")
        vcvars = candidates[0]
    if not vcvars.is_file(): raise FileNotFoundError("MSVC vcvars64.bat not found")
    cmd = env.get("COMSPEC", "cmd.exe")
    exported = runner([cmd, "/d", "/c", "call", str(vcvars), "&&", "set"], env=env,
                      capture_output=True, text=True, errors="replace", timeout=30, check=True)
    result = dict(env)
    for line in exported.stdout.splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            if key: result[key.upper()] = value
    return result

def prepare_source(name, tools_root, spec, downloader=download, runner=subprocess.run, env_extra=None):
    """Build a pinned source archive into a distinct workspace-local tool path."""
    tools_root = _inside(ROOT, tools_root); tools_root.mkdir(parents=True, exist_ok=True)
    commit = spec["source_commit"]
    version = spec["version"]
    final = _inside(tools_root, tools_root / f"{name}-source-{version}-{commit[:12]}")
    if final.exists():
        binary = _validate_source_ready(final, spec, name)
        return {"name": name, "version": version, "root": str(final), "binary": str(binary)}
    downloads = _inside(tools_root, tools_root / "downloads"); downloads.mkdir(exist_ok=True)
    archive = downloads / f"{name}-source-{commit[:12]}.zip"
    staging = Path(tempfile.mkdtemp(prefix=f".{name}-source-", dir=tools_root)); temporary = None
    try:
        if not archive.exists():
            fd, temporary = tempfile.mkstemp(prefix=f".{archive.name}-", suffix=".tmp", dir=downloads); os.close(fd); Path(temporary).unlink()
            downloader(spec["source_url"], temporary)
            if _sha256_file(temporary) != spec["source_sha256"]: raise ValueError("source archive hash mismatch")
            os.replace(temporary, archive); temporary = None
        if _sha256_file(archive) != spec["source_sha256"]: raise ValueError("source archive hash mismatch")
        payload = staging / "payload"
        extract_safe(archive, payload)
        roots = [p for p in payload.iterdir() if p.is_dir()]
        source = roots[0] if len(roots) == 1 else payload
        env = _msvc_environment(base=env_extra, runner=runner)
        env["CL"] = spec.get("build_environment", {}).get("CL", "/D_AMD64_")
        for variable in ("LC_ALL","LC_CTYPE","LANG"): env[variable] = "C"
        recipe = list(spec["build_recipe"])
        # Windows CreateProcess does not use the supplied child PATH to locate
        # its executable. Resolve nmake against the captured MSVC PATH first.
        executable = shutil.which(recipe[0],path=env.get("PATH",""))
        if executable is None: raise FileNotFoundError("MSVC build tool not found in captured environment")
        runner([executable,*recipe[1:]], cwd=source, env=env, capture_output=True, text=True, errors="replace", timeout=600, check=True)
        binary = _find_binary(source, {"binary_candidates": spec["source_binary_candidates"]})
        smoke = runner([str(binary), "-v"], cwd=source, env=env, capture_output=True, text=True, errors="replace", timeout=10, check=False)
        version_output = smoke.stdout or ""
        if smoke.returncode != 0 or not re.search(rf"NASM version\s+{re.escape(version)}(?:\s|$)", version_output): raise ValueError("source binary version smoke failed")
        fixture = source / ".native-win64-smoke.asm"; obj = source / ".native-win64-smoke.obj"
        fixture.write_text("bits 64\ndefault rel\nsection .text\nglobal smoke\nsmoke:\n xor eax,eax\n ret\n", encoding="ascii")
        try:
            assembled = runner([str(binary), "-f", "win64", "-o", str(obj), str(fixture)], cwd=source, env=env,
                               capture_output=True, text=True, errors="replace", timeout=10, check=False)
            if assembled.returncode != 0 or not obj.is_file() or obj.read_bytes()[:2] != b"\x64\x86":
                raise ValueError("source binary Win64 assembly smoke failed")
        finally:
            fixture.unlink(missing_ok=True); obj.unlink(missing_ok=True)
        relative = binary.relative_to(source).as_posix()
        marker = {"schema_version": 1, "name": name, "version": version,
                  "source_url": spec["source_url"], "source_sha256": spec["source_sha256"],
                  "source_commit": commit, "source_ref":spec.get("source_ref",commit), "build_recipe": recipe,
                  "build_environment":{"CL":env["CL"]}, "binary": relative,
                  "binary_sha256": _sha256_file(binary), "version_output": version_output.strip()}
        out = staging / "payload"
        if source != payload:
            out = staging / "payload-final"; os.rename(source, out)
            binary = out / relative
        (out / ".complete.json").write_text(json.dumps(marker, sort_keys=True), encoding="utf-8")
        os.rename(out, final)
        shutil.rmtree(_inside(tools_root, staging), ignore_errors=True)
        return {"name": name, "version": version, "root": str(final), "binary": str(final / relative)}
    except Exception:
        if temporary: Path(temporary).unlink(missing_ok=True)
        shutil.rmtree(_inside(tools_root, staging), ignore_errors=True); raise

def _validate_source_ready(final, spec, name):
    marker = Path(final) / ".complete.json"
    data = json.loads(marker.read_text(encoding="utf-8"))
    required = {"schema_version", "name", "version", "source_url", "source_sha256", "source_commit", "source_ref", "build_recipe", "build_environment", "binary", "binary_sha256", "version_output"}
    if (set(data) != required or data["schema_version"] != 1 or data["name"] != name
            or data["version"] != spec["version"] or data["source_url"] != spec["source_url"]
            or data["source_sha256"] != spec["source_sha256"]
            or data["source_commit"] != spec["source_commit"]
            or data["source_ref"] != spec.get("source_ref",spec["source_commit"])
            or data["build_recipe"] != list(spec["build_recipe"])
            or data["build_environment"] != {"CL":spec.get("build_environment",{}).get("CL","/D_AMD64_")}
            or not re.search(rf"NASM version\s+{re.escape(spec['version'])}(?:\s|$)", data["version_output"])):
        raise ValueError("source completion marker does not match build provenance")
    binary = _inside(final, Path(final) / data["binary"])
    if not binary.is_file() or _sha256_file(binary) != data["binary_sha256"]: raise ValueError("source binary is missing or modified")
    return binary

def ready_tool(name, tools_root, spec):
    root = Path(tools_root)
    if "source_url" in spec:
        final = root / f"{name}-source-{spec['version']}-{spec['source_commit'][:12]}"
        return _validate_source_ready(final, spec, name)
    return _validate_ready(root / f"{name}-{spec['version']}", {**spec, "name": name})

def _validate_ready(final, spec):
    marker = Path(final) / ".complete.json"
    if not marker.is_file(): raise FileExistsError("partial native-tool directory exists")
    data = json.loads(marker.read_text(encoding="utf-8"))
    required = {"schema_version", "name", "version", "sha256", "binary", "binary_sha256"}
    if set(data) != required or data["schema_version"] != 1 or data["name"] not in (spec.get("name", data["name"]),) or data["version"] != spec["version"] or data["sha256"] != spec["sha256"]:
        raise ValueError("native-tool completion marker does not match manifest")
    binary = _inside(final, Path(final) / data["binary"])
    if not binary.is_file() or binary.name != Path(data["binary"]).name or _sha256_file(binary) != data["binary_sha256"]:
        raise ValueError("native-tool binary is missing or modified")
    return binary

def prepare(name, tools_root, spec, downloader=download):
    tools_root = _inside(ROOT, tools_root); tools_root.mkdir(parents=True, exist_ok=True)
    final = _inside(tools_root, tools_root / f"{name}-{spec['version']}"); marker = final / ".complete.json"
    if final.exists():
        binary = _validate_ready(final, {**spec, "name": name})
        return {"name": name, "version": spec["version"], "root": str(final), "binary": str(binary)}
    downloads = _inside(tools_root, tools_root / "downloads"); downloads.mkdir(exist_ok=True)
    archive = downloads / f"{name}-{spec['version']}.zip"; staging = Path(tempfile.mkdtemp(prefix=f".{name}-", dir=tools_root)); temporary = None
    try:
        if not archive.exists():
            fd, temporary = tempfile.mkstemp(prefix=f".{archive.name}-", suffix=".tmp", dir=downloads); os.close(fd); Path(temporary).unlink()
            downloader(spec["url"], temporary)
            if _sha256_file(temporary) != spec["sha256"]: raise ValueError("archive hash mismatch")
            os.replace(temporary, archive)
            temporary = None
        if _sha256_file(archive) != spec["sha256"]: raise ValueError("archive hash mismatch")
        extract_safe(archive, staging / "payload"); binary = _find_binary(staging / "payload", spec)
        relative = binary.relative_to(staging / "payload").as_posix()
        (staging / "payload" / ".complete.json").write_text(json.dumps({"schema_version":1,"name":name,"version":spec["version"],"sha256":spec["sha256"],"binary":relative,"binary_sha256":_sha256_file(binary)}, sort_keys=True), encoding="utf-8")
        # Rename without replacement: a concurrent/newly-created final path is
        # a fail-closed partial-install conflict, never something to overwrite.
        os.rename(staging / "payload", final)
        staging.rmdir()
        return {"name": name, "version": spec["version"], "root": str(final), "binary": str(final / relative)}
    except Exception:
        if temporary: Path(temporary).unlink(missing_ok=True)
        shutil.rmtree(_inside(tools_root, staging), ignore_errors=True); raise

def main(argv=None):
    parser = argparse.ArgumentParser(); parser.add_argument("--tool", choices=["nasm", "strawberry-perl", "all"], default="all"); parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    if os.name != "nt":
        print(json.dumps({"supported": False, "reason": "Windows-only native build tools"})); return 0 if args.check else 2
    specs = load_manifest(); selected = specs.keys() if args.tool == "all" else [args.tool]; root = ROOT / ".tools" / "native"
    try:
        results = []
        for name in selected:
            spec = specs[name]; final = root / f"{name}-{spec['version']}"
            source_mode = "source_url" in spec
            if args.check:
                if source_mode:
                    final = root / f"{name}-source-{spec['version']}-{spec['source_commit'][:12]}"
                if not final.exists():
                    results.append({"name":name,"ready":False,"root":str(final)}); continue
                try:
                    binary = (_validate_source_ready(final, spec, name) if source_mode
                              else _validate_ready(final, {**spec, "name": name}))
                    results.append({"name":name,"ready":True,"root":str(final),"binary":str(binary)})
                except (OSError, ValueError, KeyError, FileNotFoundError) as exc:
                    results.append({"name":name,"ready":False,"root":str(final),"error":str(exc)})
            else:
                if source_mode:
                    perl = prepare("strawberry-perl", root, specs["strawberry-perl"])
                    perl_dir = str(Path(perl["binary"]).parent)
                    env_extra = dict(os.environ)
                    env_extra["PATH"] = os.pathsep.join([perl_dir, env_extra.get("PATH", "")])
                    results.append(prepare_source(name, root, spec, env_extra=env_extra))
                else:
                    results.append(prepare(name, root, spec))
        print(json.dumps({"supported":True,"tools":results}, ensure_ascii=False, sort_keys=True)); return 0
    except (OSError, ValueError, KeyError, zipfile.BadZipFile, subprocess.SubprocessError) as exc:
        print(f"error: {exc}", file=sys.stderr); return 1

if __name__ == "__main__": raise SystemExit(main())
