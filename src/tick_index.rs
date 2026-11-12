//! Experimental fixed-barrier first-hit index. Not enabled in authoritative replay.
//! It does not account for FX-dependent equity, MFE/MAE or portfolio transitions.
use crate::{
    Result,
    contracts::{Profile, Side},
    data::Dataset,
    tick_engine::{Costs, ExitReason},
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone)]
struct Block {
    start: usize,
    end: usize,
    min_bid: i64,
    max_bid: i64,
    min_ask: i64,
    max_ask: i64,
}
#[derive(Debug, Clone)]
pub struct Index {
    quotes: Vec<(i64, i64)>,
    timestamps: Vec<i64>,
    blocks: Vec<Block>,
    block_size: usize,
    pub dataset_hash: String,
    pub profile_hash: String,
    pub cost_hash: String,
}
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    pub event_index: usize,
    pub reason: ExitReason,
}
impl Index {
    pub fn build(
        data: &Dataset,
        costs: &Costs,
        block_size: usize,
        max_events: usize,
    ) -> Result<Self> {
        data.verify()?;
        if !(8..=65536).contains(&block_size) || data.ticks.len() > max_events {
            return Err("index block/event budget invalid".into());
        }
        let quotes = data
            .ticks
            .iter()
            .map(|t| costs.effective(&data.profile, *t))
            .collect::<Result<Vec<_>>>()?;
        let mut blocks = Vec::new();
        for start in (0..quotes.len()).step_by(block_size) {
            let end = (start + block_size).min(quotes.len());
            let part = &quotes[start..end];
            blocks.push(Block {
                start,
                end,
                min_bid: part.iter().map(|v| v.0).min().unwrap(),
                max_bid: part.iter().map(|v| v.0).max().unwrap(),
                min_ask: part.iter().map(|v| v.1).min().unwrap(),
                max_ask: part.iter().map(|v| v.1).max().unwrap(),
            });
        }
        Ok(Self {
            timestamps: data.ticks.iter().map(|t| t.timestamp_ms).collect(),
            quotes,
            blocks,
            block_size,
            dataset_hash: data.manifest.content_hash.clone(),
            profile_hash: data.manifest.profile_hash.clone(),
            cost_hash: crate::data::hash(costs)?,
        })
    }
    pub fn validate_profile(&self, p: &Profile) -> Result<()> {
        if self.profile_hash != crate::data::hash(p)? {
            return Err("index/profile mismatch".into());
        }
        Ok(())
    }
    pub fn first_hit(
        &self,
        start: usize,
        side: Side,
        stop: i64,
        target: i64,
        deadline_ms: i64,
    ) -> Result<Option<Hit>> {
        if stop <= 0
            || target <= 0
            || (side == Side::Long && stop >= target)
            || (side == Side::Short && stop <= target)
        {
            return Err("invalid static barriers".into());
        }
        if start >= self.quotes.len() {
            return Ok(None);
        }
        let mut cursor = start;
        for block in &self.blocks[start / self.block_size..] {
            if cursor == block.start && self.timestamps[block.end - 1] < deadline_ms {
                let impossible = match side {
                    Side::Long => block.min_bid > stop && block.max_bid < target,
                    Side::Short => block.max_ask < stop && block.min_ask > target,
                };
                if impossible {
                    cursor = block.end;
                    continue;
                }
            }
            while cursor < block.end {
                let (bid, ask) = self.quotes[cursor];
                let reason = if self.timestamps[cursor] >= deadline_ms {
                    Some(ExitReason::DeadlineMarket)
                } else if (side == Side::Long && bid <= stop)
                    || (side == Side::Short && ask >= stop)
                {
                    Some(ExitReason::StopMarket)
                } else if (side == Side::Long && bid >= target)
                    || (side == Side::Short && ask <= target)
                {
                    Some(ExitReason::TargetLimit)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    return Ok(Some(Hit {
                        event_index: cursor,
                        reason,
                    }));
                }
                cursor += 1;
            }
        }
        Ok(None)
    }
}
