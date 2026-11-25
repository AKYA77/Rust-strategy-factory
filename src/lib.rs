pub mod candles;
pub mod contracts;
pub mod data;
pub mod indicators;
pub mod sparse_mask;
pub mod strategies;
pub mod tick_engine;

pub type Result<T> = std::result::Result<T, String>;
pub mod indexed_policy;
pub mod indexed_replay;
pub mod orders;
pub mod risk;
pub mod storage;
pub mod tick_index;
pub mod validation;

pub const ENGINE_VERSION: &str = "ordered-l1-v6-frozen-atr-stops";
pub const INDICATOR_VERSION: &str = "causal-primitives-v4-outward-atr";
pub const SIGNAL_VERSION: &str = "closed-publication-next-event-v3-scoped-features";
pub mod candidate_screen;
pub mod daily_checkpoint;
pub mod dashboard;
pub mod data_validation;
pub mod economic_calendar;
pub mod proof_screen;
pub mod research;
pub mod science;
pub mod screen;

pub mod uncertainty;

pub mod robustness;

pub mod diagnostics;

pub mod regimes;

pub mod native;

pub mod neighborhood;

pub mod stops;

pub mod improvement;

pub mod training_review;

pub mod training_diagnostics;

pub mod exploration;

pub mod behavior;

pub mod compressed;

pub mod parquet_store;

pub mod historical_migration;

pub mod historical_corpus;

pub mod historical_catalog;

pub mod coverage;

pub mod ordered_coverage;

pub mod stage_machine;

pub mod telemetry;

pub mod import_job;

pub mod catalog;

pub mod data_review;

pub mod calendar_evidence;

pub mod archive_evidence;

pub mod machine_evidence;

pub mod data_approval;
pub mod review_authority;

pub mod corpus;

pub mod streaming;

pub mod resource;

pub mod corpus_search;

mod coordinator;
pub mod corpus_fleet;
pub mod operator_config;

pub mod result_artifact;

pub mod disk;

mod search_control;

pub mod corpus_failure;

pub mod retry_policy;

mod retry_audit;

pub mod supervision_session;

pub mod owned_process;

pub mod owned_history;

pub mod fleet_control;

pub mod fleet_loop;

pub mod blocked_native;

pub mod win_evidence;

pub mod evidence_gates;

pub mod recovery;

pub mod corpus_campaign;

pub mod campaign_rotation;

pub mod worker_selection;

pub mod os_enforcement;

pub mod qualification;

pub mod prep_cache;
pub mod scientific_validation;
