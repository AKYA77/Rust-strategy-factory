# Conversion Convention Documentation — Non-USD Quotes (Offline Historical Simulation Only)

## Scope

This convention applies **exclusively to offline historical simulation** within the Rust Strategy Factory research pipeline. It governs how quote-currency amounts for non-USD-quoted instruments (USDJPY, USDCHF, USDCAD) are converted to USD-denominated account units for P&L and position sizing calculations.

**Not authorized for:**
- Live trading or broker access
- External data acquisition
- Weakened scientific validation
- Automatic qualification of strategies
- Any production deployment without explicit owner approval

## Provenance

The convention derives each conversion rate from the instrument's own verified quote price at each tick event, using only data already present in the retained historical corpus. No external rates, no broker feeds, no identity (USD=USD) substitution for non-USD quotes.

The conversion function `Conversion::usd_account_from_usd_base_price(&profile, price_ms_units, event_ms)` is defined in `src/contracts.rs` and is called by `Driver::step()` during research execution. It requires:
- `profile.base == "USD"` (the instrument's base currency is USD)
- `profile.quote != "USD"` (the instrument's quote currency is not USD)
- `price` in the instrument's native profile units (verified tick price)
- `event_ms` as the tick's `timestamp_ms` (event-aligned)

## Arithmetic

All arithmetic is exact integer arithmetic — no floating point, no rounding, no approximation.

### Price parsing

A decimal price string (e.g. `"150.10"`) is parsed into profile units:

```
whole = integer part
frac  = fractional digits
denom = 10^(number of fractional digits)
units = (whole * denom + frac) * price_scale
price_units = units / denom
```

For example, USDJPY `"150.10"` with `price_scale=1000`:
- whole=150, frac=10, denom=100
- units = (150 * 100 + 10) * 1000 = 15,010,000
- price_units = 15,010,000 / 100 = 150,100

### Conversion rate derivation

```
g = gcd(price_scale, price_units)
usd_num   = price_scale / g
quote_den = price_units / g
```

The resulting conversion rate is `usd_num / quote_den` USD per 1 unit of quote currency.

### Verified synthetic fixtures

| Instrument | price_scale | Price | price_units | gcd | usd_num | quote_den | Rate (USD per quote) |
|------------|-------------|-------|-------------|-----|---------|-----------|---------------------|
| USDJPY | 1,000 | 150.10 JPY/USD | 150,100 | 100 | 10 | 1,501 | 10/1501 = 1/150.1 |
| USDCHF | 100,000 | 0.8550 CHF/USD | 85,500 | 500 | 200 | 171 | 200/171 = 1/0.855 |
| USDCAD | 100,000 | 1.3650 CAD/USD | 136,500 | 500 | 200 | 273 | 200/273 = 1/1.365 |

All rate relationships verified: `usd_num * price_units == quote_den * price_scale` (cross-product equality, exact integer arithmetic).

**Real-corpus profiles differ from these synthetic fixtures.** The retained corpus manifests
(2025-03, verified 2026-11-25) use `price_scale=1000000` for all three instruments, with
`tick=1000` for USDJPY and `tick=10` for USDCHF/USDCAD. The synthetic table above
(`price_scale=1000`/`tick=1` for USDJPY) is a test fixture convention only and must never be
applied to retained corpus ticks: doing so silently yields a 1000x-wrong USDJPY rate that still
passes `Conversion::validate`. The real-corpus evidence, exact rates and the permanent negative
control are in `docs/VERIFICATION/real-corpus-conversion-2026-11-25.md` and
`tests/real_conversion_evidence.rs`.

## Historical timestamp alignment

Each `Conversion` record carries `as_of_ms` set to the tick's `timestamp_ms`. Validation enforces:

1. `as_of_ms <= event_ms` — the conversion must not be from the future relative to the event being priced
2. `event_ms - as_of_ms <= max_age_ms` — the conversion must not be stale beyond the configured maximum age
3. For `max_age_ms = 0` (strict mode), the conversion must be exactly event-aligned (`as_of_ms == event_ms`)

The identity sentinel `Conversion::usd()` (usd_num=1, quote_den=1, as_of_ms=i64::MIN) is **rejected** for all non-USD-quoted instruments. It is valid only for USD-quoted instruments (EURUSD, GBPUSD, AUDUSD, NZDUSD).

## Resource assessment

**No large duplicate JSON files are generated.** Conversion rates are derived from each tick's verified bid price at runtime using exact integer arithmetic. The research loader (`research_inputs`) allocates one `Conversion` per tick — a `Vec<Conversion>` of 24 bytes per tick (3 × i64 fields) plus Vec allocation overhead. This is O(n) memory where n is the dataset tick count, not constant memory.

**Measured conversion memory per partition (real corpus, 22 months each):**

| Instrument | 202504 (peak) | 202608 (typical) | 22-month total (conversion vectors only) |
|------------|---------------|------------------|------------------------------------------|
| USDJPY | 133.0 MB | 49.9 MB | ~1.49 GB |
| USDCHF | 73.3 MB | 26.2 MB | ~0.77 GB |
| USDCAD | 81.0 MB | 26.0 MB | ~1.14 GB |
| EURUSD | 94.0 MB | 25.3 MB | ~1.03 GB |
| GBPUSD | 87.1 MB | 38.8 MB | ~1.19 GB |
| AUDUSD | 73.5 MB | 22.0 MB | ~0.93 GB |
| NZDUSD | 59.5 MB | 28.0 MB | ~0.70 GB |
| **All 7 × 22 months** | — | — | **~6.58 GB** |

**Critical distinction — input-file budget vs RAM budget:**

The 2 GiB input-file budget in `research_inputs()` bounds the **on-disk file size** of loaded corpus partitions, **not** RAM allocation. Conversion vectors are allocated **after** file load, in addition to the raw tick data, candles, feature buffers, and index structures. For the full 7-instrument × 22-month corpus:

- Conversion vectors alone: ~6.58 GB
- Plus tick data: ~9-11 GB additional
- Plus candles, features, masks, indexes: multiple GB more

**This exceeds the 8 GB RAM target.** The full corpus cannot be loaded simultaneously within 8 GB RAM. Instruments or partitions must be loaded sequentially, one at a time, with conversion vectors dropped between instruments. The current 20-month dev config (4 USD-quoted instruments) is the maximum that fits within the RAM target when loaded sequentially.

No per-instrument conversion files are written to disk.

## Coverage reconciliation — 20-month dev config vs 22-month corpus

| | Current dev config | Full corpus | Gap |
|------------|-------------------|-------------|-----|
| Instruments | 4 (EURUSD, GBPUSD, AUDUSD, NZDUSD) | 7 (+USDJPY, USDCHF, USDCAD) | 3 instruments |
| Months per instrument | 20 (202501–202608) | 22 (202501–202610) | 2 months (202609, 202610) |
| Total inputs | 80 | 154 | 74 inputs |
| Conversion memory (sequential) | ~2.0 GB peak | ~6.58 GB cumulative | Exceeds 8 GB RAM if simultaneous |

The dev configuration intentionally limits coverage to 4 USD-quoted instruments × 20 months to stay within the 8 GB RAM target when loading sequentially. Adding USDJPY/USDCHF/USDCAD (owner approval required) and extending to 22 months would exceed the RAM target if loaded simultaneously; sequential loading with per-instrument conversion vector release is required.

## Limitations

1. **Offline simulation only** — this convention is not validated against live broker execution and must not be used for live trading.
2. **Historical corpus only** — conversion rates are derived from historical tick prices; no forward-looking or real-time rate data is used.
3. **Exact arithmetic only** — all conversions use integer GCD reduction; no floating-point approximation is ever introduced.
4. **Owner approval required** — USDJPY/USDCHF/USDCAD are currently excluded from the research service (80 inputs = 4 USD-quoted instruments × 20 months). Adding them requires explicit owner approval of this conversion convention.
5. **No scientific qualification** — no strategy has yet achieved 50+ trades for scientific qualification on any instrument, including those using this conversion convention.
6. **Economic calendar data not implemented** — conversion-based P&L does not account for news events or calendar-driven volatility.
7. **RAM budget exceeded by full corpus** — conversion vectors alone total ~6.58 GB for all 7 instruments × 22 months, exceeding the 8 GB RAM target when loaded simultaneously. The 2 GiB input-file budget bounds file size, not RAM. Sequential per-instrument loading with conversion vector release is required. The current 20-month dev config (4 USD-quoted instruments) is the maximum that fits within 8 GB RAM.
8. **Conversion correctness ≠ pipeline completeness** — verified conversion implementation does not imply the research pipeline is complete. The full-history ordered coverage, registered full-period executor, scientific qualification (50+ trades per strategy), and authenticated dashboard acceptance remain separate, unverified milestones.

## Validation safeguards (all enforced in code)

| Safeguard | Location | Enforced |
|-----------|----------|----------|
| `usd_num > 0` | `Conversion::validate` | Yes |
| `quote_den > 0` | `Conversion::validate` | Yes |
| `as_of_ms <= event_ms` | `Conversion::validate` | Yes |
| `event_ms - as_of_ms <= max_age_ms` | `Conversion::validate` | Yes |
| Identity sentinel rejected for non-USD | `Conversion::validate` | Yes |
| `usd_account_from_usd_base_price` rejects USD-quoted | `src/contracts.rs` | Yes |
| `usd_account_from_usd_base_price` rejects non-USD-base | `src/contracts.rs` | Yes |
| Exact integer cross-product equality | `tests/verify_conversion.rs` | 14 tests, all passing |
