from verify import *
import hashlib
run('cpu',['powershell','-NoProfile','-Command','Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors | ConvertTo-Json'])
run('cargo-tools',['cargo','install','--list'])
run('rust-components',['rustup','component','list','--installed'])
run('python-environment',[str(ROOT/'venv/Scripts/python.exe'),'-c','import sys,platform,numpy,importlib.metadata as m;print(sys.version);print(platform.platform());print("numpy",numpy.__version__);print("mosqito",m.version("mosqito"));print("scipy",m.version("scipy"))'])
(E/'wheel-integrity.json').write_text(json.dumps([dict(file=p.name,sha256=hashlib.sha256(p.read_bytes()).hexdigest(),source='0ed8adbb256ad29ba43781f57bd5ac370e549121')for p in (ROOT/'wheels').glob('*.whl')],indent=2))
