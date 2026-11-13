# Experimental full-accounting indexed replay

`indexed_replay::replay` is a build-and-run convenience API; `indexed_replay::Index::build(...).replay(...)` reuses one immutable scoped quote index across candidates. It returns the complete existing `Replay`, an explicit used/fallback flag/reason and query counters. This module is experimental and is not selected by research, batch CLI or scientific defaults. The old `tick_index` remains a separate fixed-barrier prototype.

## Supported exact cases

The fast path supports market entries, including frozen fixed/ATR initial distances, for USD-quoted profiles with literal event rates1/1. Every volume step must produce integral USD millionths per price tick: `contract_units * step_micro_lots * tick` is divisible by `price_scale`. All marked/filled prices remain on that tick grid, so account movement is exactly linear without discarding the P&L rounding contract. Scoped timestamps must be nonnegative and leave room for the24h deadline. The index caps input at2million events.

Pending entries, non-USD quotes, nonliteral USD rates, fractional cash grids and unsupported spans use the sequential path. A failed convenience-API attempt also reruns the authoritative implementation, preserving its error ordering. Direct index construction is a preflight API and may refuse before replay. Every scoped FX/quote is validated; invalid FX outside an exclusive scope is not consumed. An immutable borrow binds quotes, costs, conversion stream and window throughout reuse.

The shared replay prelude verifies dataset, exact source candles, frozen causal mask/prefix, canonical strategy, stop preparation and cost contract for both engines. Costs, risk sizing, price/fee arithmetic and protective levels use the existing functions. No narrower spread, missing fee, loss cap, favorable target improvement, fabricated tick/gap, changed holding rule or liquidity claim is introduced.

## Ordered accounting summaries

Blocks contain actual effective bid/ask extrema plus **ordered** maximum bid drop and ask rise. Combining earlier/later blocks preserves cross-boundary peak-before-trough relationships. This differs from inferring order from OHLC. Full/partial ranges retain exact original event order and original sequence/timestamps.

On the integral cash grid, each position's marked equity is a linear transform of its executable quote. The range summary therefore gives exact internal drawdown and cross-range drawdown from the existing account peak. Commissions remain in their actual entry/exit cash updates. MFE/MAE include the actual exit quote, while equity/peak uses post-fill cash at that quote, matching the reference rather than recording a favorable pre-limit mark. Deadline lookup precedes any protective hit at the first actual quote at/after24h. Target fills remain at the requested level; market/stop/deadline fills use the actual executable quote and separate adverse slippage. Same-quote reopening is blocked; a signal published on an exit event may enter on the next actual sequence, including a timestamp tie.

The result preserves every trade field, balance/marked equity/drawdown, rejection, unresolved/pending state, provenance/fill assumption and evaluation window. Scientific eligibility is unchanged. This API has no registry/campaign policy or independent runtime-audit circuit yet; successful indexed results need caller/test comparison before any promotion into research defaults.

## Evidence and limits

184 Rust tests and formatting/Clippy pass, along with debug/release builds, browser and local operations checks.512 seeded whole-replay comparisons span all8 profiles, both directions, multiple costs/fees, fixed/ATR stops, gaps/ties and exclusive windows. USD eligible cases use the index; others explicitly fall back. Independent all-pairs range oracles cover block edges/order. Long unresolved paths check exact marked drawdown and MFE/MAE with real block skipping. Other regressions cover pending/fractional/nonliteral-rate fallback, forged masks, future FX scope, deadline precedence over a simultaneous gap target, empty windows and full-u64 tied event order.

`indexed-replay-v1` is **full-accounting engine comparison, not factory throughput**.8 synthetic inputs x50000 quotes x2 sides x3 distances,5 rotated mode repetitions: each48-replay batch has5956 nonzero closed trades. Every one of15 full replay fingerprints is equal; these are48 repeated cases, not independent scientific trials. Each indexed mode uses30 eligible paths and18 sequential fallbacks. Caller candle/mask preparation and final hashing are excluded equally; engine validation/accounting is included.

Median setup+replay times: sequential830.881175ms; build-each1044.294126ms (slower, not promoted); shared-index755.517528ms, including48.248413ms median shared build attempts. Build-each setup occurs inside its replay clock; only shared setup is timed separately. All index cost remains in the total. Peak whole-child RSS43148KiB, exit0 and all source-manifest hashes verified. No per-path RSS, registry/screen/persistence/cache-throughput improvement, representative/physical-cold/full-history/VPS result or default promotion is claimed. Factory context binding, independent runtime audits/failure monitoring and full-pipeline measurement remain next work.
