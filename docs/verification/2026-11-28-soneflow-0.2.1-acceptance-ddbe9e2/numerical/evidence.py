import pathlib,json,zipfile,hashlib,struct
root=pathlib.Path(__file__).resolve().parents[5]
p=pathlib.Path(__file__).parent
# Snapshot restoration: all archived tracked files must remain byte identical.
result={}
for snap in ['b','c']:
 with zipfile.ZipFile(root/f'num-{snap}.zip') as z:
  mismatches=[];count=0
  for n in z.namelist():
   if n.endswith('/'):continue
   count+=1
   if z.read(n)!=(root/f'num-{snap}'/n).read_bytes():mismatches.append(n)
 result[snap]={'archived_files_checked':count,'modified_tracked_files':mismatches}
(p/'snapshot-restoration.json').write_text(json.dumps(result,indent=2)+'\n')
assert all(not r['modified_tracked_files'] for r in result.values())
def rows(name):return {s[0]:s[1:] for line in (p/name).read_text().splitlines() if (s:=line.split(','))}
a,b,inputs=rows('b-release-nonrn.csv'),rows('c-release-nonrn.csv'),rows('inputs.csv')
examples=[]
for k,v in b.items():
 if k.startswith(('ref:','guard:')):continue
 for band,(x,y) in enumerate(zip(v,a[k])):
  if abs(int(x,16)-int(y,16))==5:
   mode,field,case=k.split(':'); data=inputs[f'input:{field}:{case}']
   examples.append({'mode':mode,'field':field,'case':int(case),'band':band,'baseline_bits':y,'candidate_bits':x,'ulp':5,'input_bits':data,'input_decimal':[struct.unpack('>d',bytes.fromhex(h))[0] for h in data]})
(p/'five-ulp-cases.json').write_text(json.dumps(examples,indent=2)+'\n')
print(json.dumps(result));print('5 ULP cases',len(examples))
