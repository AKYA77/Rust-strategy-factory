# Continuous UTC two-year context check

1,051,200 artificial constant EURUSD quotes, one per UTC minute in2025/2026, produce1,389,183 closed M1–D1 candles across33 native artifacts. This tests observed minute-density events, never fabricated intrabar quotes or real source coverage. Flat prices and a false above-mean mask intentionally yield no economic evidence. Exact aggregate refusal, scalar/full receipt equality, five registered resume rounds and separate cached reuse pass.

|Actual fresh-process role|Elapsed observation|Peak child RSS observation|
|---|---:|---:|
|Inventory|2,355.434 ms|11,648 KiB|
|Aggregate minus-one refusal|2,624.640 ms|144,848 KiB|
|Scalar replay|3,232.983 ms|170,020 KiB|
|Registered five-round resume and integrity|19,096.684 ms|181,364 KiB|
|Separate cached reuse and integrity|4,954.838 ms|15,708 KiB|

These are single observations, not repeated representative performance trials. Peak child RSS excludes fixture generation and other processes; it is not total service/host memory or hard enforcement. OS cache is uncontrolled. The JSON receipt binds release bytes and compute identity, exact every-frame inventory and full receipt fingerprints. Cached reuse still validates the source; resume still recomputes saved prefixes.

Reproduce with `python3 scripts/check-full-horizon.py --binary target/release/rust-strategy-factory --output NEW_RECEIPT.json`. All source files/registries are disposable private synthetic fixtures and are removed. This check does not establish real data, source approval, realistic tick density or order activity, qualification, target-VPS acceptance or indexed/codec/default promotion. See DECISIONS/0052-full-utc-horizon-context-admission.md.
