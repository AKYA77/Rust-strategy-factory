#!/usr/bin/env python3
"""Measure complete synthetic registered corpus sessions, including prefix recovery."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

root=Path(__file__).resolve().parent.parent
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('output',type=Path)
parser.add_argument('--binary',type=Path,default=root/'target/release/rust-strategy-factory')
args=parser.parse_args()
binary=args.binary.resolve()
def command(*values):
    out=subprocess.run([str(binary),*map(str,values)],capture_output=True,text=True,check=True)
    return json.loads(out.stdout) if out.stdout.strip() else None

def read(path):
    try:return Path(path).read_text().strip()
    except OSError:return 'unavailable'

source_manifest=[{'path':str(p.relative_to(root)),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted([root/'build.rs',root/'Cargo.toml',root/'Cargo.lock',root/'rust-toolchain.toml',*root.glob('src/**/*.rs')])]
with tempfile.TemporaryDirectory(prefix='corpus-workers-') as temp:
    directory=Path(temp)
    profile=next(p for p in command('profiles') if p['symbol']=='EURUSD')
    profile_file=directory/'profile.json';profile_file.write_text(json.dumps(profile))
    native=[]
    for chunk in range(4):
        csv=directory/f'{chunk}.csv';dataset=directory/f'{chunk}.json';artifact=directory/f'{chunk}.rsf'
        rows=[]
        for i in range(1000):
            event=chunk*1000+i;bid=100000+(event%240 if (event//240)%2==0 else 240-event%240)
            rows.append(f'{1735689600000+event*60000},{18446744073709547000+event},{bid/100000:.5f},{(bid+10)/100000:.5f}\n')
        csv.write_text('timestamp_ms,sequence,bid,ask\n'+''.join(rows))
        command('import',csv,profile_file,'SYNTHETIC_TEST_ONLY worker comparison',dataset,'synthetic')
        command('pack-native',dataset,artifact);native.append(artifact)
    spec=directory/'corpus.json';command('corpus-create',spec,*native)
    observed=command('corpus-inspect',spec)
    strategies=[{'timeframe':'M1','side':'Long','stop_ticks':n,'rule':{'AboveMean':{'window':2}}} for n in range(80,160,10)]
    trials=[];expected=None
    # Rotate counts rather than measuring every1-worker trial before all4-worker trials.
    for repeat in range(3):
        for workers in [1,2,4][repeat:]+[1,2,4][:repeat]:
            config=directory/f'config-{repeat}-{workers}.json';db=directory/f'run-{repeat}-{workers}.sqlite';out=directory/'stdout.json';err=directory/'stderr.txt'
            config.write_text(json.dumps({'specification':'corpus.json','strategies':strategies,'budget':{'max_candles':10000,'feature_cells':100000,'max_receipts':10000},'limits':{'workers':workers,'max_parallel_candles':workers*10000,'max_parallel_feature_cells':workers*100000,'max_parallel_receipts':workers*10000,'max_new_chunks':1,'lease_ms':600000}}))
            start=time.monotonic()
            with out.open('w') as stdout,err.open('w') as stderr:
                child=subprocess.Popen([str(binary),'corpus-batch',str(config),str(db),'10'],stdout=stdout,stderr=stderr)
                _,status,usage=os.wait4(child.pid,0);child.returncode=os.waitstatus_to_exitcode(status)
            elapsed=time.monotonic()-start
            if child.returncode:raise RuntimeError(err.read_text())
            result=json.loads(out.read_text());assert result['stop_reason']=='complete',result
            receipts=[j['Ok']['Completed']['result'] for j in result['last']['jobs']]
            hashes=[r['result_hash'] for r in receipts]
            if expected is None:expected=hashes
            assert hashes==expected,'worker count changed complete receipt hashes'
            trades=sum(len(r['result']['tick_replay']['trades']) for r in receipts);assert trades>0
            trials.append({'repeat':repeat,'workers':workers,'rounds':result['rounds'],'jobs':len(receipts),'resolved_trades':trades,'wall_seconds':elapsed,'user_cpu_seconds':usage.ru_utime,'system_cpu_seconds':usage.ru_stime,'peak_rss_kib_linux':usage.ru_maxrss,'filesystem_input_blocks':usage.ru_inblock,'filesystem_output_blocks':usage.ru_oublock})
    report={'version':'corpus-workers-v1','recorded_at_utc':datetime.now(timezone.utc).isoformat(),'git_base_revision':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'source_dirty_at_measurement':bool(subprocess.check_output(['git','status','--porcelain'],cwd=root)),'source_manifest':source_manifest,'measurement_script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'executable_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cpu_model':next((s.split(':',1)[1].strip() for s in read('/proc/cpuinfo').splitlines() if s.startswith('model name')),'unavailable'),'affinity_cpu_count':len(os.sched_getaffinity(0)),'cgroup_cpu_max':read('/sys/fs/cgroup/cpu.max'),'cgroup_memory_max':read('/sys/fs/cgroup/memory.max'),'corpus_identity':observed['corpus_identity'],'ordered_events_hash':observed['ordered_events_hash'],'events':observed['events'],'chunks':observed['chunks'],'receipt_hashes':expected,'trials':trials,'median_wall_seconds':{str(w):statistics.median(t['wall_seconds'] for t in trials if t['workers']==w) for w in [1,2,4]},'limitations':'Synthetic4000-event EURUSD corpus,8 strategies,4 checkpoint rounds; complete CLI sessions include registration, decoding, feature preparation, prefix recomputation, SQLite writes and publication. Per-child Linux wait4 resources exclude compilation/source construction. Warm uncontrolled page cache and small fixture; no representative throughput, physical-cold, real/full-history, optimal production worker count or hard resource enforcement claim.'}
    with args.output.open('x') as output:json.dump(report,output,indent=2);output.write('\n')
