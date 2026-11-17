# Historical data readiness — verified facts and open questions

Structural status only. Nothing here grants calendar, profile, legal or scientific approval.
All figures come from the real VPS corpus via the commands listed at the end.

## Verified

- **Coverage:** 8 instruments × 22 months (2025-01 … 2026-10) = 176 canonical partitions,
  421,078,937 events, 4,299 native chunks, 13 GiB. The seven noncanonical legacy partitions
  are excluded by the canonical planner.
- **Source identities:** every partition binds a `source_sha256`, `parquet_sha256`,
  `dataset_hash`, instrument, price scale, source id, ingest batch and reproduced
  `quote_stream_sha256`. Outputs are deterministic and restart-safe.
- **Source metadata (all 8 instruments):** `source_id = HistData.com`,
  `source_precision = millisecond`, `price_scale = 1000000`, `quality_flags` empty.
- **Timestamp continuity:** per-month first/last and tie counts are recorded in the catalog.
  Intra-month gaps over one hour: 91–96 per FX instrument, 465 for XAUUSD. Inter-month gap
  median 2–14 s; maximum ≈ 176,400 s (≈ 49 h) for every instrument.
- **Session pattern (EURUSD 2026-03 sample):** Monday–Friday active, **Saturday absent**,
  Sunday present (session open), all 24 hours present. Consistent with a Sunday-open /
  Friday-close forex week. This is an observation, not a broker-confirmed calendar.
- **Quality anomalies:** legacy corrections (rows reordered) total 638–8,170 per FX
  instrument and 103,621 for XAUUSD; crossed-quote quarantine removed 353 quotes in
  USDCAD 2026-07. Timestamp ties are common (up to 20,219,432 for XAUUSD), i.e. multiple
  quotes share a millisecond.
- **Structural validity:** 8/8 instruments structurally valid; **0 eligible for research**.

## Unknown — requires owner/external determination (do not invent)

- Exact session open/close times, holiday calendar and daylight-saving handling. Saturday
  absence and the ≈49 h weekend gap are observations only.
- Broker/provider license terms for `HistData.com`, redistribution rights, and whether the
  data may be used for this research. The legal gate stays **pending**.
- Broker instrument specification (contract size, tick/pip definition, swap/rollover) for
  each symbol. The profile gate stays **pending**.
- Scientific protocol approval and data-quality sign-off. The scientific gate stays
  **pending**.
- Whether the corrected legacy corpus is the authoritative version for every partition.

## Gate status

| Gate | State | Required evidence |
|---|---|---|
| Calendar | pending | broker/session definition (open/close, holidays, DST); per-instrument coverage and continuity report |
| Profile | pending | broker instrument specification; profile review record |
| Legal | pending | provider license terms and redistribution review |
| Scientific | pending | research protocol approval; data-quality review record |

A reviewer-supplied acceptance flag is a claim bound to evidence, not independently verified
evidence. Missing evidence is never recorded as accepted.

## Reproducible commands

```sh
# structural status + gates
target/release/rust-strategy-factory historical-catalog-scan \
  /var/lib/hermes-factory /root/workspace/historical-migration-output/full-100k \
  /root/workspace/historical-migration-output/catalog.json
target/release/rust-strategy-factory historical-catalog-verify \
  /root/workspace/historical-migration-output/catalog.json

# bounded full-corpus audit for one instrument
target/release/rust-strategy-factory historical-corpus-audit \
  /var/lib/hermes-factory /root/workspace/historical-migration-output/full-100k \
  EURUSD /root/workspace/historical-migration-output/audit-EURUSD.json
```

The catalog scan is receipt-only; the corpus audit streams chunk by chunk with bounded memory.
