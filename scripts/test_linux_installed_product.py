"""Owned PTY smoke test. No provider connection or real credentials."""
import errno, hashlib, json, os, pty, select, signal, subprocess, sys, tempfile, termios, time
from pathlib import Path

package = Path(sys.argv[1]).resolve()
phrase = b'synthetic-only-install-smoke-phrase'

def console(command, env, first, wrong=False):
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(str(command), [str(command)], env)
    data = bytearray()
    transcript = bytearray()
    alive = True
    deadline = time.monotonic() + 100
    sent = 0
    exit_sent = False
    try:
        while time.monotonic() < deadline:
            ready, _, _ = select.select([fd], [], [], .1)
            if ready:
                try:
                    chunk = os.read(fd, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    chunk = b''
                if chunk:
                    data.extend(chunk)
                    transcript.extend(chunk)
                else:
                    # PTY EOF can arrive before waitpid reports the child's exit.
                    # Keep polling within the original deadline, without accepting EOF as success.
                    time.sleep(.01)
            if first and sent == 0 and b'Choice:' in data:
                os.write(fd, b'1\n')
                sent = 1
                print('smoke: headless backend selected', flush=True)
                data.clear()
            marker = b'Enter headless vault passphrase' if first else b'Unlock headless vault'
            if marker in data and sent < (3 if first else 1):
                # Wait for the hidden input prompt, then send only synthetic bytes.
                if b'(input hidden):' in data and not termios.tcgetattr(fd)[3] & termios.ECHO:
                    os.write(fd, (b'incorrect-synthetic-phrase' if wrong else phrase) + b'\n')
                    sent += 1
                    print('smoke: protected synthetic input submitted', flush=True)
                    data.clear()
            if b'1) Connect a Google account' in data and not exit_sent:
                os.write(fd, b'2\r')
                exit_sent = True
                print('smoke: no-account menu reached', flush=True)
            done, status = os.waitpid(pid, os.WNOHANG)
            if done:
                alive = False
                assert phrase not in transcript
                return os.waitstatus_to_exitcode(status), exit_sent, bytes(data)
        done, status = os.waitpid(pid, os.WNOHANG)
        if done:
            alive = False
            assert phrase not in transcript
            return os.waitstatus_to_exitcode(status), exit_sent, bytes(data)
        raise AssertionError('installed CLI did not finish its bounded PTY flow')
    finally:
        os.close(fd)
        try:
            if alive:
                os.kill(pid, signal.SIGTERM)
                os.waitpid(pid, 0)
        except ProcessLookupError:
            pass
        except ChildProcessError:
            pass

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
