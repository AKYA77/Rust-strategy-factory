# Frozen time-block uncertainty sensitivity

An optional scientific plan field `uncertainty_block_ms` freezes one epoch-aligned block duration between1 and90 days before OOS exposure. The protocol definition binds both duration and method. Existing plans and reports omit this optional field, preserving legacy typed receipt hashes. Retuning or removing it after exposure is refused by the existing interval reservations.

Let block b contain n_b resolved exits, W_b net wins and N total resolved exits. Under independence of block scores **conditional on observed block sizes**, arbitrary dependence is permitted inside each block. Hoeffding's bounded-variable inequality gives the one-sided conditional lower bound

`max(0, sum(W_b)/N - sqrt(log(declared_trials/0.05) * sum(n_b^2) / (2*N^2)))`.

Bonferroni adjustment does not require independence between candidates. The target is the conditional expected pooled net-win share for these block contexts; this does not establish a stationary future success probability. Complete search/trial accounting is also unverified. The report retains exact sum(n_b^2), occupied blocks and effective block count N^2/sum(n_b^2). Empty blocks add no sample size. Breakevens are not wins.

This deliberately reports a sensitivity diagnostic with `assumptions_verified=false`. Trades and market regimes can remain dependent across block boundaries; a24-hour holding cap does not prove independence. Conditioning on realized block sizes is a substantive unverified assumption. Neither this calculation, a high trade count, nor synthetic execution can grant qualification. Source coverage, unresolved positions, annual samples, expectancy, full-period equity, prespecified robustness acceptance and diverse ranking remain separate unfinished requirements.

Tests use independently calculated unequal block sizes, duplicate within-block successes, singleton blocks, empty input, immutable protocol reuse and retrospective duration-change refusal. This is an incremental statistical safeguard, not completed dependence-aware scientific qualification.
