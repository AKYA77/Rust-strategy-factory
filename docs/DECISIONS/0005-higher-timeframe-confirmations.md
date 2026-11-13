# Closed higher-timeframe confirmations

The typed rule grammar accepts `HigherTimeframe { timeframe, rule }` inside normal Boolean rules. Every confirmation timeframe must strictly exceed its enclosing timeframe, including nested confirmations. The existing 32-node/four-level rule budgets still apply. Unsupported same/lower timeframe graphs fail canonical strategy validation.

Example rule for an M1 strategy:

```json
{"All":[{"Rising":{"lookback":2}},{"HigherTimeframe":{"timeframe":"H1","rule":{"AboveMean":{"window":20}}}}]}
```

Higher candles are built from the authoritative original tick stream on the same continuous UTC research calendar. A primary signal uses the last higher candle actually published by its `(available_ms, publication_sequence)` decision event. Same-event publication is eligible for that decision; the engine still requires a subsequent quote event for entry. No unfinished higher candle, future close or fabricated gap candle is used. Insufficient higher warmup returns false. These research calendars and feature carry-over across gaps are not approved broker/session rules.

`Features::with_context` supplies the dataset and primary timeframe; legacy `Features::new` remains valid for single-timeframe rules and rejects higher rules without context. Nested compilers receive only the remaining retained-cell budget after the parent cache. Repeated root masks use the bounded cache. This is feature-cache accounting, not hard RSS/OS enforcement; candle construction and data preparation still need measured shared-cache optimization.

The authoritative replay independently rebuilds the expected higher-aware mask, so callers cannot inject a future-bearing or contradictory mask. Its verification allowance now accommodates valid complex bounded graphs (128 retained-cell units per primary candle, rather than the former 34-unit allowance). The caller's configured compilation budget remains separate and enforced. A 30-node/44-mean-feature regression compiles and replays nonzero trades; accepting larger grammar graphs does not relax mask equality or financial checks.

Signal version is `closed-publication-next-event-v2-htf`, bound into experiment/protocol identities. Existing single-timeframe concept definitions retain their canonical shape. Every fifth expanded autonomous region adds a direction-matched 20-period mean confirmation on the next supported higher timeframe; D1 has no higher frame and remains unchanged. The generator still freezes effective concepts before evaluation; confirmations are not fitted on OOS/final outcomes.

Five focused tests cover exact closed-candle joins, same-publication decisions and next-event entries, actual nonzero replay, mask tampering rejection, prefix invariance, missing context, budget refusal, strict/nested hierarchy, autonomous generation and a valid complex graph. An additional frozen-protocol integration actually replays a higher-aware candidate on synthetic OOS windows. Chromium protocol setup also submits a higher-aware plan. No real profile/calendar/coverage approval or qualification is claimed.
