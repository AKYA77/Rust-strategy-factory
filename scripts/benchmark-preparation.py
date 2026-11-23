#!/usr/bin/env python3
"""Fresh-process synthetic CLI pipeline comparison; never historical acceptance."""
import argparse, hashlib, json, os, platform, resource, statistics, subprocess, sys, tempfile, time
from pathlib import Path

def command(binary,*args):
    r=subprocess.run([str(binary),*map(str,args)],capture_output=True,text=True,check=True)
    return json.loads(r.stdout) if r.stdout.strip() else None

def receipts(value):
    if isinstance(value,dict):
        if 'tick_replay' in value:return [value['tick_replay']]
        return [x for v in value.values() for x in receipts(v)]
    if isinstance(value,list):return [x for v in value for x in receipts(v)]
    return []

def fingerprint(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()

if len(sys.argv)>1 and sys.argv[1]=='--worker':
    _,_,role,binary,config,database,output=sys.argv
    start=time.perf_counter_ns()
    report=command(binary,'corpus-batch',config,database,'100')
    counts=command(binary,'registry-check',database)
    elapsed=time.perf_counter_ns()-start
    cpu=resource.getrusage(resource.RUSAGE_CHILDREN)
    values=receipts(report)
    assert len(values)==len(json.loads(Path(config).read_text())['strategies']),(role,report)
    trades=sum(len(v['trades']) for v in values)
    assert trades>0
    cached=command(binary,'corpus-batch',config,database,'100')
    assert fingerprint(receipts(cached))==fingerprint(values)
    assert command(binary,'registry-check',database)==counts
    Path(output).write_text(json.dumps({'role':role,'elapsed_ms':elapsed/1e6,'child_user_seconds':cpu.ru_utime,'child_system_seconds':cpu.ru_stime,'peak_pipeline_child_rss_kib':cpu.ru_maxrss,'financial_receipt_sha256':fingerprint(values),'resolved_trades':trades,'counts':counts,'preparation':report.get('last',{}).get('preparation'),'cached_preparation':cached.get('last',{}).get('preparation'),'cached_receipt_and_counts_match':True}))
    raise SystemExit(0)

parser=argparse.ArgumentParser()
parser.add_argument('before',type=Path);parser.add_argument('after',type=Path);parser.add_argument('output',type=Path)
parser.add_argument('--new-chunks',type=int,default=64)
parser.add_argument('--workers',type=int,default=1)
parser.add_argument('--strategies',type=int,choices=[1,2],default=2)
parser.add_argument('--chunks',type=int,default=2)
parser.add_argument('--format',choices=['native','blocked','mixed'],default='native')
parser.add_argument('--timeframe',choices=['M1','M5','M15'],default='M1')
parser.add_argument('--repetitions',type=int,default=3);parser.add_argument('--quotes',type=int,default=20000)
a=parser.parse_args()
assert 1<=a.repetitions<=10 and 1000<=a.quotes<=1000000 and 1<=a.workers<=3 and 1<=a.new_chunks<=64 and 2<=a.chunks<=32
if a.output.exists():raise SystemExit('output must be new')
assert (a.quotes+a.chunks-1)//a.chunks<=100000, 'keep every artifact within native admission cap'
before=a.before.resolve();after=a.after.resolve()
with tempfile.TemporaryDirectory(prefix='preparation-benchmark-') as tmp:
    base=Path(tmp)
    profile=next(p for p in command(after,'profiles') if p['symbol']=='EURUSD')
    (base/'profile.json').write_text(json.dumps(profile))
    artifacts=[]
    for chunk in range(a.chunks):
        rows=[]
        for index in range(chunk*a.quotes//a.chunks,(chunk+1)*a.quotes//a.chunks):
            phase=index%240;bid=100000+(phase if phase<120 else 240-phase)*10;ask=bid+10
            rows.append(f'{1735689600000+index*1000},{index+1},{bid/100000:.5f},{ask/100000:.5f}')
        csv=base/f'{chunk}.csv';csv.write_text('timestamp_ms,sequence,bid,ask\n'+'\n'.join(rows)+'\n')
        dataset=base/f'{chunk}.json';native=base/f'{chunk}.rsf'
        command(after,'import',csv,base/'profile.json','SYNTHETIC_PREPARATION_BENCHMARK_ONLY',dataset,'synthetic')
        if a.format=='blocked' or (a.format=='mixed' and chunk%2==1):
            native=base/f'{chunk}.rsfb';command(after,'pack-blocks',dataset,native,256)
        else:command(after,'pack-native',dataset,native)
        artifacts.append(native)
    command(after,'corpus-create',base/'corpus.json',*artifacts)
    strategies=[]
    for entry in [None,{'StopMarket':{'offset_ticks':2,'ttl_ms':120000,'cancel_on_signal_false':True}}][:a.strategies]:
        strategies.append({'timeframe':a.timeframe,'side':'Long','stop_ticks':100,'entry':entry,'rule':{'UtcSession':{'start_hour':0,'end_hour':23,'weekdays':127}}})
    config=base/'config.json';config.write_text(json.dumps({'specification':'corpus.json','strategies':strategies,'budget':{'max_candles':100000,'feature_cells':1000000,'max_receipts':10000},'limits':{'workers':a.workers,'max_parallel_candles':100000*a.workers,'max_parallel_feature_cells':1000000*a.workers,'max_parallel_receipts':10000*a.workers,'max_new_chunks':a.new_chunks,'lease_ms':600000}}))
    records=[]
    for repetition in range(a.repetitions):
        for role in (['before','after'] if repetition%2==0 else ['after','before']):
            result=base/f'{role}-{repetition}.json';database=base/f'{role}-{repetition}.sqlite';binary=before if role=='before' else after
            subprocess.run([sys.executable,str(Path(__file__).resolve()),'--worker',role,str(binary),str(config),str(database),str(result)],check=True)
            row=json.loads(result.read_text());row['repetition']=repetition;records.append(row)
    assert len({r['financial_receipt_sha256'] for r in records})==1
    summaries={role:{'median_elapsed_ms':statistics.median(r['elapsed_ms'] for r in records if r['role']==role),'median_peak_pipeline_child_rss_kib':statistics.median(r['peak_pipeline_child_rss_kib'] for r in records if r['role']==role)} for role in ['before','after']}
    evidence={'class':'SYNTHETIC_DEVELOPMENT_PIPELINE_ONLY','host':{'platform':platform.platform(),'available_parallelism':len(os.sched_getaffinity(0))},'quotes':a.quotes,'artifacts':a.chunks,'artifact_format':a.format,'strategy_timeframe':a.timeframe,'max_new_chunks_per_round':a.new_chunks,'workers':a.workers,'strategies':a.strategies,'repetitions':a.repetitions,'cache_scope':'Fresh worker processes and registries; OS cache uncontrolled, no physical-cold claim. Cached route checked separately and excluded from measured first-route timing/CPU/RSS.','measurement_scope':'Each timed pipeline includes actual corpus-batch CLI and registry integrity CLI. CPU and RSS are child-resource observations in a fresh worker, not total host/service memory.','versions':{role:{'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'version':command(binary,'version')} for role,binary in [('before',before),('after',after)]},'records':records,'summaries':summaries,'limitations':'Small one-symbol synthetic workload; declared worker count is recorded, not an equal CPU-share claim. Repetitions are repeated measurements of the selected fixed strategies, not independent trials. No genuine data, source approval, qualification, production default or universal throughput/RSS gain is established.'}
    a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(evidence,indent=2)+'\n')
    print(json.dumps(summaries))
