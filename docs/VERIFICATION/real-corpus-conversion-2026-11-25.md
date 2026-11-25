# Real-corpus conversion evidence — non-USD quotes (2026-11-25)

Status: VERIFIED (offline diagnostic evidence). No financial, scientific, provenance or
security safeguard was changed. No calendar completeness, broker/profile approval,
source licensing, research eligibility or qualification is granted.

## Scope

Closes the evidence gap left by the synthetic-only `tests/verify_conversion.rs`/`docs/CONVERSION_CONVENTION.md`
increment: the three USD-base, non-USD-quote instruments (`USDJPY`, `USDCHF`, `USDCAD`) are now
verified against **actual retained corpus ticks** and the **actual corpus manifest profile**,
not a synthetic fixture.

Read-only inputs: `/root/workspace/historical-migration-output/full-100k/{SYMBOL}-202503/manifest.json`
and `chunk-000000.rsf`. Nothing was downloaded, mutated or deleted.

## Baseline (before increment)

`main` clean at `18bba89d95e7c003decb140ad7e3e5b6842fd011` (equal to `origin/main`), no
`cargo`/`rustc` worker, heavy lock free. With
`CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup PATH=/workspace/.cargo/bin:$PATH RSF_BUILDER_PROFILE=hermes-v2-m1`:

- `scripts/check.sh` **EXIT=0**: `cargo fmt --all -- --check` clean, `cargo clippy --locked --all-targets -- -D warnings` clean,
  **755 passed / 0 failed across 70 test binaries** (lib unit tests 276). Zero named test failures.

## Red / green regression demonstration

New target `tests/real_conversion_evidence.rs` (2 tests). The negative control was first
written with the *wrong* expectation (`assert_eq!` between the corpus-profile rate and the
synthetic-profile rate), executed, then corrected to the true invariant (`assert_ne!`) and
re-executed. Both runs used `scripts/cargo-locked.sh test --locked --test real_conversion_evidence -- --nocapture`.

| Phase | Command result | Named outcome |
|---|---|---|
| RED (wrong expectation) | `EXIT=101`, `1 passed; 1 failed` | `real_corpus_ticks_require_the_corpus_profile_not_the_synthetic_profile` **FAILED**: `left: Conversion { usd_num: 250, quote_den: 37661, as_of_ms: 1740953041160 }`, `right: Conversion { usd_num: 1, quote_den: 150644, as_of_ms: 1740953041160 }` |
| GREEN (true invariant) | `EXIT=0`, `2 passed; 0 failed` | both tests pass; negative control prints `real_rate=250/37661 synthetic_rate=1/150644 silent_validation=yes` |

The RED result is a genuine hazard, not a fabricated failure: applying the synthetic fixture's
scale (`price_scale=1000`, `tick=1`) to a real corpus bid (`150644000` profile units) yields
`1/150644` USD per JPY instead of `250/37661` (`1/150.644`) — a **1000x** error — while both
rates still satisfy `Conversion::validate`. The permanent `assert_ne!` test now guards against
that silent misapplication.

Logs: `/tmp/rsf-conversion-red.log`, `/tmp/rsf-conversion-green.log` (host-local, not committed).

## Verified positive evidence (GREEN run)

For each instrument, one 100,000-event verified native chunk, 64 deterministic stride samples
spanning the chunk:

| Instrument | Manifest profile | Events | Sampled | Distinct exact rates | Consumed-byte blake3 |
|---|---|---:|---:|---:|---|
| USDJPY | `price_scale=1000000`, `tick=1000`, quote JPY | 100000 | 64 | 61 | `738f99d1b369e84dc190c0a392a712e5e3dccbc407cca4dc8ed852f0b59c125f` |
| USDCHF | `price_scale=1000000`, `tick=10`, quote CHF | 100000 | 64 | 61 | `c88a7e4703f872d00eeb35f9f06c73ae42e55935afd9fde4330f9a60a578357b` |
| USDCAD | `price_scale=1000000`, `tick=10`, quote CAD | 100000 | 64 | 60 | `1b7b332811bbf4c11bc67a115f03d445c7a8100fd6334212bc45cc61794f041e` |

Manifest provenance bound for each instrument (all 64-hex): `source.parquet_sha256`,
`source.dataset_hash`, `quote_stream_sha256`, `output_hash`, per-chunk `dataset_hash`, plus the
native reader's exact consumed-byte fingerprint (deterministic across two reads) and equality of
the native metadata `Profile` with the manifest `Profile`. Declared class remains
`REAL_STRUCTURALLY_VALID`.

Per sampled tick the test asserts: `as_of_ms == tick.timestamp_ms` (strict event alignment);
`validate(profile, event_ms, 0)` passes; exact cross-product
`usd_num * price == quote_den * price_scale` in `i128`; no `1/1` identity rate; the identity
sentinel is refused; future and stale conversions are refused; bounded 1 ms staleness is accepted
when `max_age_ms=1`. More than one distinct rate per instrument proves rates are derived from
real quotes rather than a constant.

## Limitations

- Five-minute 2025-03 window, first chunk only, 64 ticks per instrument — bounded evidence, not
  a full 22-month conversion study.
- The reciprocal closing-rate convention remains an explicit unverified research assumption,
  not an approved broker conversion rule.
- Diagnostic evidence only: no strategy qualification, no live/broker use, no data acquisition.
- CI without the retained corpus skips the two tests with an explicit `SKIP` line; setting
  `RSF_REAL_CORPUS_REQUIRED` makes a missing corpus a hard failure. On this host the corpus was
  present and both tests executed (no skip).
