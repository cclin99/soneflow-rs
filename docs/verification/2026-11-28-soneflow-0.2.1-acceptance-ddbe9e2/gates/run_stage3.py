import pathlib
exec((pathlib.Path(__file__).parent/'run_gates.py').read_text(encoding='utf-8-sig').split("run('00-versions'")[0].replace("results.json", "results-stage3.json"))
results=[]
import tomllib,hashlib
versions={p:tomllib.loads((REPO/p/'Cargo.toml').read_text())['package']['version'] for p in ['soneflow','soneflow-ffi','soneflow-py']}
versions['lock']={x['name']:x['version'] for x in tomllib.loads((REPO/'Cargo.lock').read_text())['package'] if x['name'].startswith('soneflow')}
versions['citation']=[x for x in (REPO/'CITATION.cff').read_text().splitlines() if x.startswith('version:')]
a=(OUT/'soneflow.generated.h').read_bytes();b=(REPO/'soneflow-ffi/include/soneflow.h').read_bytes();c=subprocess.check_output(['git','show','82d1d61:soneflow-ffi/include/soneflow.h'],cwd=REPO)
versions['header']={'generated_equals_committed':a==b,'committed_equals_baseline':b==c,'sha256':hashlib.sha256(b).hexdigest()}
(OUT/'r1-check.json').write_text(json.dumps(versions,indent=2))
smoke=ROOT/'smoke-gates';smoke.mkdir(exist_ok=True)
batch='''@echo off
call "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\Common7\\Tools\\VsDevCmd.bat" -arch=x64
if errorlevel 1 exit /b %errorlevel%
cl /nologo /W3 /I"<AUDIT_ROOT>\\repo\\soneflow-ffi\\include" "<AUDIT_ROOT>\\repo\\soneflow-ffi\\tests\\smoke.c" /link /LIBPATH:"<AUDIT_ROOT>\\target-gates\\release" soneflow_ffi.dll.lib
if errorlevel 1 exit /b %errorlevel%
copy /Y "<AUDIT_ROOT>\\target-gates\\release\\soneflow_ffi.dll" . >NUL
smoke.exe
'''
(OUT/'smoke.cmd').write_text(batch)
run('32-ffi-c-smoke',['cmd','/c',str(OUT/'smoke.cmd')],smoke)
wheel_rows=[{'file':p.name,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in (ROOT/'wheels-gates').glob('*.whl')]
(OUT/'wheel-hashes.json').write_text(json.dumps(wheel_rows,indent=2))
run('33-wheel-provenance',[str(ROOT/'venv-gates/Scripts/python.exe'),'-c','import soneflow,importlib.metadata; print(soneflow.__file__); print(importlib.metadata.version("soneflow"))'])
assert all(versions[p]=='0.2.1' for p in ['soneflow','soneflow-ffi','soneflow-py'])
assert all(v=='0.2.1' for v in versions['lock'].values())
assert versions['citation']==['version: 0.2.1']
assert a==b==c, 'header bytes differ'
print('R1 version/header assertions PASS',flush=True)

