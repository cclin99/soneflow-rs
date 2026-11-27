import json, pathlib
p=pathlib.Path(__file__).parent
report={}
for profile in ['debug','release']:
 a=(p/f'b-{profile}-rn.csv').read_text().splitlines();b=(p/f'c-{profile}-rn.csv').read_text().splitlines()
 report[profile]={'rn_rows':len(a),'rn_equal':a==b,'rn_differing_rows':sum(x!=y for x,y in zip(a,b)),'ftz_equal':len({(p/f'c-{profile}-ftz{k}.csv').read_bytes() for k in [0,64,32768,32832]})==1}
 def data(snap):
  return {s[0]:s[1:] for line in (p/f'{snap}-{profile}-nonrn.csv').read_text().splitlines() if (s:=line.split(',')) and not line.startswith('guard:')}
 base, cand=data('b'),data('c'); diffs={}; refs=[]
 for key, vals in cand.items():
  if key.startswith('ref:'):continue
  if vals!=cand['ref:'+key]:refs.append(key)
  mode=key.split(':')[0]
  for band,(v,w) in enumerate(zip(vals,base[key])):
   if v!=w:
    d=abs(int(v,16)-int(w,16)); entry=diffs.setdefault(mode,{}).setdefault(str(band),{'count':0,'min_ulp':d,'max_ulp':d});entry['count']+=1;entry['min_ulp']=min(entry['min_ulp'],d);entry['max_ulp']=max(entry['max_ulp'],d)
 report[profile]['runtime_reference_mismatching_rows']=refs
 report[profile]['nonrn_differing_bands']=diffs
(p/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
assert all(v['rn_equal'] and v['ftz_equal'] and not v['runtime_reference_mismatching_rows'] for v in report.values())

