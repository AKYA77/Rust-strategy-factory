# PR #6: data-integrity review checkpoint

Reviewed source: `86c172c74127234ed2c70a7cddd0732e371d75d8`, branch `hermes/real-data-migration`. Review branch: `codex/review-pr6-data-integrity`, based on that exact commit. No Hermes branch, source archive or VPS state was modified. The two temporary Rust test injections have been removed from production source; the reproducer patch is retained alongside this note. No formal review/comment was sent to another developer.

## P1 — resumed output is not semantically compared with its source prefix

`rebuild_state` accepts valid native chunks based on self-consistent metadata, hashes, sequence and context (src/historical_migration.rs:523–600). `convert` then skips their source rows (around lines878–905). Its final quote-stream check hashes the original Parquet and compares that input hash with the catalog, but does not hash/compare the resumed native prices with the corresponding source prices (around lines966–994).

The generated20-row regression first creates a valid conversion, removes only its completion receipt, replaces the first committed chunk with a valid native artifact whose bid and ask prices are each shifted by one profile tick, preserving parser/source/archive/profile/sequence identities, and resumes. `convert` returns success and publishes a new receipt. The source Parquet and catalog are unchanged. A native checksum and a new output hash establish internal consistency, not equivalence to the source prefix.

Required fix belongs to Hermes: verify adopted output ticks against the exact converted source prefix, or use a separately trustworthy producer-bound recovery design. A self-hashed chunk or newly recomputed receipt is insufficient. Keep the adversarial regression mandatory.

## P1 — completed-output reuse ignores the newly requested conversion scope

`convert` returns `verify_output(output_dir, None)` as soon as manifest.json exists (around lines850–855), before validating the requested source fingerprint/catalog identity or comparing its requested profile with the receipt. On the generated fixture, a completed NZDUSD output is returned successfully for a valid EURUSD profile and a nonexistent different Parquet source path.

Required fix belongs to Hermes: bind idempotent reuse to the exact requested source/configuration/profile/evidence contract and refuse mismatches. Returning a self-consistent existing receipt alone is not verification of the requested conversion.

## Reproduction

Apply `pr6-integrity-regressions.patch` to the exact reviewed source in a disposable review branch and run:

    cargo test --locked --lib codex_review_

Observed:0 passed,2 failed,112 filtered out. Both fail at the intended refusal assertion; compilation and synthetic fixture setup succeed. They use20 generated rows, not real historical quotes. No real-data acceptance, independent source approval or full security review is claimed.

## Additional readiness concerns, not dynamically reproduced here

`read_group` uses the Parquet-declared entire row-group count for multiple Vec::with_capacity/read_records calls (around lines332–396). A512MiB compressed-file ceiling is not a decoded row/string-memory bound, and the chunk budget does not bound these allocations. Add explicit row/decoded-byte preflight or genuinely bounded row-group windows before claiming bounded untrusted-input memory.

The converter's default500000-row chunks exceed the current native corpus admission cap100000. Per-partition sequences restart at0 and its source description embeds per-partition hashes, while the current multi-artifact corpus requires globally increasing sequence and consistent source context. Before full backfill, agree an end-to-end assembly/chunk/provenance contract; do not weaken corpus validation to hide this mismatch. These are integration concerns, not a claim that the fresh sample conversions have wrong prices.

## Coordination

Hermes owns the conversion implementation and source ingestion. Codex's independent feature PRs remain separate; this review does not merge or silently fix the other developer's work. Owner/Hermes disposition of these findings is needed before treating resumed conversion as verified source equivalence. Central project ledgers are unchanged.

## Latest-head applicability

Hermes subsequently pushed `4ebb54baf7d8f741f53bf752572d5a484ec832a2` (canonical planner/batch runner). Exact function-body comparison confirms `rebuild_state`, `convert` and `verify_output` are unchanged from the reproduced86c172c source. The two confirmed findings therefore still apply to that head; the new batch implementation itself has not received a complete review here.
