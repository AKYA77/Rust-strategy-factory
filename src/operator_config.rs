//! Bounded operator files, separate from immutable research identities.
use crate::{Result, corpus, corpus_search, resource, tick_engine};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
pub fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("operator config must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("operator config exceeds1 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchFile {
    #[serde(default)]
    pub resource_guard: Option<resource::Guard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_guard: Option<resource::CpuGuard>,
    #[serde(default)]
    pub disk_guard: Option<crate::disk::Guard>,
    pub specification: String,
    pub seed: u64,
    #[serde(default)]
    pub start_index: u64,
    pub maximum_candidates: usize,
    pub queue_width: usize,
    pub budget: tick_engine::CorpusBudget,
    pub limits: tick_engine::CorpusBatchLimits,
}
pub struct LoadedSearch {
    pub file: SearchFile,
    pub specification: corpus::Specification,
    pub base: PathBuf,
    pub costs: tick_engine::Costs,
}
impl LoadedSearch {
    pub fn run(&self) -> corpus_search::SearchRun<'_> {
        corpus_search::SearchRun {
            base: &self.base,
            specification: &self.specification,
            costs: &self.costs,
            budget: self.file.budget,
            seed: self.file.seed,
            start_index: self.file.start_index,
            maximum_candidates: self.file.maximum_candidates,
            queue_width: self.file.queue_width,
        }
    }
}
pub fn load_search(path: &Path) -> Result<LoadedSearch> {
    let file: SearchFile = read(path)?;
    if let Some(guard) = &file.resource_guard {
        guard.validate()?;
    }
    if let Some(guard) = &file.cpu_guard {
        guard.validate()?;
    }
    if let Some(guard) = &file.disk_guard {
        guard.validate()?;
    }
    let specification_path = path
        .parent()
        .unwrap_or(Path::new("."))
        .join(&file.specification);
    let specification = corpus::load(&specification_path)?;
    let base = specification_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    Ok(LoadedSearch {
        file,
        specification,
        base,
        costs: tick_engine::Costs::default(),
    })
}
