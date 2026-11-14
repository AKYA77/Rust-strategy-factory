#!/usr/bin/env python3
"""Exercise local operations against real binaries and disposable synthetic state."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--release', type=Path, default=root / 'target/release/rust-strategy-factory')
parser.add_argument('--alternate', type=Path, default=root / 'target/debug/rust-strategy-factory')
args = parser.parse_args()

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def command(*arguments, success=True, **kwargs):
    result = subprocess.run(list(map(str, arguments)), capture_output=True, text=True, **kwargs)
    if success:
        assert result.returncode == 0, result.stderr
    else:
        assert result.returncode != 0, result.stdout
    return result

with tempfile.TemporaryDirectory(prefix='factory-operations-') as directory:
    base = Path(directory)
    prefix = base / 'install with spaces'
    def operate(*arguments, success=True):
        result = command(sys.executable, root / 'scripts/operate.py', '--prefix', prefix,
                         *arguments, success=success)
        return json.loads(result.stdout) if success else result
    first = operate('install', args.release)['release']
    assert first == digest(args.release)
    notices = prefix / 'releases' / first / 'THIRD_PARTY_NOTICES.txt'
    assert notices.read_bytes() == b'\n\n'.join(p.read_bytes() for p in sorted((root / 'docs/THIRD_PARTY_NOTICES').glob('*.txt')))
    saved_notices = notices.read_bytes()
    notices.write_bytes(saved_notices + b'tamper')
    operate('status', success=False)
    notices.write_bytes(saved_notices)
    assert operate('status')['release'] == first
    assert operate('install', args.release)['release'] == first
    assert operate('status')['previous'] is None
    sentinel = prefix / 'config/operator-notes.txt'
    sentinel.write_text('preserve local configuration\n')
    binary = prefix / 'current/rust-strategy-factory'
    dataset = prefix / 'state/input.json'
    strategy = prefix / 'config/test-strategy.json'
    database = prefix / 'state/research.sqlite'
    command(binary, 'fixture', 'EURUSD', dataset)
    strategy.write_text(json.dumps({'timeframe': 'M1', 'side': 'Long', 'stop_ticks': 100,
                                   'rule': {'AboveMean': {'window': 100000}}}))
    experiment = json.loads(command(binary, 'experiment', dataset, strategy, database).stdout)
    command(binary, 'diagnose', database, experiment['experiment_id'])
    # Actual replay, not injected metrics: enough synthetic resolved trades to externalize receipts.
    profile = next(p for p in json.loads(command(binary, 'profiles').stdout) if p['symbol'] == 'EURUSD')
    profile_file, quotes, large_dataset = base / 'profile.json', base / 'quotes.csv', base / 'large-input.json'
    profile_file.write_text(json.dumps(profile))
    rows = ['timestamp_ms,sequence,bid,ask']
    for cycle in range(1600):
        for index, (offset, bid, ask) in enumerate([(0,'1.00000','1.00010'),(60000,'1.00020','1.00030'),(120000,'1.00040','1.00050'),(120001,'1.00050','1.00060'),(121000,'1.00500','1.00510')]):
            rows.append(f'{1735689600000 + cycle * 180000 + offset},{cycle * 5 + index},{bid},{ask}')
    quotes.write_text('\n'.join(rows) + '\n')
    command(binary, 'import', quotes, profile_file, 'SYNTHETIC_TEST_ONLY artifact operations', large_dataset, 'synthetic')
    strategy.write_text(json.dumps({'timeframe':'M1','side':'Long','stop_ticks':50,'rule':{'AboveMean':{'window':2}}}))
    large = json.loads(command(binary, 'experiment', large_dataset, strategy, database).stdout)
    inventory = json.loads(command(binary, 'registry-artifacts', database).stdout)
    assert int(inventory['referenced_receipts']) >= 1
    def artifacts(db):
        folder = Path(str(db) + '.artifacts')
        return {file.name: digest(file) for file in folder.iterdir()} if folder.exists() else {}
    source_artifacts = artifacts(database)
    assert source_artifacts
    counts = json.loads(command(binary, 'registry-check', database).stdout)
    backup = prefix / 'backups/verified.sqlite'
    assert operate('backup', database, backup)['counts'] == counts
    original_backup = digest(backup)
    assert artifacts(backup) == source_artifacts
    operate('backup', database, backup, success=False)
    assert digest(backup) == original_backup
    restored = prefix / 'state/recovered.sqlite'
    assert operate('restore', backup, restored)['counts'] == counts
    assert json.loads(command(binary, 'registry-check', restored).stdout) == counts
    assert artifacts(restored) == artifacts(backup) == source_artifacts
    assert digest(backup) == original_backup
    restored_hash = digest(restored)
    operate('restore', backup, restored, success=False)
    assert digest(restored) == restored_hash
    collision = prefix / 'state/sidecar-collision.sqlite'
    sidecar = Path(str(collision) + '-wal')
    sidecar.write_bytes(b'preserve unrelated sidecar')
    operate('restore', backup, collision, success=False)
    assert sidecar.read_bytes() == b'preserve unrelated sidecar'
    assert not collision.exists()
    assert not list(prefix.rglob('.registry-*'))
    command(sys.executable, root / 'scripts/operate.py', '--prefix', '/tmp/..', 'status', success=False)
    live = Path(str(backup) + '-wal')
    live.write_bytes(b'live-backup-refusal-fixture')
    operate('restore', backup, prefix / 'state/live-refused.sqlite', success=False)
    assert not (prefix / 'state/live-refused.sqlite').exists()
    live.unlink()
    artifact_collision = prefix / 'state/artifact-collision.sqlite'
    reserved = Path(str(artifact_collision) + '.artifacts')
    reserved.mkdir()
    (reserved / 'preserve').write_bytes(b'user evidence')
    operate('restore', backup, artifact_collision, success=False)
    assert not artifact_collision.exists() and (reserved / 'preserve').read_bytes() == b'user evidence'
    artifact = next(Path(str(backup) + '.artifacts').iterdir())
    original_artifact = artifact.read_bytes()
    artifact.write_bytes(b'x' + original_artifact[1:])
    corrupt_destination = prefix / 'state/corrupt-artifact.sqlite'
    operate('restore', backup, corrupt_destination, success=False)
    assert not corrupt_destination.exists() and not Path(str(corrupt_destination) + '.artifacts').exists()
    artifact.write_bytes(original_artifact)
    assert artifacts(backup) == source_artifacts
    second = operate('install', args.alternate)['release']
    assert second != first
    assert operate('status')['previous'] == first
    assert operate('rollback')['release'] == first
    assert operate('status')['previous'] == second
    assert sentinel.read_text() == 'preserve local configuration\n'
    assert json.loads(command(binary, 'registry-check', database).stdout) == counts
    operate('activate', second)
    operate('activate', first)
    with (prefix / 'releases' / second / 'rust-strategy-factory').open('ab') as stream:
        stream.write(b'tamper-test')
    operate('activate', second, success=False)
    assert operate('status')['release'] == first
    unrelated = base / 'unrelated'
    unrelated.mkdir()
    (unrelated / 'current').write_text('user file\n')
    command(sys.executable, root / 'scripts/operate.py', '--prefix', unrelated,
            'install', args.release, success=False)
    assert (unrelated / 'current').read_text() == 'user file\n'
    invalid = base / 'invalid.sqlite'
    invalid.write_bytes(b'not a registry')
    output = prefix / 'state/refused.sqlite'
    operate('restore', invalid, output, success=False)
    assert not output.exists()
    incomplete = base / 'incomplete-package'
    (incomplete / 'scripts').mkdir(parents=True)
    (incomplete / 'docs/THIRD_PARTY_NOTICES').mkdir(parents=True)
    shutil.copyfile(root / 'scripts/operate.py', incomplete / 'scripts/operate.py')
    shutil.copyfile(root / 'docs/THIRD_PARTY_NOTICES/zstd.txt', incomplete / 'docs/THIRD_PARTY_NOTICES/zstd.txt')
    incomplete_prefix = base / 'incomplete-install'
    command(sys.executable, incomplete / 'scripts/operate.py', '--prefix', incomplete_prefix,
            'install', args.release, success=False)
    assert not (incomplete_prefix / 'current').exists()
    units_verified = False
    if shutil.which('systemd-analyze'):
        # Validate populated user-unit copies, without installing or starting a service.
        unit_prefix = base / 'unit-install'
        command(sys.executable, root / 'scripts/operate.py', '--prefix', unit_prefix,
                'install', args.release)
        runtime = base / 'runtime'
        runtime.mkdir(mode=0o700)
        units = []
        for template in sorted((root / 'deploy').glob('*.service')):
            populated = base / template.name
            populated.write_text(template.read_text().replace('%h/.local/share/rust-strategy-factory', str(unit_prefix)))
            units.append(populated)
        environment = os.environ.copy()
        environment['XDG_RUNTIME_DIR'] = str(runtime)
        command('systemd-analyze', '--user', 'verify', *units, env=environment)
        units_verified = True
    print(json.dumps({'synthetic': True, 'passed': ['install', 'repeat install', 'update',
        'rollback', 'explicit activation', 'checksum tamper refusal', 'preserved configuration',
        'native verified backup', 'new-path restore', 'no-overwrite', 'invalid registry refusal',
        'unrelated activation file preservation', 'actual large receipt artifact backup/restore', 'artifact collision/corruption refusal'], 'populated_user_units_verified': units_verified,
        'live_systemd_and_vps_execution': 'NOT_RUN'}))
