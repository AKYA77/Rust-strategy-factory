#!/usr/bin/env python3
"""Actual reversible rebuild checks; development only, no production inputs."""
from pathlib import Path
import hashlib,json,subprocess,sys
root=Path(__file__).resolve().parent.parent
if len(sys.argv)!=2:
    raise SystemExit('usage: python3 scripts/check-data-compute-identity.py <new-evidence.json>')
output=Path(sys.argv[1])
if output.exists():
    raise SystemExit('evidence output must be new')
exe=root/'target/debug/rust-strategy-factory'
def build():
    subprocess.run(['cargo','build','--locked'],cwd=root,check=True,stdout=subprocess.DEVNULL)
    return json.loads(subprocess.check_output([str(exe),'version'],cwd=root))['compute_revision']
base=build()
records=[]
# These inputs cover admission, permissions, eligibility and streaming paths.
# Changing a comment deliberately exercises byte-level source fingerprinting.
for name in ['src/catalog.rs','src/machine_evidence.rs','src/review_authority.rs','src/data_approval.rs','src/corpus.rs','web/style.css']:
    path=root/name
    original=path.read_bytes()
    changed=(original+b'\n/* reversible compute identity fixture */\n') if name.endswith('.css') else (original+b'\n// reversible compute identity fixture\n')
    try:
        path.write_bytes(changed)
        identity=build()
        expected_change=name.endswith('.rs')
        if (identity!=base)!=expected_change:
            raise RuntimeError('source identity boundary failed: '+name)
        records.append({'path':name,'original_sha256':hashlib.sha256(original).hexdigest(),'probe_sha256':hashlib.sha256(changed).hexdigest(),'identity_changed':identity!=base,'identity':identity})
    finally:
        path.write_bytes(original)
        restored=build()
        if restored!=base:
            raise RuntimeError('original compute identity was not restored: '+name)
with output.open('x') as f:
    json.dump({'class':'SYNTHETIC_DEVELOPMENT_REBUILD_ONLY','baseline_identity':base,'checks':records,'all_original_bytes_and_identity_restored':True,'limitations':'Actual debug rebuild source fingerprint checks only; no market data, simulations, throughput or scientific eligibility. CSS changes stay outside compute identity. No background processes or external services.'},f,indent=2)
    f.write('\n')
print(json.dumps({'passed':len(records),'originals_restored':True}))
