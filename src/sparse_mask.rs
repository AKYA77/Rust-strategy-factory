//! Density-adaptive bitset storage for causal signal masks.
//!
//! The dense word representation of [`crate::indicators::Mask`] is right for
//! high-density conditions, but a long history with few active signals still
//! pays for every 64-bit word. This type keeps a sorted, deduplicated index
//! list when that is smaller and falls back to dense words otherwise. The
//! choice is made from measured density at construction, is part of the
//! serialized identity, and is validated on read, so equivalent inputs always
//! produce the same bytes.
//!
//! Logical behaviour (membership, counting, prefix comparison, Boolean
//! combination, complement) matches [`crate::indicators::Mask`] exactly; the
//! tests use that type as an independent oracle. No causal, receipt or
//! eligibility semantics are attached to this container.
use crate::{Result, indicators};
use serde::{Deserialize, Serialize};

/// Serialized representation tag exposed for honest reporting/evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    Sparse,
    Dense,
}

/// A bitset mask over `0..len` with density-adaptive backing.
///
/// Exactly one of `words`/`indices` is populated, except for the canonical
/// empty mask (`len == 0`, both empty). Sparse indices are `u32`, so sparse
/// storage is only available while `len <= u32::MAX`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparseMask {
    len: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    words: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    indices: Vec<u32>,
}

impl SparseMask {
    /// Bytes per sparse index versus bytes per dense 64-bit word. Sparse wins
    /// below roughly two active bits per word (~3% density).
    const INDEX_BYTES: usize = 4;
    const WORD_BYTES: usize = 8;

    /// Build from boolean cells, choosing the smaller backing representation.
    pub fn from_bools(values: impl IntoIterator<Item = bool>) -> Self {
        let mut words = Vec::new();
        let mut indices = Vec::new();
        let mut len = 0_usize;
        for value in values {
            if len.is_multiple_of(64) {
                words.push(0);
            }
            if value {
                words[len / 64] |= 1 << (len % 64);
                if let Ok(index) = u32::try_from(len) {
                    indices.push(index);
                }
            }
            len += 1;
        }
        if Self::sparse_is_smaller(len, indices.len()) && len <= u32::MAX as usize {
            words.clear();
        } else {
            indices.clear();
        }
        Self {
            len,
            words,
            indices,
        }
    }

    /// Build from active indices. Indices may arrive unsorted or duplicated;
    /// they are normalized deterministically. Out-of-range indices refuse.
    pub fn from_indices(len: usize, indices: impl IntoIterator<Item = usize>) -> Result<Self> {
        let mut indices = indices
            .into_iter()
            .map(|index| {
                u32::try_from(index).map_err(|_| "sparse mask index exceeds u32".to_string())
            })
            .collect::<Result<Vec<_>>>()?;
        indices.sort_unstable();
        indices.dedup();
        if indices.last().is_some_and(|&index| index as usize >= len) {
            return Err("sparse mask index out of range".into());
        }
        if Self::sparse_is_smaller(len, indices.len()) && len <= u32::MAX as usize {
            Ok(Self {
                len,
                words: Vec::new(),
                indices,
            })
        } else {
            let mut words = vec![0_u64; len.div_ceil(64)];
            for &index in &indices {
                words[index as usize / 64] |= 1 << (index as usize % 64);
            }
            Ok(Self {
                len,
                words,
                indices: Vec::new(),
            })
        }
    }

    /// Build from dense words, normalizing padding and choosing the smaller
    /// canonical representation.
    pub fn from_words(len: usize, mut words: Vec<u64>) -> Result<Self> {
        if words.len() != len.div_ceil(64) {
            return Err("invalid sparse mask backing length".into());
        }
        let bits = len % 64;
        if bits > 0
            && let Some(last) = words.last_mut()
        {
            *last &= (1_u64 << bits) - 1;
        }
        let count = words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum::<usize>();
        if Self::sparse_is_smaller(len, count) && len <= u32::MAX as usize {
            let indices = Self::indices_of_words(&words);
            Ok(Self {
                len,
                words: Vec::new(),
                indices,
            })
        } else {
            Ok(Self {
                len,
                words,
                indices: Vec::new(),
            })
        }
    }

    /// Dense word backing exactly as [`indicators::Mask`] stores it.
    pub fn from_mask(mask: &indicators::Mask) -> Result<Self> {
        Self::from_words(mask.len(), mask.words().to_vec())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn count(&self) -> usize {
        if self.indices.is_empty() {
            self.words
                .iter()
                .map(|word| word.count_ones() as usize)
                .sum()
        } else {
            self.indices.len()
        }
    }

    pub fn get(&self, index: usize) -> bool {
        if index >= self.len {
            return false;
        }
        if self.indices.is_empty() {
            self.words
                .get(index / 64)
                .is_some_and(|word| word & (1 << (index % 64)) != 0)
        } else {
            self.indices.binary_search(&(index as u32)).is_ok()
        }
    }

    /// Validate backing lengths, padding, ordering and canonical representation.
    pub fn validate(&self) -> Result<()> {
        if self.words.is_empty() && self.indices.is_empty() {
            if self.len == 0 {
                return Ok(());
            }
            return Err("empty sparse mask backing for nonzero length".into());
        }
        if !self.words.is_empty() && !self.indices.is_empty() {
            return Err("ambiguous sparse mask backing".into());
        }
        if self.len > u32::MAX as usize && !self.indices.is_empty() {
            return Err("sparse mask indices exceed addressable length".into());
        }
        let count = if self.indices.is_empty() {
            if self.words.len() != self.len.div_ceil(64) {
                return Err("invalid sparse mask backing length".into());
            }
            let bits = self.len % 64;
            if bits > 0
                && self
                    .words
                    .last()
                    .is_some_and(|word| word & (!0_u64 << bits) != 0)
            {
                return Err("nonzero sparse mask padding".into());
            }
            self.words
                .iter()
                .map(|word| word.count_ones() as usize)
                .sum()
        } else {
            if self.indices.len() > self.len {
                return Err("sparse mask indices exceed length".into());
            }
            let mut previous = None;
            for &index in &self.indices {
                if index as usize >= self.len {
                    return Err("sparse mask index out of range".into());
                }
                if previous.is_some_and(|previous| previous >= index) {
                    return Err("sparse mask indices not strictly ascending".into());
                }
                previous = Some(index);
            }
            self.indices.len()
        };
        if self.indices.is_empty() == Self::sparse_is_smaller(self.len, count) {
            return Err("sparse mask representation is not canonical".into());
        }
        Ok(())
    }

    /// Ascending active indices without allocating an expanded cell vector.
    pub fn set_indices(&self) -> Result<SetIndices<'_>> {
        self.validate()?;
        Ok(SetIndices {
            words: &self.words,
            next_word: 0,
            loaded_word: 0,
            bits: 0,
            sparse: (!self.indices.is_empty()).then_some(self.indices.iter()),
        })
    }

    /// Restrict traversal to a causal prefix.
    pub fn set_indices_prefix(&self, count: usize) -> Result<impl Iterator<Item = usize> + '_> {
        self.validate()?;
        if count > self.len {
            return Err("sparse prefix exceeds mask length".into());
        }
        Ok(self.set_indices()?.take_while(move |&index| index < count))
    }

    /// Compare the first `count` cells with another mask.
    pub fn matches_prefix(&self, other: &Self, count: usize) -> Result<bool> {
        self.validate()?;
        other.validate()?;
        if count > self.len || count > other.len {
            return Err("sparse prefix exceeds mask length".into());
        }
        for index in self.set_indices_prefix(count)? {
            if !other.get(index) {
                return Ok(false);
            }
        }
        for index in other.set_indices_prefix(count)? {
            if !self.get(index) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Logical equality over the whole length.
    pub fn equals_logical(&self, other: &Self) -> Result<bool> {
        if self.len != other.len {
            return Ok(false);
        }
        self.matches_prefix(other, self.len)
    }

    /// Bitwise AND (`and == true`) or OR of equal-length masks.
    pub fn combine(&self, other: &Self, and: bool) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        if self.len != other.len {
            return Err("incompatible sparse masks".into());
        }
        if self.indices.is_empty() && other.indices.is_empty() && !self.words.is_empty() {
            let words = self
                .words
                .iter()
                .zip(&other.words)
                .map(|(a, b)| if and { a & b } else { a | b })
                .collect();
            return Self::from_words(self.len, words);
        }
        if self.len <= u32::MAX as usize {
            let indices = self.combine_indices(other, and)?;
            return Self::from_indices(self.len, indices);
        }
        let mut words = vec![0_u64; self.len.div_ceil(64)];
        for index in self.combine_indices(other, and)? {
            words[index / 64] |= 1 << (index % 64);
        }
        Self::from_words(self.len, words)
    }

    fn combine_indices(&self, other: &Self, and: bool) -> Result<Vec<usize>> {
        if and {
            Ok(self
                .set_indices()?
                .filter(|index| other.get(*index))
                .collect())
        } else {
            let mut indices: Vec<usize> = self.set_indices()?.collect();
            for index in other.set_indices()? {
                if !self.get(index) {
                    indices.push(index);
                }
            }
            indices.sort_unstable();
            Ok(indices)
        }
    }

    /// Bitwise complement within the mask length.
    pub fn complement(&self) -> Result<Self> {
        self.validate()?;
        let mut words = if self.indices.is_empty() {
            self.words.iter().map(|word| !word).collect::<Vec<_>>()
        } else {
            vec![!0_u64; self.len.div_ceil(64)]
        };
        if !self.indices.is_empty() {
            for &index in &self.indices {
                words[index as usize / 64] &= !(1 << (index as usize % 64));
            }
        }
        let bits = self.len % 64;
        if bits > 0
            && let Some(last) = words.last_mut()
        {
            *last &= (1_u64 << bits) - 1;
        }
        Self::from_words(self.len, words)
    }

    /// Active-bit density in basis points; `None` for an empty mask.
    pub fn density_bps(&self) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        Some((self.count() as u128 * 10_000 / self.len as u128).min(u32::MAX as u128) as u32)
    }

    /// Chosen backing representation; `None` for the canonical empty mask.
    pub fn storage(&self) -> Option<Storage> {
        if self.len == 0 {
            None
        } else if self.indices.is_empty() {
            Some(Storage::Dense)
        } else {
            Some(Storage::Sparse)
        }
    }

    /// Convert to the dense shared mask type.
    pub fn to_mask(&self) -> Result<indicators::Mask> {
        self.validate()?;
        if self.indices.is_empty() {
            indicators::Mask::from_words(self.len, self.words.clone())
        } else {
            let words = Self::words_of_indices(self.len, &self.indices);
            indicators::Mask::from_words(self.len, words)
        }
    }

    fn indices_of_words(words: &[u64]) -> Vec<u32> {
        words
            .iter()
            .enumerate()
            .flat_map(|(word, &bits)| {
                let mut remaining = bits;
                std::iter::from_fn(move || {
                    if remaining == 0 {
                        return None;
                    }
                    let bit = remaining.trailing_zeros();
                    remaining &= remaining - 1;
                    Some((word * 64 + bit as usize) as u32)
                })
            })
            .collect()
    }

    fn words_of_indices(len: usize, indices: &[u32]) -> Vec<u64> {
        let mut words = vec![0_u64; len.div_ceil(64)];
        for &index in indices {
            words[index as usize / 64] |= 1 << (index as usize % 64);
        }
        words
    }

    fn sparse_is_smaller(len: usize, count: usize) -> bool {
        // A sparse mask with no active bits cannot be distinguished from the
        // canonical empty mask, so zero-count masks stay dense.
        if len == 0 || count == 0 || len > u32::MAX as usize {
            return false;
        }
        count.saturating_mul(Self::INDEX_BYTES) < len.div_ceil(64).saturating_mul(Self::WORD_BYTES)
    }
}

/// Ascending active-index iterator. Dense words are consumed lazily; sparse
/// indices are yielded directly from the stored slice.
pub struct SetIndices<'a> {
    words: &'a [u64],
    next_word: usize,
    loaded_word: usize,
    bits: u64,
    sparse: Option<std::slice::Iter<'a, u32>>,
}

impl Iterator for SetIndices<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        if let Some(indices) = self.sparse.as_mut() {
            return indices.next().map(|&index| index as usize);
        }
        loop {
            if self.bits != 0 {
                let bit = self.bits.trailing_zeros() as usize;
                self.bits &= self.bits - 1;
                return Some(self.loaded_word * 64 + bit);
            }
            if self.next_word >= self.words.len() {
                return None;
            }
            self.loaded_word = self.next_word;
            self.next_word += 1;
            self.bits = self.words[self.loaded_word];
        }
    }
}
