from verify import *
import re,tomllib,hashlib
versions={}
for p in ['soneflow/Cargo.toml','soneflow-ffi/Cargo.toml','soneflow-py/Cargo.toml']:
 versions[p]=tomllib.loads((C/p).read_text())['package']['version']
versions['Cargo.lock']={p['name']:p['version']for p in tomllib.loads((C/'Cargo.lock').read_text())['package']if p['name'] in ['soneflow','soneflow-ffi','soneflow-py']}
versions['CITATION.cff']=re.search(r'^version: (.*)',(C/'CITATION.cff').read_text(),re.M).group(1)
(E/'versions.json').write_text(json.dumps(versions,indent=2))
ci=(C/'.github/workflows/ci.yml').read_text();allowed='api_errors hash_gate l4_batch_fault nl_zero_init output_grid py_contract_dump simd_dispatch stream_alloc stream_fault_rt stream_no_rayon core_characterization determinism low_freq_correction simd_parity stream_contract stream_scalar zup_eps'.split()
(E/'ci-test-inventory.json').write_text(json.dumps({'data_free':{x:('--test '+x in ci)for x in allowed},'excluded':'annexb annexb_validator golden_core golden_dsp golden_zwst golden_zwtv stream_golden'.split(),'all_test_files':[p.stem for p in (C/'soneflow/tests').glob('*.rs')],'source_audit':'No data read calls in included test files; shared common readers have no calls from included suites. zup_eps uses own synthetic frozen fixture; provenance inspected.'},indent=2))
rows=[('48 kHz','api_errors',4,'UnsupportedSampleRate equality'),('minimum input','output_grid',59,'4800 succeeds with 50 points; 4799 rejects'),('input precedence','api_errors',94,'rate error before NaN and short input before NaN'),('first nonfinite','api_errors',83,'first offending index 50 asserted; code 4 at line 54'),('exact output times','output_grid',78,'time to_bits equals j*96/48000'),('processor reuse','nl_zero_init',159,'fresh and reused complete output compared bitwise'),('chunk invariance','stream_contract',61,'fixed and LCG chunks compare n,phon,index,flags'),('warmup and latency','stream_contract',34,'24 samples; every frame warmup predicate and 580 constant'),('reset','stream_contract',84,'reset output matches newly constructed stream bitwise'),('MXCSR restoration','stream_contract',214,'control bits asserted; exception status bits masked. Source guard restores saved register. Narrow coverage is parked, no observed violation.')]
(E/'contract-sample.json').write_text(json.dumps([dict(promise=a,path='soneflow/tests/'+b+'.rs',line=c,evidence=d,status='consistent',limitation='Sampled assertions are examples rather than exhaustive input-space proof')for a,b,c,d in rows],indent=2))
for name in ['candidate','baseline']:
 snap=json.loads((ROOT/(name+'-snapshot.json')).read_text());(E/(name+'-snapshot.json')).write_text(json.dumps({'commit':snap['commit'],'files':snap['files'],'excluded':['data/','private/','docs/verification/']},indent=2))
