# Causal feature scope boundaries

Engine `ordered-l1-v5-causal-feature-scopes` and signal version `closed-publication-next-event-v3-scoped-features` fix a numerical lookahead failure: a valid later quote could overflow full-history indicator arithmetic and fail an earlier, otherwise valid chronological evaluation.

Research compiles only primary candles published strictly before the exclusive evaluation end. Prior candles remain available for warmup; entry/start restrictions still apply. Future mask cells are padded false without evaluating their indicators. Higher compilers truncate to their enclosing primary context's last actual publication event, recursively. Thus a future indicator failure cannot alter an earlier window or consume its feature budget. Full parent dataset/candle integrity still must pass; an invalid or quarantined parent is not accepted just because its defect lies later.

Authoritative replay independently recomputes the relevant prefix. It requires the full expected mask length and a valid backing representation, then checks every relevant bit. Future logical bits supplied by a library caller are irrelevant because execution stops before the exclusive end. No mask equality requirement inside the usable context, order handling or accounting gate is relaxed. Non-windowed replay still checks the complete mask.

Mask backing length and unused padding bits are validated before use; get is defensive against malformed serialized storage and Boolean combination rejects malformed inputs. Two regressions exercise a valid 8e18 future price that causes full-history Bollinger overflow, while preserving the earlier completed $4.998 trade/signal count and higher-context mask, plus malformed serialized masks without index panics.

The benchmark harness now emits the actual compiled engine constant rather than its historical hardcoded v2 label. Existing archived benchmark files remain historical; no new performance measurement is claimed by this correction. Independent statistical qualification and real-data prerequisites remain unfinished.
