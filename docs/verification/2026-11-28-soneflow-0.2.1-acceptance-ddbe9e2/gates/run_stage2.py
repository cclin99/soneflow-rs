import pathlib
exec((pathlib.Path(__file__).parent/'run_gates.py').read_text(encoding='utf-8-sig').split("run('00-versions'")[0])
# Preserve stage 1 results; stage 2 has its own ledger.
results=json.loads((OUT/'results.json').read_text())
vp=ROOT/'venv-gates/Scripts/python.exe'
run('21-venv',['python','-m','venv',str(ROOT/'venv-gates')])
run('22-pip',[str(vp),'-m','pip','install','maturin','pytest','numpy'])
run('23-wheel',[str(vp),'-m','maturin','build','--release','--out',str(ROOT/'wheels-gates')],REPO/'soneflow-py')
wheels=list((ROOT/'wheels-gates').glob('*.whl'))
if wheels:
    run('24-wheel-install',[str(vp),'-m','pip','install',str(wheels[0])])
    run('25-python-contract',[str(vp),'-m','pytest','-p','no:cacheprovider','tests/test_smoke.py','tests/test_stream.py','tests/test_quality_v1.py','-v'],REPO/'soneflow-py')
os.environ['CARGO_TARGET_DIR']=str(ROOT/'target-coverage-gates')
run('26-coverage-clean',['cargo','llvm-cov','clean','--workspace'])
run('27-coverage-soneflow',['cargo','llvm-cov','--no-report','--release','-p','soneflow','--features','internals','--lib']+tests(prod+internal))
run('28-coverage-ffi',['cargo','llvm-cov','--no-report','--release','-p','soneflow-ffi'])
run('29-lcov',['cargo','llvm-cov','report','--release','--lcov','--output-path',str(OUT/'lcov.info')])
run('30-crap',['cargo','crap','-p','soneflow','-p','soneflow-ffi','--lcov',str(OUT/'lcov.info'),'--format','json','--output',str(OUT/'crap.json')])
run('31-arch',['python','tools/arch_check.py','--crap',str(OUT/'crap.json')])
