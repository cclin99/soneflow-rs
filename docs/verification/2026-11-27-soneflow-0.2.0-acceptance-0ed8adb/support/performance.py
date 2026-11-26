from verify import *
import shutil,statistics
os.environ['RAYON_NUM_THREADS']='1'
ids=['zwtv_10s_t1/scalar/480000','zwtv_10s_t1/avx2/480000','stream_push_10s_480chunk/scalar','stream_push_10s_480chunk/avx2']
rows=[]
for rnd in range(1,4):
 for label,repo in [('baseline',B),('candidate',C)]:
  run(f'bench-{rnd}-{label}',['cargo','bench','--bench','loudness','--','--noplot','zwtv_10s_t1|stream_push_10s_480chunk'],repo/'soneflow')
  for id in ids:
   p=repo/'target/criterion'/id/'new/estimates.json';v=json.loads(p.read_text());rows.append(dict(round=rnd,snapshot=label,id=id,median_ns=v['median']['point_estimate']))
   dest=E/'criterion'/f'round-{rnd}-{label}'/id;dest.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest/'estimates.json')
  (E/'benchmark-rounds.json').write_text(json.dumps(rows,indent=2))
comp=[]
for id in ids:
 a=statistics.median(r['median_ns']for r in rows if r['id']==id and r['snapshot']=='candidate');b=statistics.median(r['median_ns']for r in rows if r['id']==id and r['snapshot']=='baseline');comp.append(dict(id=id,candidate_ns=a,baseline_ns=b,percent=(a/b-1)*100,pass_gate=a<=b*1.03))
(E/'benchmark-comparison.json').write_text(json.dumps(comp,indent=2));print(comp,flush=True)
os.environ.pop('RAYON_NUM_THREADS')
run('readme-bench-mt',['cargo','bench','-p','soneflow','--bench','loudness','--','--noplot'])
run('readme-filter-bank',['cargo','bench','-p','soneflow','--features','internals','--bench','core','--','--noplot','filter_bank'])
V=str(ROOT/'venv/Scripts/python.exe')
run('compare-30',[V,'tools/compare_mosqito.py','--iterations','30'])
run('compare-tail-5',[V,'tools/compare_mosqito.py','--iterations','5','--nl-init','mosqito_tail'])
run('compare-tables',[V,'tools/compare_mosqito.py','--check-tables'])
