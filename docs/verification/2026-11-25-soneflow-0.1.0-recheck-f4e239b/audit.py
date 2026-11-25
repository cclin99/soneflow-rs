from pathlib import Path
import subprocess,json,re,hashlib,tomllib
root=Path.cwd(); out=root/'docs/verification/2026-11-25-soneflow-0.1.0-recheck-f4e239b'
def git(*a):return subprocess.check_output(['git',*a])
def dump(n,x):(out/n).write_text(json.dumps(x,indent=2,ensure_ascii=False),encoding='utf8')
files=git('ls-tree','-r','--name-only','f4e239b').decode().splitlines()
pat={'workstation':r'(?i)[A-Za-z]:[\\/](?:users|iso532(?:-1)?|soneflow)|/(?:home|Users|users)/','email':r'[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}','official_mentions':r'(?i)Annex\s*[AB]|official|workbook|官方|工作簿|電子附件'}
rows=[]
for f in files:
 if f=='tools/golden.sha256':rows.append({'file':f,'excluded':'user-only ISO-related checksum manifest'});continue
 if f.endswith('.bin'):rows.append({'file':f,'excluded':'binary fixture; owned synthetic provenance declaration, unchanged from parent; user material inspection pending'});continue
 text=git('show','f4e239b:'+f).decode('utf8')
 for name,p in pat.items():
  hits=[i for i,l in enumerate(text.splitlines(),1) if re.search(p,l)]
  if hits:rows.append({'file':f,'pattern':name,'lines':hits})
dump('r1-scan.json',{'tracked_files':len(files),'hits':rows})
# Tokenizer preserves literal contents; comments are recognized after literals.
lex=re.compile(r'(?P<raw>(?:br|r)(?P<hash>\#*)".*?"(?P=hash))|(?P<string>b?"(?:\\.|[^"\\])*")|(?P<char>b?\'(?:\\.|[^\'\\])\')|(?P<line>//[^\n]*)|(?P<block>/\*.*?\*/)|(?P<word>[A-Za-z_][A-Za-z_0-9]*)|(?P<other>\S)',re.S)
def toks(b):return [m.group() for m in lex.finditer(b.decode()) if m.lastgroup not in ('line','block')]
changed=git('diff','--name-only','fe786a0','f4e239b').decode().splitlines()
res=[]
for f in changed:
 if f.endswith(('.rs','.h')):
  a=git('show','fe786a0:'+f);b=git('show','f4e239b:'+f)
  res.append({'file':f,'equal_without_comments':toks(a)==toks(b),'old_tokens':len(toks(a)),'new_tokens':len(toks(b))})
dump('token-comparison.json',res)
unchanged={f:git('show','fe786a0:'+f)==git('show','f4e239b:'+f) for f in ['tools/setup_env.sh','tools/requirements.lock']}
dump('r4-unchanged.json',unchanged)
for f in ['README.md','README.zh-TW.md']:
 a=git('show','fe786a0:'+f).decode();b=git('show','f4e239b:'+f).decode()
 dump('r3-'+f+'.json',{'only_change':a.replace('1.8e-4','1.7e-4')==b,'new_value':'1.7e-4','prior_synthetic_measurement':'1.748e-4','rounded_two_significant_digits':format(1.748e-4,'.1e')})
lock=tomllib.loads((root/'Cargo.lock').read_text());versions={p['name']:p['version'] for p in lock['package'] if p['name'] in ['soneflow','soneflow-ffi','soneflow-py']}
for f in ['soneflow','soneflow-ffi','soneflow-py']:versions[f+'/Cargo.toml']=tomllib.loads((root/f/'Cargo.toml').read_text())['package']['version']
versions['CITATION.cff']=re.search(r'^version:\s*(.*)',(root/'CITATION.cff').read_text(),re.M)[1];dump('versions.json',versions)
p=subprocess.run(['gh','api','repos/cclin99/soneflow-rs/commits?sha=main&per_page=100'],capture_output=True);(out/'logs/remote-history.json').write_bytes(p.stdout);dump('remote-api-exit.json',{'exit':p.returncode})
print(json.dumps({'scan_hits':rows,'tokens':res,'unchanged':unchanged,'versions':versions},ensure_ascii=False,indent=2))
