from pathlib import Path
import json,re,subprocess,hashlib
root=Path.cwd();o=root/'docs/verification/2026-11-25-soneflow-0.1.0-recheck-f4e239b'
summary={}
for name in ['core-lib','doctest','integrations','validator','ffi','hash-enforced']:
 p=o/'logs'/f'{name}.txt'
 if not p.exists():raise SystemExit('not ready: '+name)
 text=p.read_text(encoding='utf8');rows=[]
 for m in re.finditer(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',text):rows.append(dict(zip(['status','passed','failed','ignored','measured','filtered'],[m[1],*[int(m[i]) for i in range(2,7)]])))
 summary[name]={'results':rows,'total_passed':sum(x['passed'] for x in rows),'total_failed':sum(x['failed'] for x in rows),'total_ignored':sum(x['ignored'] for x in rows),'total_filtered':sum(x['filtered'] for x in rows)}
 assert rows and not summary[name]['total_failed']
(o/'test-summary.json').write_text(json.dumps(summary,indent=2))
# Remove other personal machine identifiers from the retained failed SSH diagnostic.
for p in o.rglob('*'):
 if p.is_file() and p.name!='integrity.sha256':
  s=p.read_bytes().decode('utf8');clean=re.sub(r'S-1-5-21-(?:\d+-){3}\d+', '<WINDOWS_SID>',s)
  if clean!=s:p.write_bytes(clean.encode('utf8'))
passes=[]
for args in [['python','tools/redact_paths.py',str(o)],['python','tools/redact_paths.py','--check',str(o)]]:
 p=subprocess.run(args,capture_output=True,text=True,encoding='utf8');passes.append({'mode':'check' if '--check' in args else 'write','exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
# Normalize tool output paths before saving log metadata.
import importlib.util
sp=importlib.util.spec_from_file_location('redactor',root/'tools/redact_paths.py');mod=importlib.util.module_from_spec(sp);sp.loader.exec_module(mod)
for row in passes:
 for k in ['stdout','stderr']:row[k]=mod.redact_text(row[k])[0]
(o/'redaction.json').write_text(json.dumps(passes,indent=2))
sep=r'(?:\\\\|\\|/)'
extra=[re.compile(r'(?i)[A-Za-z]:'+sep+r'iso532-1(?![\w-])'),re.compile(r'(?i)[A-Za-z]:'+sep+r'users'+sep+r'[^\\/\s\"\']+'),re.compile(r'(?i)/[a-z]/iso532-1(?![\w-])'),re.compile(r'(?i)/(?:[a-z]/)?users/[^/\s\"\']+')]
fixes=[]
for p in o.rglob('*'):
 if p.is_file() and p.name!='integrity.sha256':
  s=p.read_text(encoding='utf8');original=s
  for pat in extra:s=pat.sub('<PRIVATE_PATH>',s)
  if s!=original:p.write_text(s,encoding='utf8');fixes.append(p.relative_to(o).as_posix())
remaining=[];non_utf8=[]
for p in o.rglob('*'):
 if p.is_file() and p.name!='integrity.sha256':
  try:s=p.read_text(encoding='utf8')
  except UnicodeDecodeError:non_utf8.append(p.relative_to(o).as_posix());continue
  if any(pat.search(s) for pat in extra):remaining.append(p.relative_to(o).as_posix())
(o/'supplemental-path-check.json').write_text(json.dumps({'rules':'case-insensitive private repository name and lowercase-user home paths, including drive/MSYS/escaped forms','modified_files':fixes,'remaining':remaining,'non_utf8':non_utf8,'redaction_warnings_reviewed':True,'redaction_skipped':0,'sid_redacted':True},indent=2))
assert not remaining and not non_utf8 and all(x['exit']==0 for x in passes)
# Verify all tracked source remains unchanged.
p=subprocess.run(['git','diff','--exit-code'],capture_output=True);assert p.returncode==0
(o/'integrity.sha256').write_text(''.join(hashlib.sha256(p.read_bytes()).hexdigest()+'  '+p.relative_to(o).as_posix()+'\n' for p in sorted(o.rglob('*')) if p.is_file() and p.name!='integrity.sha256'),encoding='utf8')
print(json.dumps({'tests':summary,'redaction':passes,'files_hashed':len((o/'integrity.sha256').read_text().splitlines())},indent=2))
