import pathlib,subprocess,re,json,difflib
root=pathlib.Path.cwd()
out=root/'docs/verification/2026-11-24-soneflow-0.1.0-acceptance-fe786a0'
files=subprocess.check_output(['git','ls-tree','-r','--name-only','fe786a0'],text=True).splitlines()
# Lex strings before comments so comment markers inside literals remain tokens.
pat=re.compile(r'(?P<raw>(?:br|r)(?P<hash>\#*)".*?"(?P=hash))|(?P<string>b?"(?:\\.|[^"\\])*")|(?P<char>b?\'(?:\\.|[^\'\\])\')|(?P<line>//[^\n]*)|(?P<block>/\*.*?\*/)|(?P<word>[A-Za-z_][A-Za-z_0-9]*)|(?P<other>\S)',re.S)
def toks(s):
 s=s.replace('Iso532Error','SoneflowError')
 s=s.replace('iso532_core','soneflow_core').replace('ISO532','SNF').replace('Iso532','Snf')
 s=re.sub(r'\biso532(?=_)','snf',s);s=s.replace('iso532','soneflow')
 s=re.sub(r'((?:pub\s+)?use\s+[^;{]+)\{([^{}]+)\};',lambda m:m[1]+'{'+','.join(sorted(x.strip() for x in m[2].split(',') if x.strip()))+'};',s)
 return [m.group() for m in pat.finditer(s) if m.lastgroup not in ['line','block']]
rows=[]
for f in files:
 if '/src/' not in f or not f.endswith('.rs'):continue
 old=f.replace('soneflow-ffi/','iso532-ffi/').replace('soneflow-py/','iso532-py/').replace('soneflow/','iso532/')
 a=subprocess.check_output(['git','show','0a3a7c9:'+old]).decode('utf8');b=(root/f).read_text(encoding='utf8')
 aa,bb=toks(a),toks(b)
 changes=[]
 for tag,i,j,k,l in difflib.SequenceMatcher(None,aa,bb,autojunk=False).get_opcodes():
  if tag!='equal':changes.append({'old':aa[i:j],'new':bb[k:l]})
 row={'file':f,'equal':aa==bb,'changes':changes};rows.append(row)
for row in rows:
 row['changed_token_groups']=len(row.pop('changes'))
print(json.dumps({'files':len(rows),'equal':sum(r['equal'] for r in rows),'results':rows},indent=2))
