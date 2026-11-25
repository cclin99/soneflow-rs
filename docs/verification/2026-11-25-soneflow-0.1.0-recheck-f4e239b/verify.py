from pathlib import Path
import subprocess,json,hashlib,re,os,datetime
root=Path.cwd(); out=root/'docs/verification/2026-11-25-soneflow-0.1.0-recheck-f4e239b'; out.mkdir(parents=True,exist_ok=True); (out/'logs').mkdir(exist_ok=True)
def run(name,args,env=None):
 p=subprocess.run(args,cwd=root,capture_output=True,env=env); (out/'logs'/f'{name}.txt').write_bytes(p.stdout+p.stderr)
 with (out/'commands.jsonl').open('a',encoding='utf8') as f:f.write(json.dumps({'name':name,'argv':args,'exit':p.returncode})+'\n')
 print(name,p.returncode,flush=True); return p
run('ci',['gh','run','view','45519392033','--repo','cclin99/soneflow-rs','--json','headSha,status,conclusion,jobs,url'])
run('remote',['git','ls-remote','origin','refs/heads/main'])
run('history',['git','log','origin/main','--format=%H %P %ae %ce'])
run('cbindgen-version',['cbindgen','--version'])
run('header',['cbindgen','--config','soneflow-ffi/cbindgen.toml','--crate','soneflow-ffi','--output',str(out/'soneflow.generated.h')])
a=(out/'soneflow.generated.h').read_bytes(); b=(root/'soneflow-ffi/include/soneflow.h').read_bytes(); print('header equal',a==b)
(out/'header-check.json').write_text(json.dumps({'byte_equal':a==b,'generated_sha256':hashlib.sha256(a).hexdigest(),'tracked_worktree_sha256':hashlib.sha256(b).hexdigest()},indent=2))
env=os.environ.copy();env['REQUIRE_AVX2']='1';env['HASH_GATE_DUMP_ONLY']='1'
args=['cargo','test','-p','soneflow','--lib']
run('core-lib',args,env)
run('doctest',['cargo','test','-p','soneflow','--doc'],env)
args=['cargo','test','-p','soneflow']
for t in 'api_errors core_characterization determinism hash_gate low_freq_correction nl_zero_init output_grid py_contract_dump simd_dispatch simd_parity stream_alloc stream_no_rayon stream_scalar zup_eps'.split():args+=['--test',t]
run('integrations',args,env)
run('validator',['cargo','test','-p','soneflow','--test','annexb_validator','--','--skip','csv_'],env)
run('ffi',['cargo','test','-p','soneflow-ffi','--features','test-panic'],env)
