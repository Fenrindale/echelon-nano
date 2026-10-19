"""Operator-only native regression: a disconnected RPC must keep its receipt."""
import http.client
import json
import pathlib
import time
import uuid

token = pathlib.Path('/root/.broker/token').read_text().strip()
job_id = str(uuid.uuid4())
payload = json.dumps({
    'id': job_id,
    'script': 'sleep 2; printf DISCONNECT_OK > disconnect.txt',
    'files': [],
    'outputs': ['disconnect.txt'],
    'timeout_seconds': 10,
})
headers = {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}
first = http.client.HTTPConnection('127.0.0.1', 9090, timeout=15)
first.request('POST', '/jobs', body=payload, headers=headers)
time.sleep(0.5)
first.close()
retry = http.client.HTTPConnection('127.0.0.1', 9090, timeout=15)
retry.request('POST', '/jobs', body=payload, headers=headers)
response = retry.getresponse()
assert response.status == 200, 'Worker did not reconcile the disconnected RPC'
result = json.loads(response.read())
retry.close()
assert result['replayed'] and result['exit_code'] == 0
assert result['outputs'] == [{'name': 'disconnect.txt', 'content': 'DISCONNECT_OK'}]
print(json.dumps({'passed': True, 'job_id': job_id, 'replayed': result['replayed']}))
