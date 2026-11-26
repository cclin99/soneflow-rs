from verify import *
import importlib.util
spec=importlib.util.spec_from_file_location('api',C/'tools/public_api.py');api=importlib.util.module_from_spec(spec);spec.loader.exec_module(api)
a=set(api.read_list(C/'soneflow/public-api.txt'));b=set(api.collect(B/'target/doc/soneflow'))
d={'baseline_items':len(b),'candidate_items':len(a),'added':sorted(a-b),'removed':sorted(b-a)}
(E/'api-diff.json').write_text(json.dumps(d,indent=2));print('baseline',len(b),'candidate',len(a),'added',len(a-b),'removed',len(b-a));print('\n'.join(sorted(a-b)))
print('removed categories',sorted(set(x.split('soneflow::',1)[1].split('::')[0]for x in b-a if 'soneflow::' in x)))
