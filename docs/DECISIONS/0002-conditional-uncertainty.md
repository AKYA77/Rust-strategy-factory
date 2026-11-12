# Conditional win-rate uncertainty

The versioned `conditional-hoeffding-bonferroni-v1` diagnostic applies only to chronological resolved OOS receipts from independently reset folds. A net win means P&L strictly greater than zero; breakevens are not wins. Empty evidence produces no point estimate or lower bound.

For n receipts, empirical net-win fraction p, declared family size m >= 1 and family error alpha = 0.05, the one-sided lower bound is:

`max(0, p - sqrt((ln(m) - ln(alpha)) / (2*n)))`.

This combines Hoeffding's inequality for independent Bernoulli observations with a Bonferroni union bound over the declared family. The union bound does not require independence between candidates. Independence/common probability within each candidate, complete preregistered multiplicity accounting and the absence of adaptive optional stopping remain prerequisites. Market-trade receipts do not establish those prerequisites. Every diagnostic therefore persists `assumptions_verified: false` and explicit warnings. The bound cannot authorize qualification, even if it exceeds 50%.

The declared trial budget is frozen in the protocol definition. It is not inferred from successful candidates or reset when a process restarts. This diagnostic does not prove that the declared budget covers previous corpus revisions, search runs or external analysis. Corpus-wide exposure/accounting is a separate unfinished requirement. Increasing m weakens the bound. Logs are evaluated separately to avoid intermediate multiplication overflow. Counts remain exact; bound arithmetic is ordinary f64 diagnostic arithmetic, never financial accounting.

The method is dependency-free, deterministic and bounded to one million receipts. Tests independently check a numerical bound, multiplicity effects, monotonicity, empty evidence, breakevens, invalid budgets and order. Actual persisted protocol tests compare diagnostic counts with the OOS receipt totals. Final-holdout results never enter these diagnostics. The protocol definition includes the diagnostic method version; old immutable reports lacking the new optional field remain readable, without retroactively adding evidence.

Still required: dependence-aware inference, robust independent expectancy, session/regime stability, annual coverage, cost/parameter sensitivity and complete search accounting. A conditional interval must never be presented as unconditional evidence of profitability.
