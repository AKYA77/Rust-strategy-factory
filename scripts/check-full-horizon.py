#!/usr/bin/env python3
"""Synthetic continuous UTC horizon/cap check, not actual quote coverage or science."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import resource
import subprocess
import sys
import tempfile
import time

START = 1735689600000
ROWS = 730 * 24 * 60
WIDTHS = {'M1': 1, 'M5': 5, 'M15': 15, 'M30': 30, 'H1': 60, 'H4': 240, 'D1': 1440}
COUNTS = {key: (ROWS - 1) // width for key, width in WIDTHS.items()}
TOTAL = sum(COUNTS.values())
CAP = 2_000_000
SOURCE = 'SYNTHETIC_CONTINUOUS_UTC_MINUTE_QUOTES_ONLY_NOT_GENUINE'


def run(binary, *args):
    result = subprocess.run([str(binary), *map(str, args)], capture_output=True, text=True, check=True)
    return json.loads(result.stdout) if result.stdout.strip() else None


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def flat_receipt(value):
    assert int(value['initial_micro_usd']) == 20_000_000
    assert int(value['realized_micro_usd']) == int(value['initial_micro_usd'])
    assert int(value['final_marked_micro_usd']) == 20_000_000
    assert value['trades'] == value['entry_receipts'] == value['rejections'] == []
    assert value['unresolved'] is None and value['pending_entry'] is None


if len(sys.argv) > 1 and sys.argv[1] == '--worker':
    _, _, role, binary, root, output = sys.argv
    base = Path(root)
    start = time.perf_counter_ns()
    if role == 'inventory':
        value = run(binary, 'corpus-inspect', base / 'corpus.json')
        assert value['events'] == ROWS and value['source'] == SOURCE
        assert value['declared_class'] == 'SYNTHETIC_TEST_ONLY'
        assert {v['timeframe']: v['closed_candles'] for v in value['candles']} == COUNTS
        assert value['max_resident_quote_events'] <= 32768
        assert int(value['first']['timestamp_ms']) == START
        assert int(value['latest']['timestamp_ms']) == START + (ROWS - 1) * 60000
    elif role == 'source-facts':
        value = run(binary, 'corpus-source-facts', base / 'corpus.json')
        expected = json.loads((base / 'receipt-inventory.json').read_text())['inventory']
        for key in ['corpus_identity', 'profile', 'declared_class', 'source', 'events', 'ordered_events_hash', 'first', 'latest', 'max_resident_quote_events']:
            assert value[key] == expected[key]
        assert 'candles' not in value
    elif role == 'aggregate-refusal':
        destination = base / 'denied.json'
        failed = subprocess.run([binary, 'corpus-replay', str(base / 'corpus.json'), str(base / 'strategy.json'), str(TOTAL - 1), '8000000', '1', str(destination)], capture_output=True, text=True)
        assert failed.returncode != 0 and 'aggregate corpus candle budget' in failed.stderr
        assert not destination.exists()
        value = {'budget': TOTAL - 1, 'result_published': False}
    elif role == 'scalar':
        destination = base / 'scalar.json'
        run(binary, 'corpus-replay', base / 'corpus.json', base / 'strategy.json', TOTAL, 8000000, 1, destination)
        value = json.loads(destination.read_text())
        flat_receipt(value)
    else:
        report = run(binary, 'corpus-batch', base / 'batch.json', base / 'state.sqlite', '100')
        assert report['stop_reason'] == 'complete'
        completed = report['last']['jobs'][0]['Ok']['Completed']
        assert completed['cached'] == (role == 'cached')
        value = completed['result']['result']['tick_replay']
        flat_receipt(value)
        assert value == json.loads((base / 'scalar.json').read_text())
        assert report['rounds'] == (1 if role == 'cached' else 5)
        assert report['last']['preparation']['builds'] == 0
        if role == 'resumed':
            assert report['last']['preparation']['reuses'] == 1
        assert run(binary, 'registry-check', base / 'state.sqlite')['completed'] == 1
    elapsed = time.perf_counter_ns() - start
    observed = resource.getrusage(resource.RUSAGE_CHILDREN)
    receipt = {'role': role, 'elapsed_ms': elapsed / 1e6,
               'child_user_seconds': observed.ru_utime,
               'child_system_seconds': observed.ru_stime,
               'peak_pipeline_child_rss_kib': observed.ru_maxrss,
               'value_sha256': fingerprint(value)}
    if role == 'inventory':
        receipt['inventory'] = value
    Path(output).write_text(json.dumps(receipt))
    raise SystemExit(0)

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
a = parser.parse_args()
if a.output.exists():
    raise SystemExit('evidence output must be new')
binary = a.binary.resolve()
with tempfile.TemporaryDirectory(prefix='factory-full-horizon-') as temp:
    base = Path(temp)
    profile = next(p for p in run(binary, 'profiles') if p['symbol'] == 'EURUSD')
    (base / 'profile.json').write_text(json.dumps(profile))
    chunks = []
    for begin in range(0, ROWS, 32768):
        csv = base / f'{begin}.csv'
        with csv.open('w') as stream:
            stream.write('timestamp_ms,sequence,bid,ask\n')
            for index in range(begin, min(begin + 32768, ROWS)):
                stream.write(f'{START + index * 60000},{index},1.00000,1.00010\n')
        dataset = base / f'{begin}.json'
        native = base / f'{begin}.rsf'
        run(binary, 'import', csv, base / 'profile.json', SOURCE, dataset, 'synthetic')
        run(binary, 'pack-native', dataset, native)
        chunks.append(native)
    assert len(chunks) == 33
    run(binary, 'corpus-create', base / 'corpus.json', *chunks)
    strategy = {'timeframe': 'M1', 'side': 'Long', 'stop_ticks': 100, 'rule': {'AboveMean': {'window': 2}}}
    (base / 'strategy.json').write_text(json.dumps(strategy))
    (base / 'batch.json').write_text(json.dumps({'specification': 'corpus.json', 'strategies': [strategy], 'budget': {'max_candles': CAP, 'feature_cells': 8000000, 'max_receipts': 1}, 'limits': {'workers': 1, 'max_parallel_candles': CAP, 'max_parallel_feature_cells': 8000000, 'max_parallel_receipts': 1, 'max_new_chunks': 8, 'lease_ms': 600000}}))
    records = []
    for role in ['inventory', 'source-facts', 'aggregate-refusal', 'scalar', 'resumed', 'cached']:
        result = base / f'receipt-{role}.json'
        subprocess.run([sys.executable, str(Path(__file__).resolve()), '--worker', role, str(binary), str(base), str(result)], check=True)
        records.append(json.loads(result.read_text()))
    financial = [r['value_sha256'] for r in records if r['role'] in ['scalar', 'resumed', 'cached']]
    assert len(set(financial)) == 1
    evidence = {'class': 'SYNTHETIC_CONTINUOUS_UTC_HORIZON_ONLY',
                'host': {'platform': platform.platform(), 'available_parallelism': len(os.sched_getaffinity(0))},
                'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'version': run(binary, 'version'), 'quotes': ROWS, 'artifacts': len(chunks),
                'expected_closed_candles': COUNTS, 'aggregate_closed_candles': TOTAL,
                'declared_candle_ceiling': CAP, 'new_artifacts_per_round': 8,
                'records': records,
                'measurement_scope': 'Each role uses a fresh Python worker and actual CLI child processes; elapsed/child CPU/peak child RSS include only the declared role. Fixture generation is excluded. OS cache uncontrolled; no physical-cold or whole-host/service RSS claim.',
                'limitations': 'Artificial constant quote once per minute over continuous2025/2026 UTC, with no final unfinished bucket. Zero trading signals/trades are intentional for horizon/context/receipt checks and establish no economic evidence. This is not genuine tick density, actual source/calendar approval, representative throughput, independent trials, OS resource enforcement, protected/non-USD activation, qualification or production/full-history acceptance. Existing resource defaults and experimental codec/index policy remain unchanged.'}
    a.output.parent.mkdir(parents=True, exist_ok=True)
    a.output.write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps({'synthetic': True, 'quotes': ROWS, 'aggregate_closed_candles': TOTAL, 'exact_receipts': True, 'observations': [{k: v for k, v in r.items() if k in ['role', 'elapsed_ms', 'peak_pipeline_child_rss_kib']} for r in records]}))
