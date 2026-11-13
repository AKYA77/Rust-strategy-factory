# MACD and Bollinger cross operators

`causal-primitives-v3-macd-bands` adds typed `MacdCross` and `BollingerCross` rules. Existing primitives retain their earlier conventions. These operators are implemented here without copying an external library or adding a runtime dependency.

MACD uses fast/slow EMA differences and a signal EMA, each seeded by its first complete floating SMA. Exact i128 subtraction centers input prices on the first observation before floating computation. This preserves sensitivity to small changes above f64's exact absolute-integer domain and avoids large common-price cancellation. Warmup is explicit. The raw primitive exposes floating line/signal/histogram arrays. The trading predicate rounds histogram values to whole price units, then detects a crossing of the quantized sign; tiny floating residues do not create raw-zero crossings. This is a declared signal convention, separate from the older integer-mean EMA primitive.

Bollinger bands use a rolling population variance (divisor window, not window-1). Centered sums/squares and the variance numerator are checked i128 arithmetic. Sqrt/multiplier conversion is floating; final bands round half-up to the integer price-unit lattice before comparison. Unrepresentable bands or arithmetic overflow return errors; no negative-variance clamp or saturating price output hides failure. Rolling work is O(events), rather than a fresh window scan per event. This is an algorithmic property, not a measured throughput claim.

`MacdCross` fields are `fast`, `slow`, `signal_window`, `above`; fast>=2, slow>fast, signal_window>=2, with windows <=100000. `BollingerCross` fields are `window`, `multiple_bps`, `upper`; window>=2, multiplier 1..100000 basis points. Crosses require complete current and previous indicator values. Above/upper and below/lower directions use strict current comparisons with inclusive previous boundaries. All calculations use closed primary candles; ordinary higher-timeframe wrapping is also supported.

Example rules:

```json
{"MacdCross":{"fast":12,"slow":26,"signal_window":9,"above":true}}
```

```json
{"BollingerCross":{"window":20,"multiple_bps":20000,"upper":true}}
```

Every seventh expanded region adds suitable MACD or band conditions to selected family variants. It does not alter the initial family grid. Canonical parameter probes now include signal_window under `canonical-first-six-scalar-rule-neighbors-v2`; actual derived cases remain frozen in protocol definitions/trial budgets before OOS use. No operator or parameter is fitted on final data.

Four focused tests check independent rational MACD values and rounded population bands, prefix invariance, exact large-integer translation, fail-closed invalid/overflow cases, autonomous variants and actual nonzero subsequent-event tick execution. Both individual cross fixtures independently yield entry100450, exit100750, 16660 micro-lots and $4.998 net under the explicit synthetic account/profile/cost assumptions. This is software verification, not real market performance or qualification. ATR stop policies, dependence-aware inference and independent acceptance remain unfinished.
