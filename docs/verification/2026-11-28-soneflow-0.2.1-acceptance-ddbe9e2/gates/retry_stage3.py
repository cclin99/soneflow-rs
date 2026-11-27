import pathlib
exec((pathlib.Path(__file__).parent/'run_gates.py').read_text(encoding='utf-8-sig').split("run('00-versions'")[0].replace('results.json','results-stage3-retry.json'))
import hashlib
p=OUT/'smoke.cmd'
s=p.read_text().replace('smoke.exe\n','"<AUDIT_ROOT>\\smoke-gates\\smoke.exe"\n')
(OUT/'smoke-retry.cmd').write_text(s)
run('34-ffi-c-smoke-retry',['cmd','/c',str(OUT/'smoke-retry.cmd')],ROOT/'smoke-gates')
a=(OUT/'soneflow.generated.h').read_bytes(); b=(REPO/'soneflow-ffi/include/soneflow.h').read_bytes()
h=subprocess.check_output(['git','show','HEAD:soneflow-ffi/include/soneflow.h'],cwd=REPO)
c=subprocess.check_output(['git','show','82d1d61:soneflow-ffi/include/soneflow.h'],cwd=REPO)
r={'generated_equals_head_blob':a==h,'head_equals_baseline_blob':h==c,'working_tree_normalized_crlf_equals_head':b.replace(b'\r\n',b'\n')==h,'generated_sha256':hashlib.sha256(a).hexdigest(),'head_blob_sha256':hashlib.sha256(h).hexdigest(),'baseline_blob_sha256':hashlib.sha256(c).hexdigest(),'working_tree_sha256':hashlib.sha256(b).hexdigest()}
(OUT/'r1-header-normalization.json').write_text(json.dumps(r,indent=2))
assert a==h==c and b.replace(b'\r\n',b'\n')==h
print('R1 generated header exactly equals HEAD and baseline Git blobs; working tree differs only CRLF',flush=True)
