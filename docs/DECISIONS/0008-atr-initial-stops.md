# ATR initial-stop design

Status: PLANNED, not implemented or accepted. Continue implementation; this document is not an approval gate.

Add optional `stop_policy: {"Atr":{"window":14,"multiple_bps":20000}}` to Strategy; omit/skip None to preserve legacy fixed-stop concept serialization. Existing stop_ticks is the minimum distance floor under ATR, still the fixed distance when policy is absent. Validate policy/domain before computation.

Use closed primary bid candles only. Seed Wilder ATR with ceil(sum first-window true ranges / window), then ceil((previous_atr*(window-1)+TR)/window), using checked i128 arithmetic. This explicitly quantized outward policy intentionally differs from an unrounded floating ATR. Multiply outward in basis points, round outward to the profile tick grid and apply strategy/profile distance floors. Insufficient warmup must reject/skip an entry explicitly; no fallback fake ATR. Distances beyond the existing one-million-tick grammar budget must reject the affected entry, rather than clamp silently or abort an earlier window due to a later bar. Prepare only the chronological feature prefix from decision0007. Preserve a constant prepared representation for legacy fixed stops to avoid allocating a per-candle array unnecessarily.

Store the initial raw stop distance in risk::Reservation as an optional, backward-readable audit field. Every new reservation has Some(distance); historical None means unknown, never silently reconstructed. Pending orders freeze this distance and quantity at placement. Pre-fill risk/margin checks use the stored distance and current quotes/FX/cash; they never recompute ATR or resize quantity. Position SL/3R target derives from actual fill plus the frozen distance. Subsequent ATR changes do not trail/reprice an open position. Existing fees, marked equity, initial executable-stop distance checks, liquidity assumptions, expiry/cancellation ordering and uncapped losses remain unchanged. Version the engine for the new behavior and keep the benchmark engine tag dynamic.

Independent fixtures to implement:

- TR5,10,5,5, window2 gives outward ATR None,8,7,6. Tick5 and multiplier15000 with floor2 ticks gives distances None,15,15,10.
- Core EUR test bars (bid100000 at0,100020 at60000,100040 at120000) seed ATR10 at the second closed candle. Window2/multiplier50000 and floor30 produces distance50. A subsequent quote entry100090/target100240 yields quantity19210 and net $2.8815 under the declared synthetic costs/profile.
- Pending stop at publication120000: effective trigger100090, planned/fill entry100110, distance50. A downward quote99000 before the next publication raises later ATR to525 (new distance2625), but cancel_on_signal_false=false keeps the existing intent. Quote100070/ask100090 fills it unchanged; later bid100350 reaches target100260, quantity19210, net $2.8815. Reservation receipts must prove identical distance/quantity before/after fill.
- Warmup, grid/floor/domain/overflow checks, future-prefix invariance, prepared-state serialization compatibility, generator/protocol/browser integration and fixed-stop regressions.

No qualifying status, real-data approval or production performance claim follows from implementing this policy.
