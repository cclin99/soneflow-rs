import pathlib,json,subprocess,os,time
ROOT=pathlib.Path(__file__).parent
C=ROOT/'candidate'; B=ROOT/'baseline'; E=C/'docs/verification/2026-11-26-soneflow-0.2.0-acceptance-0ed8adb';(E/'logs').mkdir(parents=True,exist_ok=True)
os.environ['HASH_GATE_DUMP_ONLY']='0';os.environ['REQUIRE_AVX2']='1'
def run(name,args,cwd=C,env=None):
 t=time.time(); log=(E/'logs'/f'{name}.log').open('wb'); p=subprocess.run(args,cwd=cwd,env=env,stdout=log,stderr=subprocess.STDOUT);log.close()
 with (E/'commands.jsonl').open('a',encoding='utf-8') as f:f.write(json.dumps(dict(name=name,args=args,cwd=str(cwd),exit=p.returncode,seconds=time.time()-t))+'\n')
 print(name,p.returncode,round(time.time()-t,1),flush=True);return p
if __name__=='__main__':
 product='api_errors hash_gate l4_batch_fault nl_zero_init output_grid py_contract_dump simd_dispatch stream_alloc stream_fault_rt stream_no_rayon'.split()
 internal='core_characterization determinism low_freq_correction simd_parity stream_contract stream_scalar zup_eps'.split()
 for name,args,cwd in [('fmt',['cargo','fmt','--check'],C),('lib-default',['cargo','test','--lib'],C/'soneflow'),('lib-internals',['cargo','test','--lib','--features','internals'],C/'soneflow'),('product',['cargo','test']+sum([['--test',x] for x in product],[]),C/'soneflow'),('internals',['cargo','test','--features','internals']+sum([['--test',x] for x in internal],[]),C/'soneflow'),('doc-default',['cargo','test','--doc'],C/'soneflow'),('doc-internals',['cargo','test','--doc','--features','internals'],C/'soneflow'),('cli',['cargo','test','--example','cli'],C/'soneflow'),('ffi',['cargo','test','--features','test-panic'],C/'soneflow-ffi'),('doc-pairs',['python','tools/doc_pairs.py','--check'],C),('contracts',['python','tools/contract_tests.py'],C),('redact-docs',['python','tools/redact_paths.py','--check','docs'],C),('fnmetrics',['python','tools/fnmetrics.py','--check','--out',str(E/'fnmetrics.json')],C),('api',['python','tools/public_api.py','--check'],C)]:run(name,args,cwd)

