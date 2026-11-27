import pathlib,json,re
p=pathlib.Path(__file__).parent
rows=[]
for f in ['results.json','results-stage3.json','results-stage3-retry.json','results-header-retry.json']:
    for r in json.loads((p/f).read_text()):
        r['ledger']=f; r['log']=r['name']+'.log'; rows.append(r)
rows += [dict(name='stage3-raw-header-assert',exit=1,evidence='r1-check.json',reason='CRLF working tree vs Git LF; final resolution r1-header-final.json'),dict(name='stage3-first-header-retry-assert',exit=1,evidence='r1-header-normalization.json',reason='Two CRLF header-literal lines from working-tree config; final resolution r1-header-final.json'),dict(name='header-lf-config-final-assert',exit=0,evidence='r1-header-final.json'),dict(name='initial-docs-redaction',args=['python','tools/redact_paths.py','--check','docs'],exit=0,observed='clean before new evidence created'),dict(name='prep-venv',exit=0,log='21-prep-venv.log'),dict(name='prep-pip',exit=0,log='22-prep-pip.log')]
(p/'all-command-index.json').write_text(json.dumps(rows,indent=2),encoding='utf-8')
counts={}
for f in p.glob('*.log'):
    matches=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',f.read_text(encoding='utf-8'))
    if matches: counts[f.name]={'passed':sum(int(x[0]) for x in matches),'failed':sum(int(x[1]) for x in matches),'ignored_not_executed':sum(int(x[2]) for x in matches)}
(p/'rust-test-counts.json').write_text(json.dumps(counts,indent=2))
print(json.dumps(counts,indent=2))
