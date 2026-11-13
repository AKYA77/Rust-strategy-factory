#!/usr/bin/env python3
"""SQL lookup component only; fixture rows are not valid review receipts."""
import hashlib
import json
from pathlib import Path
import resource
import sqlite3
import statistics
import sys
import time

query = "SELECT id FROM catalog_reviews WHERE json_extract(CAST(body AS TEXT),'$.request.catalog_identity')=?1 AND id>?2 ORDER BY id LIMIT ?3"
c = sqlite3.connect(':memory:')
c.execute('CREATE TABLE catalog_reviews(id TEXT PRIMARY KEY,body BLOB)')
selected = 'a' * 64
other = 'b' * 64
expected = []
for i in range(5000):
    identity = selected if i % 1000 == 0 else other
    key = f'{i:064x}'
    if identity == selected:
        expected.append((key,))
    body = json.dumps({'request': {'catalog_identity': identity}, 'fixture': 'METADATA_ONLY_NOT_VALID_REVIEW', 'padding': 'x' * 1024}).encode()
    c.execute('INSERT INTO catalog_reviews VALUES(?,?)', (key, body))
c.commit()
measurements = []
for repetition in range(10):
    for mode in (['scan', 'index'] if repetition % 2 == 0 else ['index', 'scan']):
        c.execute('DROP INDEX IF EXISTS reviews_scope')
        if mode == 'index':
            c.execute("CREATE INDEX reviews_scope ON catalog_reviews(json_extract(CAST(body AS TEXT),'$.request.catalog_identity'),id)")
        plan = [r[3] for r in c.execute('EXPLAIN QUERY PLAN ' + query, (selected, '', 26))]
        started = time.perf_counter()
        for _ in range(12):
            assert c.execute(query, (selected, '', 26)).fetchall() == expected
        measurements.append({'repetition': repetition, 'mode': mode, 'twelve_queries_ms': (time.perf_counter() - started) * 1000, 'plan': plan})
manifest = [{'path': p, 'sha256': hashlib.sha256(Path(p).read_bytes()).hexdigest()} for p in ['scripts/benchmark-review-query.py', 'src/catalog.rs', 'src/data_review.rs', 'Cargo.lock']]
result = {'class': 'SQL_COMPONENT_METADATA_FIXTURE_NOT_VALID_REVIEWS', 'sqlite_version': sqlite3.sqlite_version, 'rows': 5000, 'matches': 5, 'measurements': measurements, 'median_twelve_queries_ms': {mode: statistics.median(r['twelve_queries_ms'] for r in measurements if r['mode'] == mode) for mode in ['scan', 'index']}, 'source_manifest': manifest, 'whole_process_peak_rss_kib_linux': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss, 'limitations': 'Warm in-memory Python SQLite query only; its library can differ from bundled Rust SQLite. Invalid business-receipt metadata fixtures, no market data. Includes query setup/fetch, excludes DDL/fixture construction. Not Rust API/receipt validation, filesystem/cold I/O, acquisition, qualification or research throughput. No application speedup claim.'}
with open(sys.argv[1], 'x') as out:
    json.dump(result, out, indent=2)
    out.write('\n')
print(json.dumps(result['median_twelve_queries_ms']))
