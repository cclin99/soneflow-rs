from verify import *
run('docs-source-redaction',['python','tools/redact_paths.py','--check']+[str(p.relative_to(C))for p in (C/'docs').glob('*.md')])
run('ffi-release',['cargo','build','--release','-p','soneflow-ffi'])
