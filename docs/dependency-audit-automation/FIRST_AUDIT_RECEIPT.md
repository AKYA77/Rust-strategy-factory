# Dependency/security/license audit — first report

**Task:** t_810065d9 (dependency-audit-automation)
**Branch:** hermes/dependency-audit-automation, created from origin/main @ c8d28345734bbd3a323b63b7bdb6eb711dafef17
**Source SHA:** c8d28345734bbd3a323b63b7bdb6eb711dafef17 (HEAD = origin/main, worktree clean)
**Cargo.lock sha256:** e5308e42eb38c93a8fa5e65dc58b8d990bc5a23ef13e14069fd109a1b24cea52
**Recorded:** 2026-11-21T12:34 UTC
**Audited target:** x86_64-unknown-linux-gnu, default features, 145 resolved crate dependencies

## Tooling (audit tooling only; no new application dependencies)

| Tool | Version | Binary sha256 | Install |
|---|---|---|---|
| cargo-deny | 0.22.0 | 5cde4559b3b3f38a63b0dfee2bad6748c38fcc371ad7de619ce7ff973a28d92f | `cargo install cargo-deny cargo-audit` |
| cargo-audit | 0.23.0 | 39bd1425b6ec89bf433abbad27693d1672d9e224197a240472172c448c2cfce4 | `cargo install cargo-deny cargo-audit` |

Installed into /workspace/.cargo/bin (CARGO_HOME). All verification runs serialized under
`flock -w 3600 /root/workspace/.rsf-heavy.lock`; cargo invocations limited with `-j 4`
where applicable. Advisory DB: https://github.com/RustSec/advisory-db (fresh fetch at run
time; 1555 advisories loaded at report time).

## deny.toml

Checked in at repo root (`deny.toml`). Configuration covers four stages — advisories,
licenses, bans, sources — run by a single `cargo deny check`.

**License policy** (`[licenses] allow`): every resolved dependency must carry a license
accepted under the pinned-source review policy in docs/SOURCE_REVIEW (2026-11-12/13).
The resolved set is MIT / Apache-2.0 (dual or either) plus reviewed exceptions:
BSD-3-Clause, BSD-2-Clause, CC0-1.0, MIT-0, Unlicense, Zlib, Unicode-3.0 and
Apache-2.0 WITH LLVM-exception. Anything not listed fails the check — nothing is
silently admitted. The allow list mirrors the declared licenses in Cargo.lock and the
reviewed packages in docs/SOURCE_REVIEW (parquet Apache-2.0, zstd 0.13.3 MIT,
zstd-safe/zstd-sys BSD, etc.).

**Advisories:** three findings affect pinned dependencies. Per policy they are reported,
not silently ignored: each is ignored in deny.toml with a reason (ignored advisories
still emit notes in cargo-deny output) and justified below. Dependency version bumps are
deferred to separate changes that re-run this audit.

## Findings

| ID | Crate | Version | Kind | Severity | Date | Disposition |
|---|---|---|---|---|---|---|
| RUSTSEC-2026-0391 | zstd | 0.13.3 | vulnerability | — | 2026-10-16 | Ignored with justification (see below) |
| RUSTSEC-2026-0459 | libsqlite3-sys | 0.35.0 | vulnerability (CVE-2026-119375) | 6.8 medium | 2026-10-30 | Ignored with justification (see below) |
| RUSTSEC-2026-0543 | chrono | 0.4.45 | unmaintained | — | 2026-09-14 | Ignored with justification (see below) |

Unfiltered `cargo audit` output concluded: `error: 2 vulnerabilities found! warning: 1
allowed warning found`. With the documented ignores, both `cargo deny check` and
`cargo audit` exit 0.

### Justifications

**RUSTSEC-2026-0391 (zstd 0.13.3 — abandoned-stream context leak).** zstd 0.13.3 is pinned
per docs/SOURCE_REVIEW/2026-11-13-zstd.json (bounded RSFZ baseline, not the selected
production codec; parquet brings zstd 0.14.4 via shared zstd-sys). The app's only zstd
usage is `src/compressed.rs`: `compress` creates a fresh `Encoder`, finishes it, and drops
it; `decoded` creates a fresh `Decoder` with `single_frame()` per frame and calls
`finish()`. Contexts are never reused across streams, so the abandoned-stream cross-stream
leak cannot occur. Fix (bump to ^0.13.4 / >=0.14.3) deferred.

**RUSTSEC-2026-0459 (libsqlite3-sys 0.35.0 — SQLite CVE-2026-119375).** Crafted
sibling `-wal` file with a differing page size can leak stale process memory; local attack
vector requiring the victim to open the files. The app opens only application-created
registry database files and never opens third-party DB files. Fix (libsqlite3-sys
>=0.39.0 / rusqlite 0.41, per advisory) changes the pinned dependency set and is deferred.

**RUSTSEC-2026-0543 (chrono 0.4.45 — unmaintained, no safe upgrade available per advisory).**
Used for wall-clock timestamps in research output. Advisory explicitly states no fixed
upgrade exists. Migration (e.g. to jiff/time) deferred.

## Stage results (this run)

```
cargo deny check:   advisories ok (3 ignored w/ reasons), bans ok, licenses ok, sources ok
cargo audit:        2 vulnerabilities + 1 unmaintained warning, all 3 documented + ignored
```

Duplicate-version warnings for zstd (0.13.3 / 0.14.4) and zstd-safe (7.4.1 / 8.3.0) are
intentional: app baseline vs parquet codec, per docs/SOURCE_REVIEW.

## Limitations

- Licenses are declared license strings from Cargo.lock / registry metadata, consistent
  with docs/SOURCE_REVIEW's "ARCHIVE_CHECKSUM_AND_DECLARED_METADATA_ONLY" scope — not a
  per-file re-review or legal approval.
- Advisory DB freshness depends on the live fetch at run time; CI re-fetches each run.
- Single target (x86_64-unknown-linux-gnu); other targets/features not covered.
- No new application dependencies adopted; this commit adds audit tooling and CI only.
- Version bumps for the three flagged crates are deferred, not applied here.

## Verification commands

```
flock -w 3600 /root/workspace/.rsf-heavy.lock cargo deny check
cargo audit --ignore RUSTSEC-2026-0391 --ignore RUSTSEC-2026-0459 --ignore RUSTSEC-2026-0543
```

The CI job `.github/workflows/ci.yml` `dependency-audit` runs the same stages in parallel
with the other three jobs; merge requires the full green rollup on the exact SHA.
