# Worker preparation: measured CLI pipelines

The two JSON receipts compare the previous verified single-pass preparation release with bounded worker-local reuse. Both use three market/stop/limit candidates and three rotated fresh-process pairs through the actual corpus-batch and registry-check CLIs. Every pair preserves full financial receipt fingerprints and registry counts; cached calls are checked separately and excluded from first-route time/CPU/RSS.

| Synthetic workload | Before median elapsed | After median elapsed | Before median peak child RSS | After median peak child RSS |
|---|---:|---:|---:|---:|
|20,000 EURUSD quotes, one worker|284.660 ms|179.957 ms|23,836 KiB|23,888 KiB|
|100,000 EURUSD quotes, two workers|893.994 ms|502.028 ms|83,760 KiB|82,260 KiB|

Each after-run records actual builds/reuses/current-byte checks and the maximum per-worker retained candle count; cached reruns construct no context. Source compute revisions and release binary hashes are recorded. The single-worker case shows a small RSS increase; these observations support no universal memory reduction. Peak child RSS is not total host/service memory or summed concurrent worker RSS. OS caches are uncontrolled, and there is no physical-cold or equal CPU-share claim.

Reproduce using `scripts/benchmark-preparation.py BEFORE_RELEASE AFTER_RELEASE NEW_OUTPUT.json`, with `--quotes 100000 --workers 2` for the second workload. Preserve the actual before binary independently. One symbol and repeated strategies are development fixtures, not representative market coverage or independent scientific trials. No genuine data, profile approval, qualification, full-history acceptance or production/default promotion follows from these timings. Slots currently end at the bounded round; persistent invocation lifecycle and fast-seek remain unfinished.
