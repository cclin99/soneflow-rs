from verify import *
import re,collections
out={}
for p in (E/'logs').glob('*.log'):
 text=p.read_text(encoding='utf-8',errors='replace');r=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',text)
 if r:out[p.stem]={'passed':sum(int(x[0])for x in r),'failed':sum(int(x[1])for x in r),'ignored':sum(int(x[2])for x in r),'suite_results':r}
(E/'test-summary.json').write_text(json.dumps(out,indent=2));print(json.dumps(out,indent=2))
