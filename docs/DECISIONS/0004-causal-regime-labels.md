# Fixed causal M1 regime bins

`prior-21-closed-m1-trend-range-v1` is a descriptive recipe with no fitted threshold. It compares the latest closed bid M1 close with the mean of the preceding 20 closes, and its high-low range with the mean range of those same 20 candles. Exact i128 cross-products avoid rounded means. Trend is up/down/flat; range is expanded/contracted/equal. This small fixed recipe does not establish comprehensive market-regime robustness.

A label is published on the actual event proving candle closure. Entry classification excludes labels published on the entry event itself, including timestamp ties. Missing one or more M1 buckets resets the rolling context. Insufficient history after a gap, initial warmup and context older than 60 seconds are separate unknown labels. No empty bars or unfinished final candle are invented. UTC continuous research candles remain unapproved as broker/DST calendars.

The streaming classifier keeps 21 bars and a bounded vector of publication summaries (at most two million input events), rather than a full duplicate candle array. One prepared classifier is reused across all frozen candidates in a protocol. This bounds software-test input work; it is not proof of full-history memory/performance acceptance or OS resource enforcement.

Frozen protocol v5 binds this recipe version. Only actual OOS trade receipts populate entry-regime buckets. Labels depend on prior context, not the trade outcome, future quotes or final-holdout performance. Unknown trades remain counted; they are never silently dropped. Regime bucket totals reconcile with the same OOS receipts as annual/session summaries. Profitability in a bin cannot authorize qualification or adaptive final-holdout tuning.

Two tests check strict publication precedence, prefix invariance after changing the entry quote/future data, timestamp ties, stale context, gap reset/recovery, downward trend and an expanded closed range. Actual protocol tests reconcile regime trade counts with pooled OOS counts. Broader regimes, sufficient samples, dependence-aware inference, approved coverage and prespecified acceptance gates remain unfinished.
