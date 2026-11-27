import pathlib
exec((pathlib.Path(__file__).parent/'run_gates.py').read_text(encoding='utf-8-sig').split("run('00-versions'")[0].replace('results.json','results-header-retry.json'))
import hashlib
config=subprocess.check_output(['git','show','HEAD:soneflow-ffi/cbindgen.toml'],cwd=REPO)
(OUT/'cbindgen.lf.toml').write_bytes(config)
run('35-cbindgen-lf-config',['cbindgen','--config',str(OUT/'cbindgen.lf.toml'),'--crate','soneflow-ffi','--output',str(OUT/'soneflow.generated-lf.h')],REPO/'soneflow-ffi')
files={'generated':(OUT/'soneflow.generated.h').read_bytes(),'generated_lf_config':(OUT/'soneflow.generated-lf.h').read_bytes(),'working_tree':(REPO/'soneflow-ffi/include/soneflow.h').read_bytes(),'head_blob':subprocess.check_output(['git','show','HEAD:soneflow-ffi/include/soneflow.h'],cwd=REPO),'baseline_blob':subprocess.check_output(['git','show','82d1d61:soneflow-ffi/include/soneflow.h'],cwd=REPO)}
r={'core_autocrlf':subprocess.check_output(['git','config','--get','core.autocrlf'],cwd=REPO,text=True).strip(),'files':{k:{'sha256':hashlib.sha256(v).hexdigest(),'bytes':len(v),'crlf':v.count(b'\r\n'),'lf':v.count(b'\n')} for k,v in files.items()},'all_equal_after_crlf_normalization':len({v.replace(b'\r\n',b'\n') for v in files.values()})==1,'lf_config_generated_equals_head_and_baseline':files['generated_lf_config']==files['head_blob']==files['baseline_blob']}
(OUT/'r1-header-final.json').write_text(json.dumps(r,indent=2))
assert r['all_equal_after_crlf_normalization'] and r['lf_config_generated_equals_head_and_baseline']
print('R1 PASS: LF config regenerated header exactly matches HEAD and baseline blobs')
