from verify import *
import re
rows=[]
expected={'zwtv_10s_t12/scalar/480000':44.1,'zwtv_10s_t12/avx2/480000':30.6,'filter_bank_10s_t12/scalar/480000':13.7,'filter_bank_10s_t12/avx2/480000':5.58}
for id,ms in expected.items():
 got=json.loads((C/'target/criterion'/id/'new/estimates.json').read_text())['median']['point_estimate']/1e6;rows.append(dict(id=id,readme_ms=ms,measured_ms=got,percent=(got/ms-1)*100,within_20=abs(got/ms-1)<=.2))
for r,ms in zip(json.loads((E/'benchmark-comparison.json').read_text()),[195.3,101.1,187.5,84.5]):
 got=r['candidate_ns']/1e6;rows.append(dict(id=r['id'],readme_ms=ms,measured_ms=got,percent=(got/ms-1)*100,within_20=abs(got/ms-1)<=.2))
t=(E/'logs/compare-30-ready.log').read_text();vals=re.findall(r'(soneflow Rust binding|MoSQITo): mean=([0-9.]+)',t)
for label,sec in vals:
 ms={'soneflow Rust binding':9.25,'MoSQITo':7168}[label];got=float(sec)*1000;rows.append(dict(id=label,readme_ms=ms,measured_ms=got,percent=(got/ms-1)*100,within_20=abs(got/ms-1)<=.2))
(E/'readme-comparison.json').write_text(json.dumps(rows,indent=2));print(json.dumps(rows,indent=2))
