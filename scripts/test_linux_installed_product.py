"""Owned PTY smoke test. No provider connection or real credentials."""
import errno, hashlib, json, os, pty, select, signal, subprocess, sys, tempfile, termios, time
from pathlib import Path

package = Path(sys.argv[1]).resolve()
candidate_evidence = None
if len(sys.argv) == 3:
    from linux_pinned_candidate import verify_candidate
    descriptor_path = Path(sys.argv[2]).resolve()
    descriptor = descriptor_path.read_bytes()
    fields = json.loads(descriptor)
    report = verify_candidate(descriptor_path, descriptor_path.parent / fields["package_filename"],
                              descriptor_path.parent / fields["source_filename"], "Linux", "x86_64")
    candidate_evidence = (descriptor, report)
elif len(sys.argv) != 2:
    raise SystemExit("invalid product test arguments")
from linux_product_pty import console, phrase

with tempfile.TemporaryDirectory(prefix='hantong-native-product-') as temporary:
    root = Path(temporary)
    home = root/'home'
    home.mkdir(mode=0o700)
    env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(root/'data'),
               XDG_CONFIG_HOME=str(root/'config'), XDG_RUNTIME_DIR=str(root/'runtime'),
               TERM='xterm-256color', LANG='C.UTF-8')
    for name in ('data', 'config', 'runtime'):
        (root/name).mkdir(mode=0o700)
    for name in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS'):
        env.pop(name, None)
    subprocess.run(['sh', str(package/'install.sh')], env=env, check=True,
                   stdout=subprocess.DEVNULL)
    cli = home/'.local/bin/hantong'
    version = (package/'VERSION').read_text().strip()
    installed = root/'data/mail-hantong-cli/versions'/version
    core = installed/'bin/mail-hantong-core'
    if candidate_evidence is not None:
        # Test-owned trust premise: the artifact built by this exact CI commit.
        # This is not publisher authentication or an E9/user migration approval.
        descriptor, report = candidate_evidence
        assert report.version == version
        manifest = (installed / "release-manifest.json").read_bytes()
        assert hashlib.sha256(manifest).hexdigest() == report.manifest_sha256
        for row in json.loads(manifest)["files"]:
            path = installed / row["path"]
            assert not path.is_symlink() and path.is_file()
            assert path.stat().st_size == row["size"]
            assert hashlib.sha256(path.read_bytes()).hexdigest() == row["sha256"]
        pin = installed.parents[1] / "release-pins" / version / report.commit
        pin.mkdir(parents=True, mode=0o700)
        for name, value in (("descriptor.json", descriptor), ("pin.json", json.dumps({
                "format_version":1,"target":"linux-x86_64","version":version,
                "commit":report.commit,"descriptor_sha256":hashlib.sha256(descriptor).hexdigest()
                },sort_keys=True,separators=(",",":")).encode())):
            with (pin / name).open("xb") as stream:
                os.fchmod(stream.fileno(),0o600)
                stream.write(value)
                stream.flush(); os.fsync(stream.fileno())
    try:
        code, menu, output = console(cli, env, True)
        assert code == 0 and menu, (code, menu, output[-2048:])
        db = root/'data/mail-hantong/data/mail-hantong.db'
        assert db.exists() and not db.read_bytes().startswith(b'SQLite format')
        subprocess.run([str(cli),'core','status','--json'],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=15,check=True)
        subprocess.run([str(core), 'unix-stop'], env=env, check=True, timeout=15)
        ciphertext = root/'data/mail-hantong/secrets/headless-v1.age'
        original = hashlib.sha256(ciphertext.read_bytes()).digest()
        code, menu, output = console(cli, env, False, wrong=True)
        assert code != 0 and not menu and phrase not in output
        assert hashlib.sha256(ciphertext.read_bytes()).digest() == original
        code, menu, output = console(cli, env, False)
        assert code == 0 and menu, (code, menu, output[-2048:])
        subprocess.run([str(core), 'unix-stop'], env=env, check=True, timeout=15)
        result = subprocess.run([str(cli), 'init'], env=env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
        assert result.returncode != 0 and b'HUMAN_STEP_REQUIRED' in result.stderr
        assert phrase not in result.stdout + result.stderr
        print(json.dumps({'gui_and_dbus_absent': True, 'installed_cli_first_start': True,
                          'encrypted_db_created': True, 'wrong_unlock_preserves_vault': True,
                          'restart_unlock': True, 'non_tty_requires_human_step': True}))
    finally:
        subprocess.run([str(core), 'unix-stop'], env=env, timeout=15, check=False,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
