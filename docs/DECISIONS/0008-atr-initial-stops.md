# Frozen ATR initial stops

Implemented in engine `ordered-l1-v6-frozen-atr-stops`, indicator version `causal-primitives-v4-outward-atr`. Software verification remains separate from research qualification.

Optional strategy field:

```json
{"stop_policy":{"Atr":{"window":14,"multiple_bps":20000}}}
```

When present, stop_ticks is a minimum floor; it is not added to ATR. When absent, the original fixed stop and canonical serialization remain unchanged. Strategy/rule/policy JSON rejects unknown fields so a misspelled stop_policy cannot silently select a different model.

Only actually closed primary bid candles contribute. True range is max(high-low, abs(high-previous_close), abs(low-previous_close)); the first bar uses high-low. Seed Wilder ATR with ceil(sum first-window TR / window), then ceil((previous_atr*(window-1)+TR)/window), using checked i128 seed/recurrence arithmetic. This explicitly outward-quantized policy intentionally differs from an unrounded floating ATR and can accumulate upward rounding. It never uses floating monetary distances.

Multiply outward in basis points, round outward to the profile tick grid and apply strategy/profile distance floors. Warmup remains unavailable; it never falls back to the floor. Values beyond the one-million-tick grammar/price representation budget reject the affected entry without clamping or aborting an earlier window. Preparation uses the causal prefix from decision0007. Legacy fixed stops retain a constant prepared representation. Preparation alone does not guarantee feasibility: the risk gate still enforces the minimum from the executable mark after entry friction.

Every new risk reservation stores Some(raw stop distance) in price units, excluding adverse exit slip. Historical missing fields deserialize as None/unknown and are not silently reconstructed. Pending orders freeze this distance and quantity at placement. Pre-fill validation uses the stored distance and current quotes/FX/cash; it never recomputes ATR or resizes. Position SL and planned 3R target derive from actual fill and frozen distance. Later ATR does not trail or reprice an open position. Existing commissions, marked equity, expiry/cancellation order, L1 assumptions and uncapped losses remain in force. Off-grid risk prices/distances are rejected rather than rounded.

Protocol v7 binds effective policies, frozen ATR window/multiplier probes and the robustness method version. Every derived case counts toward the trial budget before OOS execution. Every eleventh expanded autonomous region adds a typed ATR policy. Browser protocol setup also submits ATR together with MACD and higher confirmation. Reports remain INSUFFICIENT_EVIDENCE; probes remain descriptive and never use final data for adaptation.

Verified independent fixtures:

- TR5,10,5,5 with window2 gives ATR None,8,7,6. Tick5/multiplier15000/floor2 ticks gives distances None,15,15,10.
- EUR closed-bar seed ATR10, multiplier50000/floor30 gives distance50. Entry100090/target100240, quantity19210, net $2.8815. Fixed50 replay matches; appended valid extreme future prices cannot change the earlier window.
- Pending entry100110 freezes distance50/quantity19210. Later ATR rises to525 (new distance2625), but the existing intent fills unchanged and reaches target100260 for $2.8815. Both reservation receipts prove the same distance/quantity.
- Warmup/excess-distance/domain failure, historical reservation deserialization, strict fields, autonomous/policy probes, actual frozen OOS replay/trial-budget inclusion and risk-grid rejection all pass. Fixed-stop regressions remain enabled.

No broker/session/coverage approval, independent statistical qualification, full-history or production performance claim follows from these synthetic fixtures.
