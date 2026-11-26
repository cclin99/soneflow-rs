from verify import *
V=str(ROOT/'venv/Scripts/python.exe');os.environ['PYO3_PYTHON']=V
run('wheel',[str(ROOT/'venv/Scripts/maturin.exe'),'build','--release','--out',str(ROOT/'wheels'),'-m','soneflow-py/Cargo.toml'])
wh=list((ROOT/'wheels').glob('*.whl'))
if wh:
 run('wheel-install',[V,'-m','pip','install',str(wh[0])]);run('python-tests',[V,'-m','pytest','soneflow-py/tests/test_smoke.py','soneflow-py/tests/test_stream.py','soneflow-py/tests/test_quality_v1.py','-v'])
run('tools-tests',['python','-m','unittest','-v','tools/test_arch_check.py','tools/test_redact_paths.py','tools/test_env_guards.py','tools/test_public_api.py','tools/test_contract_tests.py'])
for name,feat in [('clippy-default',[]),('clippy-internals',['--features','internals'])]:run(name,['cargo','clippy','-p','soneflow','--all-targets']+feat+['--','-D','warnings'])
run('clippy-ffi',['cargo','clippy','-p','soneflow-ffi','--all-targets','--all-features','--','-D','warnings'])
run('clippy-py',['cargo','clippy','-p','soneflow-py','--all-targets','--','-D','warnings'])
for name,feat in [('rustdoc-default',[]),('rustdoc-internals',['--features','internals'])]:
 env=os.environ.copy();env['RUSTDOCFLAGS']='-D warnings';run(name,['cargo','doc','-p','soneflow','--no-deps']+feat,env=env)
