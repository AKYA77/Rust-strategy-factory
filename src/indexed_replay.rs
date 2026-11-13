//! Experimental exact market-entry replay for constant USD, integral cash per tick.
//! Not selected by research/science defaults. Unsupported cases use sequential replay.
use crate::{
    Result,
    candles::Candle,
    contracts::{self, Conversion, Side},
    data::Dataset,
    indicators::Mask,
    strategies::Strategy,
    tick_engine::{self, Costs, ExitReason, OpenPosition, Rejection, Replay, Trade},
};
use serde::Serialize;
const BLOCK: usize = 256;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Summary {
    min_bid: i64,
    max_bid: i64,
    min_ask: i64,
    max_ask: i64,
    bid_drop: i64,
    ask_rise: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_range_summaries_match_independent_all_pairs_oracle() {
        let p = crate::contracts::Profile::synthetic("EURUSD").unwrap();
        let mut seed = 17u64;
        let ticks = (0..700)
            .map(|i| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let bid = 100000 + (seed % 3000) as i64;
                crate::data::Tick {
                    timestamp_ms: i * 10000,
                    sequence: i as u64,
                    bid,
                    ask: bid + (seed % 80) as i64,
                }
            })
            .collect();
        let d = crate::data::dataset(
            p,
            ticks,
            "SYNTHETIC_TEST_ONLY block-summary oracle",
            "generated".into(),
            true,
        )
        .unwrap();
        let costs = Costs::default();
        let fx = vec![Conversion::usd(); d.ticks.len()];
        let index = Index::build(&d, &costs, &fx, None).unwrap();
        let mut work = Work::default();
        for (lo, hi) in [
            (0, 0),
            (0, 1),
            (0, 256),
            (0, 700),
            (255, 513),
            (256, 512),
            (257, 699),
        ] {
            let actual = index.summary(lo, hi, &mut work).unwrap();
            if lo == hi {
                assert!(actual.is_none());
                continue;
            }
            let q = d.ticks[lo..hi]
                .iter()
                .map(|t| costs.effective(&d.profile, *t).unwrap())
                .collect::<Vec<_>>();
            let mut drop = 0;
            let mut rise = 0;
            for i in 0..q.len() {
                for j in i..q.len() {
                    drop = drop.max(q[i].0 - q[j].0);
                    rise = rise.max(q[j].1 - q[i].1);
                }
            }
            let expected = Summary {
                min_bid: q.iter().map(|v| v.0).min().unwrap(),
                max_bid: q.iter().map(|v| v.0).max().unwrap(),
                min_ask: q.iter().map(|v| v.1).min().unwrap(),
                max_ask: q.iter().map(|v| v.1).max().unwrap(),
                bid_drop: drop,
                ask_rise: rise,
            };
            assert_eq!(actual, Some(expected));
        }
        assert!(index.summary(1, 0, &mut work).is_err());
        assert!(index.summary(0, 701, &mut work).is_err());
    }
}
impl Summary {
    fn quote((bid, ask): (i64, i64)) -> Self {
        Self {
            min_bid: bid,
            max_bid: bid,
            min_ask: ask,
            max_ask: ask,
            bid_drop: 0,
            ask_rise: 0,
        }
    }
    fn then(self, next: Self) -> Self {
        Self {
            min_bid: self.min_bid.min(next.min_bid),
            max_bid: self.max_bid.max(next.max_bid),
            min_ask: self.min_ask.min(next.min_ask),
            max_ask: self.max_ask.max(next.max_ask),
            bid_drop: self
                .bid_drop
                .max(next.bid_drop)
                .max(self.max_bid - next.min_bid),
            ask_rise: self
                .ask_rise
                .max(next.ask_rise)
                .max(next.max_ask - self.min_ask),
        }
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Work {
    pub summary_blocks: usize,
    pub summary_quotes: usize,
    pub barrier_blocks: usize,
    pub barrier_quotes: usize,
}
#[derive(Debug, Serialize)]
pub struct Execution {
    pub replay: Replay,
    pub used_index: bool,
    pub fallback_reason: Option<String>,
    pub work: Work,
}
pub struct Index<'a> {
    data: &'a Dataset,
    costs: &'a Costs,
    conversions: &'a [Conversion],
    window: Option<crate::validation::Interval>,
    start: usize,
    end: usize,
    quotes: Vec<(i64, i64)>,
    blocks: Vec<Summary>,
}
fn reference(
    data: &Dataset,
    candles: &[Candle],
    mask: &Mask,
    strategy: &Strategy,
    costs: &Costs,
    conversions: &[Conversion],
    window: Option<&crate::validation::Interval>,
) -> Result<Replay> {
    match window {
        Some(w) => tick_engine::replay_window(data, candles, mask, strategy, costs, conversions, w),
        None => tick_engine::replay(data, candles, mask, strategy, costs, conversions),
    }
}
/// Build+replay convenience path. Any unsupported/failed index attempt falls back
/// to the authoritative path, including its exact error ordering. Successful fast
/// results remain experimental until independently compared by the caller/tests.
pub fn replay(
    data: &Dataset,
    candles: &[Candle],
    mask: &Mask,
    strategy: &Strategy,
    costs: &Costs,
    conversions: &[Conversion],
    window: Option<&crate::validation::Interval>,
) -> Result<Execution> {
    match Index::build(data, costs, conversions, window)
        .and_then(|index| index.replay(candles, mask, strategy))
    {
        Ok(result) => Ok(result),
        Err(reason) => Ok(Execution {
            replay: reference(data, candles, mask, strategy, costs, conversions, window)?,
            used_index: false,
            fallback_reason: Some(reason),
            work: Work::default(),
        }),
    }
}
impl<'a> Index<'a> {
    pub fn build(
        data: &'a Dataset,
        costs: &'a Costs,
        conversions: &'a [Conversion],
        window: Option<&crate::validation::Interval>,
    ) -> Result<Self> {
        data.verify()?;
        costs.validate()?;
        if let Some(w) = window {
            w.validate()?;
        }
        let p = &data.profile;
        if data.ticks.len() > 2_000_000 || conversions.len() != data.ticks.len() {
            return Err("indexed event/conversion budget invalid".into());
        }
        let tick_cash =
            i128::from(p.contract_units) * i128::from(p.step_micro_lots) * i128::from(p.tick);
        if p.quote != "USD" || tick_cash % i128::from(p.price_scale) != 0 {
            return Err(
                "index requires USD and integral cash per tick on every volume step".into(),
            );
        }
        let start = window.map_or(0, |w| {
            data.ticks.partition_point(|t| t.timestamp_ms < w.start_ms)
        });
        let end = window.map_or(data.ticks.len(), |w| {
            data.ticks
                .partition_point(|t| t.timestamp_ms < w.end_exclusive_ms)
        });
        let mut quotes = Vec::with_capacity(end - start);
        for (t, fx) in data.ticks[start..end].iter().zip(&conversions[start..end]) {
            if fx.usd_num != 1
                || fx.quote_den != 1
                || t.timestamp_ms < 0
                || t.timestamp_ms > i64::MAX - contracts::MAX_HOLD_MS
            {
                return Err("index requires literal USD rates and supported epoch span".into());
            }
            fx.validate(p, t.timestamp_ms, costs.max_conversion_age_ms)?;
            quotes.push(costs.effective(p, *t)?);
        }
        let blocks = quotes
            .chunks(BLOCK)
            .map(|part| {
                part.iter()
                    .copied()
                    .map(Summary::quote)
                    .reduce(Summary::then)
                    .expect("nonempty quote block")
            })
            .collect();
        Ok(Self {
            data,
            costs,
            conversions,
            window: window.cloned(),
            start,
            end,
            quotes,
            blocks,
        })
    }
    fn quote(&self, i: usize) -> Result<(i64, i64)> {
        self.quotes
            .get(
                i.checked_sub(self.start)
                    .ok_or("quote precedes index scope")?,
            )
            .copied()
            .ok_or("quote exceeds index scope".into())
    }
    fn bounds(&self, lo: usize, hi: usize) -> Result<(usize, usize)> {
        if lo < self.start || hi > self.end || lo > hi {
            return Err("index range outside scope".into());
        }
        Ok((lo - self.start, hi - self.start))
    }
    fn summary(&self, lo: usize, hi: usize, work: &mut Work) -> Result<Option<Summary>> {
        let (mut i, end) = self.bounds(lo, hi)?;
        let mut out: Option<Summary> = None;
        while i < end {
            let block_end = ((i / BLOCK + 1) * BLOCK).min(self.quotes.len());
            let part = if i % BLOCK == 0 && block_end <= end {
                work.summary_blocks += 1;
                let part = self.blocks[i / BLOCK];
                i = block_end;
                part
            } else {
                work.summary_quotes += 1;
                let part = Summary::quote(self.quotes[i]);
                i += 1;
                part
            };
            out = Some(out.map_or(part, |s| s.then(part)));
        }
        Ok(out)
    }
    fn barrier(
        &self,
        lo: usize,
        hi: usize,
        position: &OpenPosition,
        work: &mut Work,
    ) -> Result<Option<(usize, ExitReason)>> {
        let (mut i, end) = self.bounds(lo, hi)?;
        while i < end {
            let block_end = ((i / BLOCK + 1) * BLOCK).min(self.quotes.len());
            if i % BLOCK == 0 && block_end <= end {
                work.barrier_blocks += 1;
                let s = self.blocks[i / BLOCK];
                let impossible = if position.side == Side::Long {
                    s.min_bid > position.stop && s.max_bid < position.target
                } else {
                    s.max_ask < position.stop && s.min_ask > position.target
                };
                if impossible {
                    i = block_end;
                    continue;
                }
            }
            let (bid, ask) = self.quotes[i];
            work.barrier_quotes += 1;
            let stop = if position.side == Side::Long {
                bid <= position.stop
            } else {
                ask >= position.stop
            };
            let target = if position.side == Side::Long {
                bid >= position.target
            } else {
                ask <= position.target
            };
            if stop {
                return Ok(Some((i + self.start, ExitReason::StopMarket)));
            }
            if target {
                return Ok(Some((i + self.start, ExitReason::TargetLimit)));
            }
            i += 1;
        }
        Ok(None)
    }
    /// Reuse one immutable quote/block index over multiple frozen candidates.
    /// Signal/candle/stop validation remains the same as authoritative replay.
    pub fn replay(
        &self,
        candles: &[Candle],
        mask: &Mask,
        strategy: &Strategy,
    ) -> Result<Execution> {
        if strategy.entry.is_some() {
            return Ok(Execution {
                replay: reference(
                    self.data,
                    candles,
                    mask,
                    strategy,
                    self.costs,
                    self.conversions,
                    self.window.as_ref(),
                )?,
                used_index: false,
                fallback_reason: Some("pending entry uses sequential replay".into()),
                work: Work::default(),
            });
        }
        let prepared = tick_engine::prepare_replay(
            self.data,
            candles,
            mask,
            strategy,
            self.costs,
            self.conversions,
            self.window.as_ref(),
        )?;
        let strategy = &prepared.strategy;
        let p = &self.data.profile;
        let mut account = Account {
            balance: contracts::INITIAL_EQUITY,
            marked: contracts::INITIAL_EQUITY,
            peak: contracts::INITIAL_EQUITY,
            drawdown: 0,
        };
        let mut trades = Vec::new();
        let mut rejections = Vec::new();
        let mut unresolved = None;
        let mut blocked_through = None;
        let mut work = Work::default();
        for i in mask.set_indices_prefix(prepared.visible)? {
            if self
                .window
                .as_ref()
                .is_some_and(|w| !w.contains(candles[i].available_ms))
            {
                continue;
            }
            let event_i = self
                .data
                .ticks
                .partition_point(|t| t.sequence <= candles[i].publication_sequence)
                .max(self.start);
            if event_i >= self.end {
                break;
            }
            if blocked_through.is_some_and(|end| event_i <= end) {
                continue;
            }
            let t = self.data.ticks[event_i];
            let distance = prepared.distances.at(i);
            let crate::stops::Distance::Ready(distance) = distance else {
                rejections.push(Rejection {
                    event: t.sequence,
                    reason: distance.rejection().into(),
                });
                continue;
            };
            let (bid, ask) = self.quote(event_i)?;
            let entry =
                crate::orders::market_from_effective(p, self.costs, bid, ask, strategy.side)?;
            let reservation = crate::risk::size(
                p,
                self.costs,
                crate::risk::Request {
                    equity: account.balance,
                    entry,
                    mark: if strategy.side == Side::Long {
                        bid
                    } else {
                        ask
                    },
                    side: strategy.side,
                    stop_with_exit_slip: distance
                        .checked_add(prepared.slip)
                        .ok_or("risk overflow")?,
                    conversion: self.conversions[event_i],
                },
            );
            let (reservation, (stop, target)) = match reservation
                .and_then(|r| Ok((r, tick_engine::protective(entry, strategy.side, distance)?)))
            {
                Ok(v) => v,
                Err(reason) => {
                    rejections.push(Rejection {
                        event: t.sequence,
                        reason,
                    });
                    continue;
                }
            };
            account.balance -= reservation.entry_fee_micro_usd;
            let mut position = OpenPosition {
                entry_event: t.sequence,
                entry_ms: t.timestamp_ms,
                deadline_ms: t
                    .timestamp_ms
                    .checked_add(contracts::MAX_HOLD_MS)
                    .ok_or("deadline overflow")?,
                side: strategy.side,
                entry,
                stop,
                target,
                volume_micro_lots: reservation.volume,
                entry_commission: reservation.entry_fee_micro_usd,
                mfe: 0,
                mae: 0,
            };
            account.marked = account
                .balance
                .checked_add(contracts::pnl(
                    p,
                    tick_engine::signed_delta(
                        position.side,
                        if position.side == Side::Long {
                            bid
                        } else {
                            ask
                        },
                        entry,
                    )?,
                    position.volume_micro_lots,
                    self.conversions[event_i],
                )?)
                .ok_or("marked equity overflow")?;
            account.observe()?;
            let deadline = self
                .data
                .ticks
                .partition_point(|t| t.timestamp_ms < position.deadline_ms);
            let hit = self.barrier(event_i + 1, deadline.min(self.end), &position, &mut work)?;
            let exit = hit.or_else(|| {
                (deadline < self.end).then_some((deadline, ExitReason::DeadlineMarket))
            });
            let through = exit.map_or(self.end, |(j, _)| j + 1);
            if let Some(s) = self.summary(event_i + 1, through, &mut work)? {
                let (favorable, adverse) = if position.side == Side::Long {
                    (s.max_bid, s.min_bid)
                } else {
                    (s.min_ask, s.max_ask)
                };
                position.mfe =
                    position
                        .mfe
                        .max(tick_engine::signed_delta(position.side, favorable, entry)?);
                position.mae =
                    position
                        .mae
                        .min(tick_engine::signed_delta(position.side, adverse, entry)?);
            }
            // Exit-quote excursions count, but pre-fill executable mark does not
            // enter equity/peak: authoritative replay records post-fill cash there.
            let equity_end = exit.map_or(self.end, |(j, _)| j);
            if let Some(s) = self.summary(event_i + 1, equity_end, &mut work)? {
                account.summarize(p, &position, s)?;
            }
            if let Some((j, reason)) = exit {
                let q = self.data.ticks[j];
                let (bid, ask) = self.quote(j)?;
                let price = if reason == ExitReason::TargetLimit {
                    target
                } else {
                    crate::orders::market_from_effective(
                        p,
                        self.costs,
                        bid,
                        ask,
                        if position.side == Side::Long {
                            Side::Short
                        } else {
                            Side::Long
                        },
                    )?
                };
                let fee = self.costs.commission(position.volume_micro_lots);
                let gross = contracts::pnl(
                    p,
                    tick_engine::signed_delta(position.side, price, entry)?,
                    position.volume_micro_lots,
                    self.conversions[j],
                )?;
                account.balance = account
                    .balance
                    .checked_add(gross)
                    .and_then(|v| v.checked_sub(fee))
                    .ok_or("balance overflow")?;
                trades.push(Trade {
                    entry_event: t.sequence,
                    exit_event: q.sequence,
                    entry_ms: t.timestamp_ms,
                    exit_ms: q.timestamp_ms,
                    side: position.side,
                    entry,
                    exit: price,
                    stop,
                    target,
                    volume_micro_lots: position.volume_micro_lots,
                    net_micro_usd: gross - position.entry_commission - fee,
                    commission_micro_usd: position.entry_commission + fee,
                    reason,
                    mfe_price_units: position.mfe,
                    mae_price_units: position.mae,
                    crossed_gap_ms: q
                        .timestamp_ms
                        .checked_sub(self.data.ticks[j - 1].timestamp_ms)
                        .ok_or("gap overflow")?,
                });
                account.marked = account.balance;
                account.observe()?;
                blocked_through = Some(j);
            } else {
                let (bid, ask) = self.quote(self.end - 1)?;
                account.marked = account
                    .balance
                    .checked_add(contracts::pnl(
                        p,
                        tick_engine::signed_delta(
                            position.side,
                            if position.side == Side::Long {
                                bid
                            } else {
                                ask
                            },
                            entry,
                        )?,
                        position.volume_micro_lots,
                        Conversion::usd(),
                    )?)
                    .ok_or("marked equity overflow")?;
                unresolved = Some(position);
                break;
            }
        }
        Ok(Execution {
            replay: Replay {
                version: crate::ENGINE_VERSION.into(),
                strategy_id: strategy.identity()?,
                dataset_id: self.data.manifest.content_hash.clone(),
                profile_id: self.data.manifest.profile_hash.clone(),
                cost_id: crate::data::hash(self.costs)?,
                conversion_id: crate::data::hash(&self.conversions)?,
                fill_assumption: tick_engine::FILL_ASSUMPTION.into(),
                initial_micro_usd: contracts::INITIAL_EQUITY,
                realized_micro_usd: account.balance,
                final_marked_micro_usd: account.marked,
                maximum_drawdown_micro_usd: account.drawdown,
                trades,
                rejections,
                unresolved,
                pending_entry: None,
                entry_receipts: Vec::new(),
                evaluation_window: self.window.clone(),
            },
            used_index: true,
            fallback_reason: None,
            work,
        })
    }
}
struct Account {
    balance: i128,
    marked: i128,
    peak: i128,
    drawdown: i128,
}
impl Account {
    fn observe(&mut self) -> Result<()> {
        self.peak = self.peak.max(self.marked);
        self.drawdown = self.drawdown.max(
            self.peak
                .checked_sub(self.marked)
                .ok_or("drawdown overflow")?,
        );
        Ok(())
    }
    fn summarize(
        &mut self,
        p: &crate::contracts::Profile,
        pos: &OpenPosition,
        s: Summary,
    ) -> Result<()> {
        let (low, high, drop) = if pos.side == Side::Long {
            (s.min_bid, s.max_bid, s.bid_drop)
        } else {
            (s.max_ask, s.min_ask, s.ask_rise)
        };
        let money = |price| -> Result<i128> {
            self.balance
                .checked_add(contracts::pnl(
                    p,
                    tick_engine::signed_delta(pos.side, price, pos.entry)?,
                    pos.volume_micro_lots,
                    Conversion::usd(),
                )?)
                .ok_or("marked equity overflow".into())
        };
        let low = money(low)?;
        let high = money(high)?;
        self.drawdown = self
            .drawdown
            .max(self.peak.checked_sub(low).ok_or("drawdown overflow")?)
            .max(contracts::pnl(
                p,
                drop,
                pos.volume_micro_lots,
                Conversion::usd(),
            )?);
        self.peak = self.peak.max(high);
        Ok(())
    }
}
