from verify import *
import hashlib
run('ci-runs',['gh','api','repos/cclin99/soneflow-rs/actions/runs?head_sha=0ed8adbb256ad29ba43781f57bd5ac370e549121'])
run('ci-jobs',['gh','api','repos/cclin99/soneflow-rs/actions/runs/45851619042/jobs'])
for p in ['soneflow/tests/hash_gate.rs','soneflow-py/tests/test_smoke.py','soneflow-py/tests/test_stream.py','soneflow-py/tests/test_quality_v1.py']:
 run('diff-'+pathlib.Path(p).stem,['git','diff','15a2c07','0ed8adb','--',p],pathlib.Path('<SOURCE_ROOT>'))
run('pip-freeze',[str(ROOT/'venv/Scripts/python.exe'),'-m','pip','freeze'])
run('header',['cbindgen','--config','cbindgen.toml','--crate','soneflow-ffi','--output',str(E/'soneflow.generated.h')],C/'soneflow-ffi')
a=(E/'soneflow.generated.h').read_text();b=(C/'soneflow-ffi/include/soneflow.h').read_text();(E/'header-check.json').write_text(json.dumps({'normalized_text_equal':a==b}))
