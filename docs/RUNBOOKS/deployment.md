# Local releases and target deployment

The application remains SOFTWARE_INCOMPLETE. These procedures install locally built artifacts and preserve research state; they do not approve data/profiles, qualify strategies, or establish target-VPS acceptance. Actual VPS installation, service activation/reboot, sustained resource limits and restore cutover must be verified on the intended host separately.

## Build and install

Use Linux, Python3.11+, the pinned Rust toolchain, a C compiler and Cargo.lock. In this cloud environment activate Cargo/Rustup exactly as AGENTS.md describes. Build and validate in the existing checkout:

```sh
sh scripts/check.sh
cargo build --locked --release
cargo build --locked
python3 scripts/check-operations.py
```

Distribute `docs/THIRD_PARTY_NOTICES/zstd.txt` with the installer; new releases copy and verify the added codec stack notices. Historical format1 release manifests remain readable. This is not a completed audit of every pre-existing dependency.

The operations acceptance uses real release/debug binaries in disposable directories, including a path with spaces. It checks content-addressed installation, repeat install, upgrade/rollback, checksum tampering, preservation, actual synthetic experiment/diagnosis state, verified backup/new-path restore, no-overwrite and invalid/sidecar refusal. It also validates populated user-unit copies with systemd-analyze when present. It never starts systemd or deploys a host. The alternate debug build is an installation-test artifact, not a production recommendation; a profile change changes compute identity.

Choose an absolute non-root installation directory owned by the non-root service account. For the supplied user-unit templates use the default layout:

```sh
factory_prefix="$HOME/.local/share/rust-strategy-factory"
python3 scripts/operate.py --prefix "$factory_prefix" install target/release/rust-strategy-factory
python3 scripts/operate.py --prefix "$factory_prefix" status
```

`install` copies the provided binary into `releases/<sha256>/`, verifies its copied checksum and structured `version` output, records the manifest and bundled third-party notice checksum (format2), and atomically switches `current`. It records the previous release for rollback, preserves config/state/backups and serializes operations with an advisory lock. Existing unrelated activation files or symlink release contents are refused. Installation neither downloads binaries nor establishes their upstream authenticity: build from the selected reviewed source with normal Cargo checksum/TLS verification. A content hash identifies an artifact; it is not a signature or proof of reproducible bytes across toolchains/hosts.

New directories/files use a restrictive umask; the executable remains executable. Existing local configuration is never overwritten by the installer. Source artifacts, lockfiles and the original checkout are retained. No service is installed or started automatically.

## Initialize configuration and state

Copy `config/research.example.json` to a NEW `config/research.json` under the prefix after inspecting its dataset references. Its relative `../state/fixture.json` resolves under this layout. A development smoke run can create that explicitly synthetic fixture and run ten checkpoints:

```sh
"$factory_prefix/current/rust-strategy-factory" fixture EURUSD "$factory_prefix/state/fixture.json"
"$factory_prefix/current/rust-strategy-factory" research "$factory_prefix/config/research.json" "$factory_prefix/state/research.sqlite" 10
"$factory_prefix/current/rust-strategy-factory" registry-check "$factory_prefix/state/research.sqlite"
```

Do not overwrite an existing config/fixture or substitute synthetic data for approved history. Real inputs require explicit training windows and event-aligned conversions; source/profile/calendar approval and independent evidence remain pending. Database initialization/migration occurs through normal CLI open paths. Keep a verified backup before any binary/schema change. A binary rollback does not imply backward-compatible database migration.

For the dashboard, initialize a NEW `config/admin.json` through `auth-init` with a locally chosen password on stdin, following dashboard.md. Keep mode0600, never put passwords in units/commands/history or Git, and preserve the auth file independently with private operator configuration. The automated acceptance uses disposable development authentication; it supplies no production password.

## Verify backups and restore to a new path

```sh
python3 scripts/operate.py --prefix "$factory_prefix" backup "$factory_prefix/state/research.sqlite" "$factory_prefix/backups/checkpoint.sqlite"
python3 scripts/operate.py --prefix "$factory_prefix" restore "$factory_prefix/backups/checkpoint.sqlite" "$factory_prefix/state/recovered.sqlite"
```

The backup path must be new. Native SQLite online backup includes committed WAL contents. The operation verifies a private staged database before publishing a new hard link on the same filesystem; an existing destination/sidecar is never replaced. An error produces no verified-success result. Staging is removed on ordinary failure. Keep sufficient disk capacity for the complete snapshot; a killed process may leave a hidden staging directory for operator inspection.

Restore accepts an offline finalized backup with no live sidecars, copies it into staging and validates/migrates only the new copy, preserving the original backup. It creates a new database, without replacing a running registry or starting a worker. Stop old writers and wait for outstanding coordinator/experiment leases to expire before any cutover. Verify the recovered database and counts, keep the original registry and its sidecars intact, then point the stopped service configuration at the recovered path. Never independently run the original and restored copies as though they share exposure/holdout protection; registry-local protections cannot track another copy.

Backups contain registry evidence, not original quote archives, configs or the dashboard credential file. Preserve those separately with their provenance/permissions. No retention deletion is automatic. Monitor capacity and retain known-good recovery points; never delete scientific evidence merely to force progress.

## User services and resource limits

`deploy/rust-strategy-factory-research.service` and `deploy/rust-strategy-factory-dashboard.service` are non-root user-unit templates for the default prefix. The research unit runs one coordinator and restarts only on failure; normal campaign exhaustion remains stopped. The dashboard binds loopback through the application's existing policy. No public HTTPS mode is supplied.

On an authorized target host, inspect/populate the configuration and auth first, stop any earlier writers, then copy the units into the service account's user-unit directory. Validate with `systemd-analyze --user verify` before enabling them. For a custom prefix, adjust all WorkingDirectory/ExecStart/ReadWritePaths entries in local copies. Do not overwrite unrelated services. Operators can then use `systemctl --user daemon-reload`, `start`, `stop`, `restart`, `status` and `journalctl --user -u` for these exact units. Starting at boot without login requires that host's administrator to configure lingering; this has not been performed here.

The templates declare research CPU300%/memory6GiB and dashboard CPU50%/memory512MiB, task/file-descriptor limits, owner-only file creation, no-new-privileges and read-only system paths with writable registry state. They leave capacity on the intended four-vCPU/eight-GB host. Actual user namespace/cgroup controller support and enforced caps must be checked on the target: static verification alone does not establish enforcement. The configured dataset paths must remain readable.

For a safe research shutdown, first use `research-control ... RUN_ID stopped` and wait for the coordinator's current experiment/checkpoint to finish. On Unix, service SIGINT/SIGTERM now requests an orderly process exit after the current checkpoint and releases the coordinator lease. It does not preempt a replay; a hard stop timeout/SIGKILL still invokes crash recovery. The restart delay310 seconds accommodates the default300-second lease; adjust it for any longer configured lease. Crash recovery still relies on fenced leases and committed receipts; a successful graceful exit permits immediate restart. Never run a competing coordinator during cutover.

Both units send logs to journald; inspect service errors and journal capacity/retention on the host. Monitor filesystem usage (`df`/`du`), registry/WAL size, last successful checkpoint, `/api/health` and an authenticated representative request. Health alone is insufficient to prove research progress. Automatic disk cleanup, journal retention policy, production alerts, crash/disk-full/reboot acceptance and a real restore drill remain pending.

## Update and rollback

Stop writers at a safe checkpoint, create/verify a backup, build/check the new source, and call `install` with its release binary. Check `status`, registry integrity and representative CLI/dashboard behavior before restarting the intended services. A new compute revision creates distinct experiments/runs; never mix builds within a frozen scientific protocol.

```sh
python3 scripts/operate.py --prefix "$factory_prefix" rollback
python3 scripts/operate.py --prefix "$factory_prefix" activate EXACT_RELEASE_SHA256
```

Both commands verify the selected stored binary/manifest before switching the symlink. They never reset a database, remove evidence or stop/start processes. Check schema compatibility before reopening state with an older binary. If a release is corrupted, activation fails and leaves the current release intact; preserve it for inspection and recover from a trusted artifact into a separate reviewed location. End-to-end host rollback, schema downgrade support and target-VPS acceptance remain unverified.
