from verify import *
run('environment',['python','-c','import sys,platform;print(sys.version);print(platform.platform())'])
run('rustc',['rustc','-vV'])
run('cbindgen-version',['cbindgen','--version'])
run('venv',['python','-m','venv',str(ROOT/'venv')])
run('pip',['python','-m','pip','--version'])
