"""Private MCP stdio host. No DB, Vault, Docker socket or host filesystem access.

Packages persist; each invocation gets a fresh process, UID, home and MCP session.
Only the supervisor can read the control token. Child environment is constructed.
"""
import hashlib
import hmac
import itertools
import json
import os
from pathlib import Path
import re
import resource
import selectors
import shutil
import signal
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path('/packages')
KEY = Path('/root/.broker/token').read_text().strip()
UIDS = itertools.count(100000)
LOCK = threading.Lock()
PROTOCOL = '2025-03-26'


def identity():
    with LOCK:
        return next(UIDS)


def child(uid):
    def prepare():
        os.setsid()
        os.setgroups([])
        os.setgid(uid)
        os.setuid(uid)
        resource.setrlimit(resource.RLIMIT_NOFILE, (2048, 2048))
        resource.setrlimit(resource.RLIMIT_NPROC, (48, 48))
        resource.setrlimit(resource.RLIMIT_FSIZE, (128 * 1024 * 1024,) * 2)
        resource.setrlimit(resource.RLIMIT_CPU, (180, 180))
    return prepare


def stop(p):
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    p.wait()


def environment(home, extra=None):
    return dict(PATH='/usr/local/bin:/usr/bin:/bin', HOME=str(home),
                TMPDIR=str(home), LANG='C.UTF-8', **(extra or {}))


def install(spec):
    kind, package, version = spec['transport'], spec['package'], spec['package_version']
    pattern = r'(?:@[a-z0-9_.-]+/)?[a-z0-9][a-z0-9_.-]*' if kind == 'npm' else r'[A-Za-z0-9][A-Za-z0-9_.-]*'
    if kind not in ('npm', 'python') or not re.fullmatch(pattern, package) or not re.fullmatch(r'[0-9][A-Za-z0-9.+_-]{0,79}', version):
        raise ValueError('mcp_package_invalid')
    # No credentials enter package installation. Lifecycle scripts / source builds are disabled.
    key = hashlib.sha256(f'{kind}:{package}:{version}'.encode()).hexdigest()
    target = ROOT / key
    with LOCK:
        if (target / '.ready').exists():
            return target
        uid = next(UIDS)
        if target.exists():
            shutil.rmtree(target)
        target.mkdir(mode=0o755)
        os.chown(target, uid, uid)
        with tempfile.TemporaryDirectory() as tmp:
            os.chown(tmp, uid, uid)
            cmd = (['npm', 'install', '--ignore-scripts', '--no-audit', '--no-fund', '--prefix', str(target), f'{package}@{version}'] if kind == 'npm' else
                   ['python3', '-m', 'pip', 'install', '--break-system-packages', '--only-binary=:all:', '--disable-pip-version-check', '--target', str(target), f'{package}=={version}'])
            p = subprocess.Popen(cmd, cwd=tmp, env=environment(tmp), stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, preexec_fn=child(uid))
            try:
                if p.wait(timeout=180) != 0:
                    raise ValueError('mcp_package_install_failed')
            finally:
                stop(p)
        # Only the supervisor may change installed code. Per-call UIDs can read it.
        for base, dirs, files in os.walk(target):
            os.chown(base, 0, 0)
            os.chmod(base, 0o755)
            for name in files:
                path = Path(base) / name
                if not path.is_symlink():
                    executable = bool(path.stat().st_mode & 0o111)
                    os.chown(path, 0, 0)
                    os.chmod(path, 0o755 if executable else 0o644)
        (target / '.ready').touch()
    return target


class Session:
    def __init__(self, process):
        self.p, self.buffer = process, b''
        self.selector = selectors.DefaultSelector()
        self.selector.register(process.stdout, selectors.EVENT_READ)
        self.deadline = time.monotonic() + 75

    def send(self, message):
        self.p.stdin.write(json.dumps(message, separators=(',', ':')).encode() + b'\n')
        self.p.stdin.flush()

    def rpc(self, number, method, params):
        self.send(dict(jsonrpc='2.0', id=number, method=method, params=params))
        while time.monotonic() < self.deadline:
            if b'\n' not in self.buffer:
                if not self.selector.select(max(0.01, self.deadline - time.monotonic())):
                    break
                data = os.read(self.p.stdout.fileno(), 65536)
                if not data:
                    raise ValueError('mcp_process_closed')
                self.buffer += data
                if len(self.buffer) > 1048576:
                    raise ValueError('mcp_response_too_large')
                continue
            line, self.buffer = self.buffer.split(b'\n', 1)
            message = json.loads(line)
            if message.get('id') == number and 'method' not in message:
                if 'error' in message:
                    raise ValueError('mcp_protocol_error')
                return message['result']
            if 'method' in message and 'id' in message:
                self.send(dict(jsonrpc='2.0', id=message['id'], error=dict(code=-32601, message='Client capability unavailable')))
        raise ValueError('mcp_timeout')


def invoke(request):
    spec = request['spec']
    target = install(spec)
    if request['op'] == 'install':
        return dict(installed=True)
    executable = spec['executable']
    if not re.fullmatch(r'[A-Za-z0-9_.-]{1,128}', executable):
        raise ValueError('mcp_executable_invalid')
    uid = identity()
    with tempfile.TemporaryDirectory() as home:
        os.chown(home, uid, uid)
        extra = request.get('environment', {})
        if any(k in extra for k in ('PATH', 'HOME', 'TMPDIR', 'PYTHONPATH', 'NODE_OPTIONS', 'LD_PRELOAD', 'LD_LIBRARY_PATH')):
            raise ValueError('mcp_environment_invalid')
        env = environment(home, extra)
        if spec['transport'] == 'npm':
            binary = target / 'node_modules' / '.bin' / executable
            if not binary.resolve().is_relative_to(target):
                raise ValueError('mcp_executable_invalid')
            cmd = [str(binary)]
        else:
            env['PYTHONPATH'] = str(target)
            cmd = ['python3', '-m', executable]
        p = subprocess.Popen(cmd + spec.get('args', []), cwd=home, env=env,
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                             preexec_fn=child(uid))
        try:
            rpc = Session(p)
            init = rpc.rpc(1, 'initialize', dict(protocolVersion=PROTOCOL, capabilities={}, clientInfo=dict(name='echelon-nano', version='0.3')))
            if init.get('protocolVersion') not in ('2024-11-05', PROTOCOL, '2025-06-18', '2025-11-25'):
                raise ValueError('mcp_protocol_version_unsupported')
            rpc.send(dict(jsonrpc='2.0', method='notifications/initialized'))
            if request['op'] == 'discover':
                tools, cursor, number = [], None, 2
                while True:
                    page = rpc.rpc(number, 'tools/list', dict(cursor=cursor) if cursor else {})
                    tools.extend(page['tools'])
                    cursor, number = page.get('nextCursor'), number + 1
                    if len(tools) > 256 or number > 18:
                        raise ValueError('mcp_tools_too_large')
                    if not cursor:
                        return dict(tools=tools)
            return rpc.rpc(2, 'tools/call', dict(name=request['tool'], arguments=request['arguments']))
        finally:
            stop(p)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        if self.path != '/invoke' or not hmac.compare_digest(self.headers.get('Authorization', ''), 'Bearer ' + KEY):
            self.send_error(403)
            return
        length = int(self.headers.get('Content-Length', 0))
        if length < 2 or length > 1048576:
            self.send_error(413)
            return
        try:
            result = invoke(json.loads(self.rfile.read(length)))
            body = json.dumps(dict(ok=True, result=result)).encode()
            if len(body) > 1048576:
                raise ValueError('mcp_response_too_large')
        except Exception as error:
            code = str(error) if isinstance(error, ValueError) and str(error).startswith('mcp_') else 'mcp_host_failed'
            body = json.dumps(dict(ok=False, error=code)).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


if __name__ == '__main__':
    ROOT.mkdir(exist_ok=True)
    ThreadingHTTPServer(('0.0.0.0', 9091), Handler).serve_forever()
