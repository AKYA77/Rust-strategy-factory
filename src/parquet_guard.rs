//! Allocation-free compact-Thrift preflight for our restricted Parquet baseline.
//! It bounds collection counts/depth/binary sizes before upstream metadata/page allocation.
use crate::Result;
pub(crate) struct Compact<'a> {
    bytes: &'a [u8],
    pub pos: usize,
    steps: usize,
}
impl<'a> Compact<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            steps: 0,
        }
    }
    fn byte(&mut self) -> Result<u8> {
        let b = *self
            .bytes
            .get(self.pos)
            .ok_or("Parquet compact header truncated")?;
        self.pos += 1;
        Ok(b)
    }
    fn take(&mut self, n: usize) -> Result<()> {
        self.pos = self
            .pos
            .checked_add(n)
            .filter(|n| *n <= self.bytes.len())
            .ok_or("Parquet compact length exceeds captured bytes")?;
        Ok(())
    }
    fn uint(&mut self) -> Result<u64> {
        let mut n = 0u64;
        for i in 0..10 {
            let b = self.byte()?;
            if i == 9 && b > 1 {
                return Err("Parquet compact integer overflow".into());
            }
            n |= u64::from(b & 127) << (7 * i);
            if b & 128 == 0 {
                return Ok(n);
            }
        }
        Err("Parquet compact integer overflow".into())
    }
    fn integer(&mut self) -> Result<i64> {
        let n = self.uint()?;
        Ok(((n >> 1) as i64) ^ -((n & 1) as i64))
    }
    fn field(&mut self, last: &mut i16, seen: &mut u128) -> Result<Option<(i16, u8)>> {
        self.steps += 1;
        if self.steps > 120000 {
            return Err("Parquet compact work budget exceeded".into());
        }
        let b = self.byte()?;
        if b == 0 {
            return Ok(None);
        }
        let kind = b & 15;
        let id = if b >> 4 == 0 {
            i16::try_from(self.integer()?).map_err(|_| "Parquet field overflow")?
        } else {
            last.checked_add(i16::from(b >> 4))
                .ok_or("Parquet field overflow")?
        };
        if !(1..=127).contains(&id) || kind == 0 || kind > 12 || *seen & (1u128 << id) != 0 {
            return Err("Parquet unsupported/duplicate field".into());
        }
        *seen |= 1u128 << id;
        *last = id;
        Ok(Some((id, kind)))
    }
    fn count(&mut self, n: u64) -> Result<usize> {
        if n > 4096 {
            return Err("Parquet compact collection budget exceeded".into());
        }
        Ok(n as usize)
    }
    fn skip(&mut self, kind: u8, depth: usize, in_collection: bool) -> Result<()> {
        self.steps += 1;
        if depth > 16 || self.steps > 120000 {
            return Err("Parquet compact depth/work budget exceeded".into());
        }
        match kind {
            1 | 2 => {
                if in_collection && !matches!(self.byte()?, 1 | 2) {
                    return Err("Parquet compact boolean invalid".into());
                }
            }
            3 => self.take(1)?,
            4..=6 => {
                self.uint()?;
            }
            7 => self.take(8)?,
            8 => {
                let n = self.uint()?;
                if n > 2 * 1024 * 1024 {
                    return Err("Parquet compact binary budget exceeded".into());
                }
                self.take(n as usize)?;
            }
            9 | 10 => {
                let b = self.byte()?;
                let size = if b >> 4 == 15 {
                    self.uint()?
                } else {
                    u64::from(b >> 4)
                };
                let n = self.count(size)?;
                for _ in 0..n {
                    self.skip(b & 15, depth + 1, true)?;
                }
            }
            11 => {
                let size = self.uint()?;
                let n = self.count(size)?;
                if n > 0 {
                    let b = self.byte()?;
                    for _ in 0..n {
                        self.skip(b >> 4, depth + 1, true)?;
                        self.skip(b & 15, depth + 1, true)?;
                    }
                }
            }
            12 => {
                let (mut last, mut seen) = (0, 0);
                while let Some((_, kind)) = self.field(&mut last, &mut seen)? {
                    self.skip(kind, depth + 1, false)?;
                }
            }
            _ => return Err("Parquet unsupported compact type".into()),
        }
        Ok(())
    }
    fn schema(&mut self) -> Result<()> {
        let b = self.byte()?;
        let n = if b >> 4 == 15 {
            self.uint()?
        } else {
            u64::from(b >> 4)
        };
        if b & 15 != 12 || n != 5 {
            return Err("Parquet flat schema element budget mismatch".into());
        }
        for (index, expected) in [
            b"rsf_ticks".as_slice(),
            b"timestamp_ms",
            b"sequence",
            b"bid",
            b"ask",
        ]
        .iter()
        .enumerate()
        {
            let (mut last, mut seen) = (0, 0);
            let (mut physical, mut repetition, mut children, mut named) = (None, None, None, false);
            while let Some((id, kind)) = self.field(&mut last, &mut seen)? {
                match id {
                    1 | 3 | 5 => {
                        if kind != 5 {
                            return Err("Parquet invalid schema integer".into());
                        }
                        let value = self.integer()?;
                        match id {
                            1 => physical = Some(value),
                            3 => repetition = Some(value),
                            _ => children = Some(value),
                        }
                    }
                    4 => {
                        if kind != 8 {
                            return Err("Parquet invalid schema name".into());
                        }
                        let n = self.uint()?;
                        if n != expected.len() as u64 {
                            return Err("Parquet schema column name mismatch".into());
                        }
                        let start = self.pos;
                        self.take(n as usize)?;
                        if &self.bytes[start..self.pos] != *expected {
                            return Err("Parquet schema column name mismatch".into());
                        }
                        named = true;
                    }
                    _ => self.skip(kind, 1, false)?,
                }
            }
            if !named
                || (index == 0 && (physical.is_some() || children != Some(4)))
                || (index > 0
                    && (physical != Some(2) || repetition != Some(0) || children.is_some()))
            {
                return Err("Parquet required flat INT64 schema mismatch".into());
            }
        }
        Ok(())
    }
    pub fn footer(mut self) -> Result<()> {
        let (mut last, mut seen) = (0, 0);
        let mut schema = false;
        while let Some((id, kind)) = self.field(&mut last, &mut seen)? {
            if id == 2 {
                if kind != 9 {
                    return Err("Parquet schema list missing".into());
                }
                self.schema()?;
                schema = true;
            } else {
                self.skip(kind, 0, false)?;
            }
        }
        if !schema || self.pos != self.bytes.len() {
            return Err("Parquet missing schema/trailing footer bytes".into());
        }
        Ok(())
    }
    pub fn page(mut self) -> Result<(usize, usize, usize)> {
        let (mut last, mut seen) = (0, 0);
        let (mut page_type, mut raw, mut compressed, mut values, mut encoding) =
            (None, None, None, None, None);
        while let Some((id, kind)) = self.field(&mut last, &mut seen)? {
            match id {
                1..=3 => {
                    if kind != 5 {
                        return Err("Parquet invalid page integer type".into());
                    }
                    let n = self.integer()?;
                    match id {
                        1 => page_type = Some(n),
                        2 => raw = Some(n),
                        _ => compressed = Some(n),
                    }
                }
                5 => {
                    if kind != 12 {
                        return Err("Parquet missing data-page header".into());
                    }
                    let (mut last, mut seen) = (0, 0);
                    while let Some((id, kind)) = self.field(&mut last, &mut seen)? {
                        if id == 1 || id == 2 {
                            if kind != 5 {
                                return Err("Parquet invalid page value/encoding".into());
                            }
                            let n = self.integer()?;
                            if id == 1 {
                                values = Some(n)
                            } else {
                                encoding = Some(n)
                            }
                        } else {
                            self.skip(kind, 1, false)?;
                        }
                    }
                }
                _ => self.skip(kind, 1, false)?,
            }
        }
        let raw = raw.ok_or("Parquet page missing raw length")?;
        let encoded = compressed.ok_or("Parquet page missing encoded length")?;
        let values = values.ok_or("Parquet page missing value count")?;
        if page_type != Some(0)
            || encoding != Some(0)
            || !(1..=8192).contains(&values)
            || raw != values * 8
            || !(1..=1024 * 1024).contains(&encoded)
            || self.pos > 65536
        {
            return Err("Parquet page size/encoding budget exceeded".into());
        }
        Ok((self.pos, encoded as usize, values as usize))
    }
}
