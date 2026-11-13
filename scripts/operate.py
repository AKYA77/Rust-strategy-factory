#!/usr/bin/env python3
"""Local, content-addressed installation and verified registry recovery. Python 3/Linux."""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def run(binary, *args):
    return subprocess.run([str(binary), *map(str, args)], check=True,
                          capture_output=True, text=True).stdout.strip()


def sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def directory(path):
    if path.is_symlink():
        raise ValueError(f'refusing symlink directory: {path}')
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    if not path.is_dir():
        raise ValueError(f'not a directory: {path}')


@contextmanager
def locked(prefix):
    if not prefix.is_absolute() or prefix.resolve() == Path('/'):
        raise ValueError('installation prefix must be an absolute non-root directory')
    directory(prefix)
    lock = prefix / '.operations.lock'
    if lock.is_symlink():
        raise ValueError('refusing symlink operation lock')
    with lock.open('a') as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        yield


def target(prefix, name, required=True):
    link = prefix / name
    if not link.is_symlink():
        if link.exists() or required:
            raise ValueError(f'{name} must be an installed release symlink')
        return None
    value = os.readlink(link)
    if not re.fullmatch(r'releases/[0-9a-f]{64}', value):
        raise ValueError(f'{name} points outside the release layout')
    return value.split('/')[1]


def verify(prefix, release):
    if not re.fullmatch(r'[0-9a-f]{64}', release):
        raise ValueError('release identity must be a SHA-256 digest')
    base = prefix / 'releases' / release
    binary = base / 'rust-strategy-factory'
    manifest = base / 'manifest.json'
    if (prefix / 'releases').is_symlink() or base.is_symlink() or binary.is_symlink() or manifest.is_symlink():
        raise ValueError('refusing symlink release contents')
    metadata = json.loads(manifest.read_text())
    if not isinstance(metadata, dict) or metadata.get('format') not in [1, 2] or metadata.get('sha256') != release or digest(binary) != release:
        raise ValueError('installed binary checksum mismatch')
    if metadata['format'] == 2:
        notices = base / 'THIRD_PARTY_NOTICES.txt'
        if notices.is_symlink() or digest(notices) != metadata.get('notices_sha256'):
            raise ValueError('installed third-party notices checksum mismatch')
    if json.loads(run(binary, 'version')) != metadata['version']:
        raise ValueError('installed binary version manifest mismatch')
    return binary, metadata


def replace_link(prefix, name, release):
    # Refuse to replace unrelated user files, even during recovery/retry.
    target(prefix, name, required=False)
    with tempfile.TemporaryDirectory(prefix='.activate-', dir=prefix) as temporary:
        link = Path(temporary) / name
        link.symlink_to(f'releases/{release}')
        os.replace(link, prefix / name)
    sync_directory(prefix)


def activate(prefix, release):
    verify(prefix, release)
    old = target(prefix, 'current', required=False)
    if old == release:
        return
    if old is not None:
        verify(prefix, old)
        replace_link(prefix, 'previous', old)
    replace_link(prefix, 'current', release)


def install(prefix, source):
    if not source.is_file():
        raise ValueError('binary source must be an existing file')
    # Check activation paths before adding any release or replacing a link.
    target(prefix, 'current', required=False)
    target(prefix, 'previous', required=False)
    for name in ['releases', 'config', 'state', 'backups']:
        directory(prefix / name)
    release = digest(source)
    destination = prefix / 'releases' / release
    if not destination.exists():
        staging = Path(tempfile.mkdtemp(prefix='.install-', dir=prefix / 'releases'))
        try:
            binary = staging / 'rust-strategy-factory'
            shutil.copyfile(source, binary)
            binary.chmod(0o755)
            if digest(binary) != release:
                raise ValueError('source changed during installation')
            version = json.loads(run(binary, 'version'))
            if not isinstance(version, dict) or not isinstance(version.get('compute_revision'), str) or not isinstance(version.get('engine'), str):
                raise ValueError('unsupported binary version response')
            notices = Path(__file__).resolve().parent.parent / 'docs/THIRD_PARTY_NOTICES/zstd.txt'
            shutil.copyfile(notices, staging / 'THIRD_PARTY_NOTICES.txt')
            metadata = {'format': 2, 'sha256': release, 'version': version,
                        'notices_sha256': digest(staging / 'THIRD_PARTY_NOTICES.txt')}
            with (staging / 'THIRD_PARTY_NOTICES.txt').open('rb') as stream:
                os.fsync(stream.fileno())
            with (staging / 'manifest.json').open('x') as stream:
                json.dump(metadata, stream, indent=2)
                stream.write('\n')
                stream.flush()
                os.fsync(stream.fileno())
            with binary.open('rb') as stream:
                os.fsync(stream.fileno())
            sync_directory(staging)
            staging.rename(destination)
            sync_directory(prefix / 'releases')
        finally:
            if staging.exists():
                shutil.rmtree(staging)
    activate(prefix, release)
    return {'release': release, 'prefix': str(prefix), 'activated': True}


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=Path, required=True)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('install').add_argument('binary', type=Path)
    sub.add_parser('activate').add_argument('release')
    sub.add_parser('rollback')
    sub.add_parser('status')
    for name in ['backup', 'restore']:
        command = sub.add_parser(name)
        command.add_argument('source', type=Path)
        command.add_argument('destination', type=Path)
    args = parser.parse_args()
    with locked(args.prefix):
        if args.command == 'install':
            result = install(args.prefix, args.binary.resolve())
        elif args.command in ['activate', 'rollback']:
            release = args.release if args.command == 'activate' else target(args.prefix, 'previous')
            activate(args.prefix, release)
            result = {'release': release, 'activated': True}
        else:
            release = target(args.prefix, 'current')
            binary, metadata = verify(args.prefix, release)
            if args.command == 'status':
                result = {'release': release, 'version': metadata['version'],
                          'previous': target(args.prefix, 'previous', required=False)}
            else:
                source, destination = args.source.absolute(), args.destination.absolute()
                output_paths = [Path(str(destination) + suffix) for suffix in ['', '-wal', '-shm']]
                if not source.is_file() or any(p.exists() or p.is_symlink() for p in output_paths):
                    raise ValueError('source must exist and destination/sidecars must be new')
                # Verify in private staging and atomically publish without replacing a path.
                # Restore requires an offline backup; migrations affect only its new copy.
                with tempfile.TemporaryDirectory(prefix='.registry-', dir=destination.parent) as temporary:
                    staged = Path(temporary) / 'registry.sqlite'
                    if args.command == 'restore':
                        for suffix in ['-wal', '-shm']:
                            if Path(str(source) + suffix).exists() or Path(str(source) + suffix).is_symlink():
                                raise ValueError('restore requires an offline backup without live sidecars')
                        expected = digest(source)
                        shutil.copyfile(source, staged)
                        if digest(staged) != expected or digest(source) != expected or any(Path(str(source) + suffix).exists() for suffix in ['-wal', '-shm']):
                            raise ValueError('backup source changed during restore')
                    else:
                        run(binary, 'registry-check', source)
                        run(binary, 'backup', source, staged)
                    counts = json.loads(run(binary, 'registry-check', staged))
                    for suffix in ['-wal', '-shm']:
                        if Path(str(staged) + suffix).exists():
                            raise ValueError('staged registry still has live sidecars')
                    with staged.open('rb') as stream:
                        os.fsync(stream.fileno())
                    os.link(staged, destination)  # EEXIST preserves a concurrently created path.
                    sync_directory(destination.parent)
                result = {'operation': args.command, 'destination': str(destination),
                          'integrity_verified': True, 'counts': counts}
        print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f'operation failed: {error}', file=sys.stderr)
        sys.exit(1)
