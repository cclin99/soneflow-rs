import pathlib,subprocess,re,json
root=pathlib.Path.cwd()
out=root/'docs/verification/2026-11-24-soneflow-0.1.0-acceptance-fe786a0'
files=subprocess.check_output(['git','ls-tree','-r','--name-only','fe786a0'],text=True).splitlines()
patterns={'old_name':r'iso532|ISO532','workstation':r'[A-Za-z]:[\\/](?:Users|ISO532|iso532|soneflow)|/home/|/Users/','email':r'[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}','official_mentions':r'Annex\s*[AB]|official|workbook|官方|工作簿|電子附件'}
rows=[]
for f in files:
 if f=='tools/golden.sha256': rows.append({'file':f,'excluded':'user-only ISO-related checksum manifest'});continue
 b=(root/f).read_bytes()
 if b'\0' in b: rows.append({'file':f,'binary_bytes':len(b),'inspection':'owned synthetic fixture per adjacent provenance'});continue
 text=b.decode('utf8')
 for name,pat in patterns.items():
  hits=[i for i,line in enumerate(text.splitlines(),1) if re.search(pat,line,re.I if name=='official_mentions' else 0)]
  if hits: rows.append({'file':f,'pattern':name,'lines':hits})
print(json.dumps({'tracked_files':len(files),'hits':rows},indent=2))
