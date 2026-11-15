# Foreground preparation sessions: full multi-round CLI pipeline

Both receipts compare the verified round-local worker release with invocation-owned slots, using two native artifacts, three market/stop/limit candidates and one new artifact per round. Each fresh-registry pipeline therefore includes actual checkpoint publication, prefix recomputation and a second bounded round. Three rotated pairs preserve all complete financial receipt fingerprints and registry counts; cached reuse is checked separately.

| Synthetic workload | Before median elapsed | After median elapsed | Before median peak child RSS | After median peak child RSS |
|---|---:|---:|---:|---:|
|20,000 EURUSD quotes, one worker|327.598 ms|242.655 ms|24,980 KiB|24,884 KiB|
|100,000 EURUSD quotes, two workers|899.543 ms|730.852 ms|84,776 KiB|85,360 KiB|

The last-round preparation observations show whether retained worker slots were reused. The independent full receipt hash and trade count must match across every run. The parallel case increases observed child RSS; these measurements establish no universal memory reduction or fair CPU shares. Peak child RSS is not summed concurrent-worker or whole-service memory. OS cache is uncontrolled; no physical-cold claim applies. Repetitions reuse the same three synthetic strategies and are not independent scientific trials.

Reproduce with `python3 scripts/benchmark-preparation.py BEFORE_RELEASE AFTER_RELEASE NEW_OUTPUT.json --new-chunks 1`, adding `--quotes 100000 --workers 2` for the parallel case. Keep the original before binary; binary SHA256 and compute revisions are recorded. Prefix recomputation and full source validation remain included, so this is not fast-seek recovery. Representative real/full-history qualification, production performance and default promotion remain pending.
