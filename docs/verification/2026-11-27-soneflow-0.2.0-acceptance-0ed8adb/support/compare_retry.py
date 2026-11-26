from verify import *
V=str(ROOT/'venv/Scripts/python.exe')
run('matplotlib-install',[V,'-m','pip','install','matplotlib==3.11.0'])
run('compare-30-ready',[V,'tools/compare_mosqito.py','--iterations','30'])
run('compare-tail-5-ready',[V,'tools/compare_mosqito.py','--iterations','5','--nl-init','mosqito_tail'])
run('compare-tables-ready',[V,'tools/compare_mosqito.py','--check-tables'])
run('pip-freeze-final',[V,'-m','pip','freeze'])
