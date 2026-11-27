import os, subprocess, pathlib, json, time
ROOT=pathlib.Path('<AUDIT_ROOT>'); REPO=ROOT/'repo'; OUT=REPO/'docs/verification/2026-11-28-soneflow-0.2.1-acceptance-ddbe9e2/gates'
os.environ.update(TEMP=str(ROOT/'tmp'),TMP=str(ROOT/'tmp'),CARGO_HOME=str(ROOT/'cargo-home'),CARGO_TARGET_DIR=str(ROOT/'target-gates'),PYTHONDONTWRITEBYTECODE='1',HASH_GATE_DUMP_ONLY='0',REQUIRE_AVX2='1',PIP_CACHE_DIR=str(ROOT/'pip-cache'),RUSTDOCFLAGS='-D warnings')
results=[]
def run(name,args,cwd=REPO):
    start=time.time()
    with (OUT/(name+'.log')).open('w',encoding='utf-8') as f:
        f.write('COMMAND '+json.dumps(args)+'\nCWD '+str(cwd)+'\n'); f.flush()
        p=subprocess.run(args,cwd=cwd,stdout=f,stderr=subprocess.STDOUT)
        f.write('\nEXIT '+str(p.returncode)+'\n')
    results.append(dict(name=name,args=args,exit=p.returncode,seconds=time.time()-start))
    (OUT/'results.json').write_text(json.dumps(results,indent=2),encoding='utf-8')
    print(name,p.returncode,round(time.time()-start,2),flush=True)
    return p.returncode
prod=['api_errors','hash_gate','l4_batch_fault','nl_zero_init','output_grid','py_contract_dump','simd_dispatch','stream_alloc','stream_fault_rt','stream_no_rayon']
internal=['core_characterization','determinism','low_freq_correction','main_loudness_rounding','simd_parity','stream_contract','stream_scalar','zup_eps']
def tests(xs): return [s for t in xs for s in ['--test',t]]
run('00-versions',['powershell','-NoProfile','-Command','cargo -V; rustc -Vv; cbindgen --version; cargo llvm-cov --version; cargo crap --version; rust-code-analysis-cli --version; python --version'])
run('01-fmt',['cargo','fmt','--all','--check'])
run('02-product-debug',['cargo','test','-p','soneflow','--lib']+tests(prod))
run('03-hash-release',['cargo','test','-p','soneflow','--release','--test','hash_gate'])
run('04-internals-debug',['cargo','test','-p','soneflow','--features','internals']+tests(internal))
for feat in [[],['--features','internals']]:
    tag='default' if not feat else 'internals'
    run('05-clippy-'+tag,['cargo','clippy','-p','soneflow','--all-targets']+feat+['--','-D','warnings'])
    run('06-doctest-'+tag,['cargo','test','-p','soneflow','--doc']+feat)
    run('07-doc-'+tag,['cargo','doc','-p','soneflow','--no-deps']+feat)
run('08-cli',['cargo','test','-p','soneflow','--example','cli'])
run('09-ffi-clippy',['cargo','clippy','-p','soneflow-ffi','--all-targets','--all-features','--','-D','warnings'])
run('10-ffi-test',['cargo','test','-p','soneflow-ffi','--features','test-panic'])
run('11-ffi-release',['cargo','build','-p','soneflow-ffi','--release'])
run('12-py-clippy',['cargo','clippy','-p','soneflow-py','--all-targets','--','-D','warnings'])
run('13-doc-default-api',['cargo','doc','-p','soneflow','--no-deps'])
run('14-public-api',['python','tools/public_api.py','--doc-dir',str(ROOT/'target-gates/doc/soneflow'),'--check'])
run('15-tools-tests',['python','-m','unittest','-v','tools/test_arch_check.py','tools/test_redact_paths.py','tools/test_env_guards.py','tools/test_public_api.py','tools/test_contract_tests.py','tools/test_fnmetrics.py'])
run('16-doc-pairs',['python','tools/doc_pairs.py','--check'])
run('17-contracts',['python','tools/contract_tests.py'])
run('18-fnmetrics',['python','tools/fnmetrics.py','--check'])
run('19-cbindgen',['cbindgen','--config','cbindgen.toml','--crate','soneflow-ffi','--output',str(OUT/'soneflow.generated.h')],REPO/'soneflow-ffi')
run('20-frozen-diff',['git','diff','82d1d61','ddbe9e2','--','soneflow/public-api.txt','soneflow-ffi/include/soneflow.h','soneflow/tests/hash_gate.rs','soneflow-py/tests/test_smoke.py','soneflow-py/tests/test_stream.py','soneflow-py/tests/test_quality_v1.py'])
