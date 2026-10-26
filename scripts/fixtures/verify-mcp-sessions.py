"""Owned stdio fixture: exercise the real host session/UID/process lifecycle."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile

spec = importlib.util.spec_from_file_location('owned_host', '/app/mcp_host.py')
host = importlib.util.module_from_spec(spec)
spec.loader.exec_module(host)
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    os.chmod(root, 0o755)
    module = root / 'owned_counter.py'
    module.write_text('''import sys,json
counter=0
for line in sys.stdin:
 m=json.loads(line)
 if "id" not in m: continue
 if m["method"]=="initialize": r={"protocolVersion":"2025-03-26","capabilities":{},"serverInfo":{"name":"owned-counter","version":"1"}}
 else:
  counter+=1
  r={"content":[{"type":"text","text":str(counter)}]}
 print(json.dumps({"jsonrpc":"2.0","id":m["id"],"result":r}),flush=True)
''')
    host.install = lambda _: root
    request = dict(spec=dict(transport='python', executable='owned_counter', args=[]),
                   environment={}, op='call', tool='increment', arguments={}, session_scope='owned:order_a')
    try:
        assert host.invoke(request)['content'][0]['text'] == '1'
        assert host.invoke(request)['content'][0]['text'] == '2'
        other = dict(request, session_scope='owned:order_b')
        assert host.invoke(other)['content'][0]['text'] == '1'
        rotated = dict(request, environment={'OWNED_BINDING': 'rotated-synthetic-value'})
        assert host.invoke(rotated)['content'][0]['text'] == '1'
        host.invoke(dict(request, op='close'))
        assert host.invoke(request)['content'][0]['text'] == '1'
        assert host.invoke(other)['content'][0]['text'] == '2'
        print(json.dumps(dict(passed=True, same_order_reused=True, other_order_isolated=True,
                             environment_rotation_isolated=True, close_releases_session=True)))
    finally:
        host.invoke(dict(request, op='close'))
        host.invoke(dict(other, op='close'))
