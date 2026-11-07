"""Bounded PTY input for synthetic Linux product tests only."""
phrase = b'synthetic-only-install-smoke-phrase'

def console(command, env, first, wrong=False, args=()):
    import errno, os, pty, select, signal, termios, time
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(str(command), [str(command), *args], env)
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
                stop_deadline = time.monotonic() + 3
                while time.monotonic() < stop_deadline:
                    if os.waitpid(pid, os.WNOHANG)[0]:
                        break
                    time.sleep(.02)
                else:
                    os.kill(pid, signal.SIGKILL)
                    os.waitpid(pid, 0)
        except ProcessLookupError:
            pass
        except ChildProcessError:
            pass

