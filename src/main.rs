use rust_strategy_factory::research;
use rust_strategy_factory::storage::Registry;
use rust_strategy_factory::{
    Result,
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, SYMBOLS},
    data::{self, Dataset},
    strategies::{self, Features, Strategy},
    tick_engine::{self, Costs},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 128 * 1024 * 1024 {
        return Err("JSON exceeds 128 MiB input budget".into());
    }
    serde_json::from_reader(std::io::BufReader::new(file)).map_err(|e| e.to_string())
}
fn read_operator_config<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
    rust_strategy_factory::operator_config::read(Path::new(path))
}
fn read_dataset(path: &str) -> Result<Dataset> {
    if Path::new(path).extension().is_some_and(|e| e == "parquet") {
        rust_strategy_factory::parquet_store::read(Path::new(path))
    } else if Path::new(path).extension().is_some_and(|e| e == "rsfb") {
        rust_strategy_factory::blocked_native::read(Path::new(path), 2000000)
    } else if Path::new(path).extension().is_some_and(|e| e == "rsfz") {
        rust_strategy_factory::compressed::read(Path::new(path))
    } else if Path::new(path).extension().is_some_and(|e| e == "rsf") {
        rust_strategy_factory::native::read(Path::new(path))
    } else {
        read_json(path)
    }
}
fn print<T: serde::Serialize>(value: &T) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}
fn save<T: serde::Serialize>(path: &str, value: &T) -> Result<()> {
    // Publish a complete synced inode atomically without replacing an existing artifact.
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let destination = Path::new(path);
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = destination
        .file_name()
        .ok_or("artifact filename missing")?
        .to_string_lossy();
    let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let publication = (|| -> std::io::Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::hard_link(&temp, destination)?;
        fs::File::open(parent)?.sync_all()
    })();
    drop(file);
    let cleanup = fs::remove_file(&temp);
    publication.and(cleanup).map_err(|e| e.to_string())
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ResearchFile {
    #[serde(default)]
    limits: research::Config,
    inputs: Vec<InputFile>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InputFile {
    dataset: String,
    conversions: Option<String>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusBatchFile {
    #[serde(default)]
    resource_guard: Option<rust_strategy_factory::resource::Guard>,
    #[serde(default)]
    disk_guard: Option<rust_strategy_factory::disk::Guard>,
    specification: String,
    strategies: Vec<Strategy>,
    budget: tick_engine::CorpusBudget,
    limits: tick_engine::CorpusBatchLimits,
}
type CorpusSearchFile = rust_strategy_factory::operator_config::SearchFile;
fn research_inputs(config_path: &str) -> Result<(Vec<research::Input>, research::Config)> {
    let config: ResearchFile = read_json(config_path)?;
    if config.inputs.is_empty() || config.inputs.len() > 8 {
        return Err("one to eight input datasets required".into());
    }
    let base = Path::new(config_path).parent().unwrap_or(Path::new("."));
    let mut bytes = 0_u64;
    for input in &config.inputs {
        for path in std::iter::once(&input.dataset).chain(input.conversions.iter()) {
            bytes = bytes
                .checked_add(
                    fs::metadata(base.join(path))
                        .map_err(|e| e.to_string())?
                        .len(),
                )
                .ok_or("input byte overflow")?;
        }
    }
    if bytes > 128 * 1024 * 1024 {
        return Err("loaded research input files exceed 128 MiB budget; full-history streaming remains pending".into());
    }
    let mut inputs = Vec::new();
    for input in config.inputs {
        let dataset: Dataset = read_dataset(
            base.join(input.dataset)
                .to_str()
                .ok_or("non-UTF8 input path")?,
        )?;
        let conversions = if let Some(path) = input.conversions {
            read_json(base.join(path).to_str().ok_or("non-UTF8 conversion path")?)?
        } else if dataset.profile.quote == "USD" {
            vec![Conversion::usd(); dataset.ticks.len()]
        } else {
            return Err(
                "non-USD input requires an explicit event-aligned conversion JSON file".into(),
            );
        };
        inputs.push(research::Input {
            dataset,
            conversions,
        });
    }
    Ok((inputs, config.limits))
}
fn research_driver(config_path: &str, database: &str) -> Result<research::Driver> {
    let (inputs, limits) = research_inputs(config_path)?;
    research::Driver::open(
        Path::new(database),
        inputs,
        limits,
        research::compute_revision()?,
    )
}
/// Signal listeners run independently of synchronous replay; only the main
/// coordinator exits, at its existing post-experiment checkpoint boundary.
fn checkpoint_shutdown() -> Result<(
    tokio::runtime::Runtime,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
)> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = requested.clone();
    #[cfg(unix)]
    runtime.block_on(async {
        use tokio::signal::unix::{SignalKind, signal};
        // Register both streams before publishing the coordinator's readiness line.
        let mut interrupt = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
        let mut terminate = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
        tokio::spawn(async move {
            tokio::select! {_ = interrupt.recv() => {}, _ = terminate.recv() => {}}
            flag.store(true, std::sync::atomic::Ordering::Release);
        });
        Ok::<(), String>(())
    })?;
    #[cfg(not(unix))]
    runtime.spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            flag.store(true, std::sync::atomic::Ordering::Release);
        }
    });
    Ok((runtime, requested))
}

fn run(args: &[String]) -> Result<()> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["validate-protocol", inputs, plan, database] => {
            let (inputs, limits) = research_inputs(inputs)?;
            if inputs.len() != 1 {
                return Err(
                    "each scientific protocol requires exactly one instrument dataset".into(),
                );
            }
            let plan: rust_strategy_factory::science::Plan = read_json(plan)?;
            let mut registry = Registry::open(Path::new(database))?;
            print(&rust_strategy_factory::science::execute(
                &mut registry,
                &inputs[0],
                &plan,
                &research::compute_revision()?,
                &rust_strategy_factory::science::Budget {
                    feature_cells: limits.feature_cells,
                    state_bytes: limits.state_budget_bytes,
                },
            )?)
        }
        ["review-training", inputs, database, id] => {
            if !Path::new(database).is_file() {
                return Err("registry does not exist".into());
            }
            let (inputs, limits) = research_inputs(inputs)?;
            if inputs.len() != 1 {
                return Err("training review requires exactly one input dataset".into());
            }
            if inputs[0].dataset.ticks.len() > limits.max_total_events {
                return Err("training review exceeds configured event budget".into());
            }

            let mut registry = Registry::open(Path::new(database))?;
            print(&rust_strategy_factory::training_review::execute(
                &mut registry,
                &inputs[0],
                id,
                limits.feature_cells,
                limits.state_budget_bytes,
            )?)
        }
        ["training-review-report", database, id] => {
            if !Path::new(database).is_file() {
                return Err("registry does not exist".into());
            }
            let registry = Registry::open(Path::new(database))?;
            print(
                &rust_strategy_factory::training_review::read_report(&registry, id)?
                    .ok_or("training review missing")?,
            )
        }
        ["diagnose", database, id] => {
            if !Path::new(database).is_file() {
                return Err("registry does not exist".into());
            }
            let mut registry = Registry::open(Path::new(database))?;
            print(&rust_strategy_factory::improvement::diagnose(
                &mut registry,
                id,
                &Default::default(),
            )?)
        }
        ["diagnosis-report", database, id] => {
            if !Path::new(database).is_file() {
                return Err("registry does not exist".into());
            }
            let registry = Registry::open(Path::new(database))?;
            print(
                &rust_strategy_factory::improvement::read_report(&registry, id)?
                    .ok_or("diagnosis missing")?,
            )
        }
        ["protocol-report", database, id] => {
            if !Path::new(database).is_file() {
                return Err("registry does not exist".into());
            }
            let registry = Registry::open(Path::new(database))?;
            print(
                &rust_strategy_factory::science::read_report(&registry, id)?
                    .ok_or("scientific report not completed")?,
            )
        }
        ["version"] => print(
            &serde_json::json!({"package":env!("CARGO_PKG_VERSION"),"engine":rust_strategy_factory::ENGINE_VERSION,"indicators":rust_strategy_factory::INDICATOR_VERSION,"signals":rust_strategy_factory::SIGNAL_VERSION,"compute_revision":research::compute_revision()?,"software_status":"SOFTWARE_INCOMPLETE"}),
        ),
        ["auth-init", path] => rust_strategy_factory::dashboard::initialize_auth(Path::new(path)),
        ["serve", database, auth, port, catalog @ ..] if catalog.len() <= 2 => {
            let port = port.parse::<u16>().map_err(|_| "invalid loopback port")?;
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?
                .block_on(rust_strategy_factory::dashboard::serve_with_reviews(
                    Path::new(database),
                    Path::new(auth),
                    port,
                    catalog.first().map(|p| Path::new(*p)),
                    catalog.get(1).map(|p| Path::new(*p)),
                ))
        }
        ["fixture", symbol, output] => {
            let dataset = data::synthetic_fixture(symbol)?;
            save(output, &dataset)?;
            print(&dataset.manifest)
        }
        ["pack-blocks", input, output, rows] => {
            let rows: usize = rows.parse().map_err(|_| "invalid block row count")?;
            rust_strategy_factory::blocked_native::write(
                Path::new(output),
                &read_dataset(input)?,
                rows,
            )?;
            print(&rust_strategy_factory::blocked_native::inspect(
                Path::new(output),
                2000000,
            )?)
        }
        ["block-inspect", path] => print(&rust_strategy_factory::blocked_native::inspect(
            Path::new(path),
            2000000,
        )?),
        ["block-range", path, root, start, count] => {
            let start: usize = start.parse().map_err(|_| "invalid range start")?;
            let count: usize = count.parse().map_err(|_| "invalid range count")?;
            let mut reader = rust_strategy_factory::blocked_native::Reader::open(
                Path::new(path),
                2000000,
                Some(root),
            )?;
            print(
                &serde_json::json!({"root":root,"start":start.to_string(),"count":count.to_string(),"events":reader.range(start,count)?.iter().map(|t|serde_json::json!({"timestamp_ms":t.timestamp_ms.to_string(),"sequence":t.sequence.to_string(),"bid":t.bid.to_string(),"ask":t.ask.to_string()})).collect::<Vec<_>>(),"scope":"Pinned structural row window, not a complete source verification or approval."}),
            )
        }
        ["unpack-blocks", input, output] => save(
            output,
            &rust_strategy_factory::blocked_native::read(Path::new(input), 2000000)?,
        ),
        ["profiles"] => print(
            &SYMBOLS
                .iter()
                .map(|s| Profile::synthetic(s))
                .collect::<Result<Vec<_>>>()?,
        ),
        ["generate"] => print(&strategies::generate()),
        ["import", input, profile, source, output, category] => {
            if !["real", "synthetic"].contains(category) {
                return Err("category must be real or synthetic".into());
            }
            let p: Profile = read_json(profile)?;
            let data = data::import_csv(
                Path::new(input),
                p,
                source,
                *category == "synthetic",
                64 * 1024 * 1024,
                1_000_000,
            )?;
            save(output, &data)?;
            print(&data.manifest)?;
            data.verify()
        }
        [
            "import-step",
            input,
            profile,
            source,
            directory,
            category,
            chunk_events,
        ] => {
            if !["real", "synthetic"].contains(category) {
                return Err("category must be real or synthetic".into());
            }
            print(&rust_strategy_factory::import_job::step(
                Path::new(input),
                Path::new(directory),
                read_json(profile)?,
                source,
                *category == "synthetic",
                chunk_events.parse().map_err(|_| "invalid chunk events")?,
            )?)
        }
        ["import-export", directory, output] => rust_strategy_factory::native::write(
            Path::new(output),
            &rust_strategy_factory::import_job::assemble(Path::new(directory))?,
        ),
        ["import-status", directory] => print(&rust_strategy_factory::import_job::status(
            Path::new(directory),
        )?),
        [
            "historical-convert",
            source,
            output,
            profile,
            manifest,
            import_receipt,
            chunk_events,
        ] => {
            let profile: Profile = read_json(profile)?;
            let config = rust_strategy_factory::historical_migration::Config::new(
                Path::new(source),
                Path::new(output),
                profile,
            )?
            .with_manifest(Path::new(manifest))?
            .with_import_receipt(Path::new(import_receipt))?
            .with_chunk_events(chunk_events.parse().map_err(|_| "invalid chunk events")?)?;
            print(&rust_strategy_factory::historical_migration::convert(
                &config,
            )?)
        }
        [
            "historical-convert",
            source,
            output,
            profile,
            manifest,
            import_receipt,
        ] => {
            let profile: Profile = read_json(profile)?;
            let config = rust_strategy_factory::historical_migration::Config::new(
                Path::new(source),
                Path::new(output),
                profile,
            )?
            .with_manifest(Path::new(manifest))?
            .with_import_receipt(Path::new(import_receipt))?;
            print(&rust_strategy_factory::historical_migration::convert(
                &config,
            )?)
        }
        ["historical-convert", source, output, profile, manifest] => {
            let profile: Profile = read_json(profile)?;
            let config = rust_strategy_factory::historical_migration::Config::new(
                Path::new(source),
                Path::new(output),
                profile,
            )?
            .with_manifest(Path::new(manifest))?;
            print(&rust_strategy_factory::historical_migration::convert(
                &config,
            )?)
        }
        ["historical-convert", source, output, profile] => {
            let profile: Profile = read_json(profile)?;
            let config = rust_strategy_factory::historical_migration::Config::new(
                Path::new(source),
                Path::new(output),
                profile,
            )?;
            print(&rust_strategy_factory::historical_migration::convert(
                &config,
            )?)
        }
        ["historical-verify", directory, source] => {
            print(&rust_strategy_factory::historical_migration::verify_output(
                Path::new(directory),
                Some(Path::new(source)),
            )?)
        }
        ["historical-verify", directory] => {
            print(&rust_strategy_factory::historical_migration::verify_output(
                Path::new(directory),
                None,
            )?)
        }
        ["historical-plan", root, output] => {
            let partitions = rust_strategy_factory::historical_migration::plan(Path::new(root))?;
            let rows = partitions.iter().map(|p| p.rows).sum::<u64>();
            save(output, &partitions)?;
            print(&serde_json::json!({"planned": partitions.len(), "rows": rows}))
        }
        [
            "historical-batch",
            root,
            output_root,
            profiles,
            workers,
            chunk_events,
            limit,
        ] => {
            let report = rust_strategy_factory::historical_migration::run_batch(
                Path::new(root),
                Path::new(output_root),
                Path::new(profiles),
                workers.parse().map_err(|_| "invalid worker count")?,
                chunk_events.parse().map_err(|_| "invalid chunk events")?,
                Some(limit.parse().map_err(|_| "invalid limit")?),
            )?;
            print(&report)
        }
        [
            "historical-batch",
            root,
            output_root,
            profiles,
            workers,
            chunk_events,
        ] => {
            let report = rust_strategy_factory::historical_migration::run_batch(
                Path::new(root),
                Path::new(output_root),
                Path::new(profiles),
                workers.parse().map_err(|_| "invalid worker count")?,
                chunk_events.parse().map_err(|_| "invalid chunk events")?,
                None,
            )?;
            print(&report)
        }
        ["pack-parquet", input, output] => {
            rust_strategy_factory::parquet_store::write(Path::new(output), &read_dataset(input)?)
        }
        ["unpack-parquet", input, output] => save(
            output,
            &rust_strategy_factory::parquet_store::read(Path::new(input))?,
        ),
        ["compress-native", input, output] => {
            rust_strategy_factory::compressed::compress(Path::new(input), Path::new(output))
        }
        ["decompress-native", input, output] => {
            rust_strategy_factory::compressed::decompress(Path::new(input), Path::new(output))
        }
        ["pack-native", input, output] => {
            let dataset = read_dataset(input)?;
            rust_strategy_factory::native::write(Path::new(output), &dataset)?;
            print(&dataset.manifest)
        }
        ["unpack-native", input, output] => {
            let dataset = rust_strategy_factory::native::read(Path::new(input))?;
            save(output, &dataset)?;
            print(&dataset.manifest)
        }
        ["catalog-record", dataset, path] => print(
            &rust_strategy_factory::catalog::Catalog::open(Path::new(path))?
                .register(&read_dataset(dataset)?)?,
        ),
        ["catalog-backup", source, destination] | ["catalog-restore", source, destination] => {
            rust_strategy_factory::catalog::Catalog::existing(Path::new(source))?
                .backup(Path::new(destination))
        }
        ["catalog-correction", dataset, path, parent, reason] => {
            print(
                &rust_strategy_factory::catalog::Catalog::open(Path::new(path))?
                    .register_correction(&read_dataset(dataset)?, parent, reason)?,
            )
        }
        ["catalog-review", path, request] => print(
            &rust_strategy_factory::catalog::Catalog::open(Path::new(path))?
                .record_review(read_json(request)?)?,
        ),
        ["catalog-review-report", path, id] => print(
            &rust_strategy_factory::catalog::Catalog::existing(Path::new(path))?
                .review(id)?
                .ok_or("review not found")?,
        ),
        ["catalog-reviews", path, identity, after, limit] => print(
            &rust_strategy_factory::catalog::Catalog::existing(Path::new(path))?.reviews(
                identity,
                if *after == "-" { None } else { Some(after) },
                limit.parse().map_err(|_| "invalid review limit")?,
            )?,
        ),
        ["review-authority-init", reviewers, destination] => {
            rust_strategy_factory::review_authority::Authority::initialize(
                Path::new(destination),
                read_json(reviewers)?,
            )
        }
        ["data-review-decide", catalog, authority, reviewer, request] => {
            use std::io::Read;
            let authority =
                rust_strategy_factory::review_authority::Authority::load(Path::new(authority))?;
            let mut password = String::new();
            std::io::stdin()
                .take(4096)
                .read_to_string(&mut password)
                .map_err(|_| "review password input failed")?;
            let principal =
                authority.authenticate(reviewer, password.trim_end_matches(['\r', '\n']))?;
            print(
                &rust_strategy_factory::catalog::Catalog::open(Path::new(catalog))?.decide(
                    &authority,
                    &principal,
                    read_json(request)?,
                )?,
            )
        }
        ["data-review-state", catalog, authority, identity] => {
            let authority =
                rust_strategy_factory::review_authority::Authority::load(Path::new(authority))?;
            print(
                &rust_strategy_factory::catalog::Catalog::existing(Path::new(catalog))?
                    .review_state(identity, &authority)?,
            )
        }
        [
            "data-eligibility",
            catalog,
            authority,
            machine_id,
            archive,
            dataset,
        ] => {
            let authority =
                rust_strategy_factory::review_authority::Authority::load(Path::new(authority))?;
            print(
                &rust_strategy_factory::catalog::Catalog::existing(Path::new(catalog))?
                    .data_eligibility(
                        &authority,
                        machine_id,
                        Path::new(archive),
                        &read_dataset(dataset)?,
                    )?,
            )
        }
        [
            "machine-admit",
            archive,
            dataset,
            catalog,
            identity,
            declaration,
            claim,
        ] => print(
            &rust_strategy_factory::catalog::Catalog::open(Path::new(catalog))?.admit_machine(
                Path::new(archive),
                &read_dataset(dataset)?,
                identity,
                read_json(declaration)?,
                &read_json(claim)?,
            )?,
        ),
        ["machine-report", catalog, id] => print(
            &rust_strategy_factory::catalog::Catalog::existing(Path::new(catalog))?
                .machine(id)?
                .ok_or("machine admission missing")?,
        ),
        ["machine-revalidate", catalog, id, archive, dataset] => print(
            &rust_strategy_factory::catalog::Catalog::existing(Path::new(catalog))?
                .revalidate_machine(id, Path::new(archive), &read_dataset(dataset)?)?,
        ),
        ["calendar-evidence", dataset, catalog, identity, declaration] => {
            print(&rust_strategy_factory::calendar_evidence::assess(
                &read_dataset(dataset)?,
                &rust_strategy_factory::catalog::Catalog::existing(Path::new(catalog))?
                    .entry(identity)?
                    .ok_or("catalog observation missing")?,
                read_json(declaration)?,
            )?)
        }
        ["archive-evidence", archive, dataset, catalog, identity] => {
            print(&rust_strategy_factory::archive_evidence::assess(
                Path::new(archive),
                &read_dataset(dataset)?,
                &rust_strategy_factory::catalog::Catalog::existing(Path::new(catalog))?
                    .entry(identity)?
                    .ok_or("catalog observation missing")?,
            )?)
        }
        ["catalog-entry", path, id] => print(
            &rust_strategy_factory::catalog::Catalog::existing(Path::new(path))?
                .entry(id)?
                .ok_or("catalog entry not found")?,
        ),
        ["catalog-list", path] => print(
            &rust_strategy_factory::catalog::Catalog::existing(Path::new(path))?.page(None, 25)?,
        ),
        ["catalog-list", path, after, limit] => print(
            &rust_strategy_factory::catalog::Catalog::existing(Path::new(path))?.page(
                if *after == "-" { None } else { Some(after) },
                limit.parse().map_err(|_| "invalid catalog limit")?,
            )?,
        ),
        ["corpus-create-bounded", output, maximum, chunks @ ..] if !chunks.is_empty() => {
            let maximum: usize = maximum
                .parse()
                .map_err(|_| "invalid corpus artifact event cap")?;
            let spec = rust_strategy_factory::corpus::describe(
                &chunks
                    .iter()
                    .map(std::path::PathBuf::from)
                    .collect::<Vec<_>>(),
                maximum,
            )?;
            rust_strategy_factory::corpus::inspect(Path::new("."), &spec)?;
            save(output, &spec)
        }
        ["corpus-create", output, chunks @ ..] if !chunks.is_empty() => {
            let specification = rust_strategy_factory::corpus::describe(
                &chunks
                    .iter()
                    .map(std::path::PathBuf::from)
                    .collect::<Vec<_>>(),
                65536,
            )?;
            // Refuse cross-chunk conflicts before publishing any corpus claim.
            rust_strategy_factory::corpus::inspect(Path::new("."), &specification)?;
            save(output, &specification)
        }
        ["corpus-candles", specification, timeframe, budget, output] => {
            let path = Path::new(specification);
            let tf: Timeframe =
                serde_json::from_value(serde_json::Value::String(timeframe.to_string()))
                    .map_err(|e| e.to_string())?;
            let spec = rust_strategy_factory::corpus::load(path)?;
            let candles = rust_strategy_factory::corpus::candle_context(
                path.parent().unwrap_or(Path::new(".")),
                &spec,
                tf,
                budget.parse().map_err(|_| "invalid candle budget")?,
            )?;
            save(output, &candles)
        }
        [
            "corpus-signals",
            specification,
            strategy,
            candles,
            cells,
            output,
        ] => {
            let path = Path::new(specification);
            let spec = rust_strategy_factory::corpus::load(path)?;
            let strategy: Strategy = read_json(strategy)?;
            let mask = rust_strategy_factory::corpus::signals(
                path.parent().unwrap_or(Path::new(".")),
                &spec,
                &strategy,
                candles.parse().map_err(|_| "invalid candle budget")?,
                cells.parse().map_err(|_| "invalid feature budget")?,
            )?;
            save(
                output,
                &serde_json::json!({"corpus_identity":spec.identity()?,"strategy_identity":strategy.identity()?,"signals":mask,"limitations":"Causal multi-frame signals with bounded contexts; no account replay or qualification."}),
            )
        }
        [
            "corpus-replay",
            specification,
            strategy,
            candles,
            cells,
            receipts,
            output,
        ] => {
            let path = Path::new(specification);
            let spec = rust_strategy_factory::corpus::load(path)?;
            let strategy: Strategy = read_json(strategy)?;
            let budget = rust_strategy_factory::tick_engine::CorpusBudget {
                max_candles: candles.parse().map_err(|_| "invalid candle budget")?,
                feature_cells: cells.parse().map_err(|_| "invalid feature budget")?,
                max_receipts: receipts.parse().map_err(|_| "invalid receipt budget")?,
            };
            save(
                output,
                &rust_strategy_factory::tick_engine::replay_corpus(
                    path.parent().unwrap_or(Path::new(".")),
                    &spec,
                    &strategy,
                    &Costs::default(),
                    budget,
                )?,
            )
        }
        [
            "corpus-step",
            specification,
            strategy,
            database,
            candles,
            cells,
            receipts,
            chunks,
            lease_ms,
        ] => {
            let path = Path::new(specification);
            let spec = rust_strategy_factory::corpus::load(path)?;
            let strategy: Strategy = read_json(strategy)?;
            let costs = Costs::default();
            let budget = rust_strategy_factory::tick_engine::CorpusBudget {
                max_candles: candles.parse().map_err(|_| "invalid candle budget")?,
                feature_cells: cells.parse().map_err(|_| "invalid feature budget")?,
                max_receipts: receipts.parse().map_err(|_| "invalid receipt budget")?,
            };
            let mut registry = Registry::open(Path::new(database))?;
            print(&rust_strategy_factory::tick_engine::execute_corpus_step(
                &mut registry,
                rust_strategy_factory::tick_engine::CorpusRun {
                    base: path.parent().unwrap_or(Path::new(".")),
                    spec: &spec,
                    strategy: &strategy,
                    costs: &costs,
                    budget,
                },
                chunks.parse().map_err(|_| "invalid chunk step")?,
                lease_ms.parse().map_err(|_| "invalid lease duration")?,
                || chrono::Utc::now().timestamp_millis(),
            )?)
        }
        ["corpus-batch", config, database, max_rounds] => {
            let max_rounds: usize = max_rounds
                .parse()
                .map_err(|_| "invalid corpus batch round limit")?;
            if !(1..=10000).contains(&max_rounds) {
                return Err("corpus batch requires1..10000 rounds".into());
            }
            let config_path = Path::new(config);
            let config: CorpusBatchFile = read_operator_config(config)?;
            if let Some(guard) = &config.resource_guard {
                guard.validate()?;
            }
            if config.strategies.is_empty() || config.strategies.len() > 256 {
                return Err("corpus batch requires1..256 strategies".into());
            }
            let specification = config_path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&config.specification);
            let spec = rust_strategy_factory::corpus::load(&specification)?;
            let costs = Costs::default();
            let runs: Vec<_> = config
                .strategies
                .iter()
                .map(|strategy| tick_engine::CorpusRun {
                    base: specification.parent().unwrap_or(Path::new(".")),
                    spec: &spec,
                    strategy,
                    costs: &costs,
                    budget: config.budget,
                })
                .collect();
            let (_signal_runtime, shutdown) = checkpoint_shutdown()?;
            eprintln!("corpus batch ready; signals stop after the current bounded round");
            let mut last = None;
            let mut admission_observation = None;
            let mut disk_admission = None;
            let mut rounds = 0;
            let mut reason = "round-limit";
            let mut preparation_session = tick_engine::CorpusBatchSession::default();
            for _ in 0..max_rounds {
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    reason = "signal";
                    break;
                }
                if let Some(guard) = &config.disk_guard {
                    let decision = guard.check(Path::new(database))?;
                    let allowed = decision.allowed;
                    disk_admission = Some(decision);
                    if !allowed {
                        reason = "disk-pressure";
                        break;
                    }
                }
                if let Some(guard) = &config.resource_guard {
                    let decision = guard.check()?;
                    let allowed = decision.allowed;
                    admission_observation = Some(decision);
                    if !allowed {
                        reason = "resource-pressure";
                        break;
                    }
                }
                let report =
                    preparation_session.round(Path::new(database), &runs, config.limits)?;
                rounds += 1;
                let failed = report.worker_failures > 0 || report.jobs.iter().any(Result::is_err);
                let complete = report
                    .jobs
                    .iter()
                    .all(|r| matches!(r, Ok(tick_engine::CorpusProgress::Completed { .. })));
                let pending = report
                    .jobs
                    .iter()
                    .any(|r| matches!(r, Ok(tick_engine::CorpusProgress::Pending { .. })));
                last = Some(report);
                if failed {
                    reason = "job-error";
                    break;
                }
                if complete {
                    reason = "complete";
                    break;
                }
                if !pending {
                    reason = "held-busy-or-terminal";
                    break;
                }
            }
            preparation_session.release();
            if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                reason = "signal";
            }
            print(
                &serde_json::json!({"rounds":rounds,"stop_reason":reason,"last":last,"admission_observation":admission_observation,"disk_admission":disk_admission,"limitations":"Synthetic USD checkpoint rounds; declared object caps are not measured RSS or OS enforcement; source prefixes are revalidated."}),
            )
        }
        ["corpus-fleet-init", config, database] => {
            let id = rust_strategy_factory::corpus_fleet::register_file(
                Path::new(config),
                Path::new(database),
            )?;
            print(
                &serde_json::json!({"fleet_id":id,"status":rust_strategy_factory::corpus_fleet::status(&Registry::open(Path::new(database))?,&id)?}),
            )
        }
        ["corpus-fleet", database, id, maximum_visits] => {
            let maximum_visits: usize = maximum_visits
                .parse()
                .map_err(|_| "invalid fleet visit bound")?;
            if !(1..=10000).contains(&maximum_visits) {
                return Err("fleet requires1..10000 visits".into());
            }
            let (_signal_runtime, shutdown) = checkpoint_shutdown()?;
            eprintln!("corpus fleet ready; signals stop after the current bounded visit");
            let mut visits = 0;
            let mut last = None;
            let mut reason = "visit-limit".to_owned();
            for _ in 0..maximum_visits {
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    reason = "signal".into();
                    break;
                }
                let progress = rust_strategy_factory::corpus_fleet::step(Path::new(database), id)?;
                visits += usize::from(progress.visit.is_some());
                let advance = progress.kind == "visited";
                if !advance {
                    reason = progress.kind.clone();
                }
                last = Some(progress);
                if !advance {
                    break;
                }
            }
            if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                reason = "signal".into();
            }
            print(
                &serde_json::json!({"visits":visits,"stop_reason":reason,"last":last,"limitations":"Bounded round-robin input visits, not equal CPU allocation or new simulation counts. No process remains running after exit."}),
            )
        }
        [
            "corpus-fleet-supervise",
            database,
            id,
            policy,
            limits,
            maximum,
        ] => {
            let maximum: usize = maximum.parse().map_err(|_| "invalid fleet visit bound")?;
            if !(1..=10000).contains(&maximum) {
                return Err("fleet requires1..10000 visits".into());
            }
            let limits: rust_strategy_factory::owned_process::Limits =
                read_operator_config(limits)?;
            let (_signals, shutdown) = checkpoint_shutdown()?;
            let supervision = rust_strategy_factory::corpus_fleet::Supervision {
                policy: Path::new(policy),
                limits: &limits,
                shutdown: shutdown.clone(),
            };
            let mut visits = 0;
            let mut last = None;
            let mut reason = "visit-limit";
            for _ in 0..maximum {
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    reason = "signal";
                    break;
                }
                let result = rust_strategy_factory::corpus_fleet::supervised_step(
                    Path::new(database),
                    id,
                    &supervision,
                )?;
                visits += usize::from(result.visit.is_some());
                let advance = result.kind == "visited";
                if !advance {
                    reason = if result.kind == "exhausted" {
                        "exhausted"
                    } else if result.kind == "no-ready-sources" {
                        "no-ready-sources"
                    } else {
                        "held-or-pressure"
                    };
                }
                last = Some(result);
                if !advance {
                    break;
                }
            }
            if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                reason = "signal";
            }
            print(
                &serde_json::json!({"visits":visits.to_string(),"stop_reason":reason,"last":last,"limitations":"Explicit foreground owned child visits in deterministic round order; one bounded round per visit, zero retry waiting. Measured CPU ticks do not establish equal CPU allocation or kernel enforcement."}),
            )
        }
        [
            "corpus-fleet-loop",
            database,
            id,
            policy,
            limits,
            maximum,
            wait,
        ] => {
            let maximum: usize = maximum.parse().map_err(|_| "invalid fleet visit bound")?;
            let wait: u64 = wait.parse().map_err(|_| "invalid fleet wait bound")?;
            let limits: rust_strategy_factory::owned_process::Limits =
                read_operator_config(limits)?;
            let (_signals, shutdown) = checkpoint_shutdown()?;
            let supervision = rust_strategy_factory::corpus_fleet::Supervision {
                policy: Path::new(policy),
                limits: &limits,
                shutdown,
            };
            print(&rust_strategy_factory::fleet_loop::run(
                Path::new(database),
                id,
                &supervision,
                maximum,
                wait,
            )?)
        }
        ["corpus-fleet-status", database, id] => {
            print(&rust_strategy_factory::corpus_fleet::status(
                &Registry::open(Path::new(database))?,
                id,
            )?)
        }
        ["corpus-fleet-control", database, id, state] => {
            rust_strategy_factory::corpus_fleet::control(
                &mut Registry::open(Path::new(database))?,
                id,
                state,
            )?;
            print(&serde_json::json!({"ok":true}))
        }
        ["campaign-rotation-init", config, database] => {
            let id = rust_strategy_factory::campaign_rotation::register_file(
                Path::new(config),
                Path::new(database),
            )?;
            print(&rust_strategy_factory::campaign_rotation::status(
                &Registry::open(Path::new(database))?,
                &id,
            )?)
        }
        ["campaign-rotation-status", database, id] => {
            print(&rust_strategy_factory::campaign_rotation::status(
                &Registry::open(Path::new(database))?,
                id,
            )?)
        }
        ["campaign-rotation-control", database, id, state] => {
            let mut registry = Registry::open(Path::new(database))?;
            rust_strategy_factory::campaign_rotation::control(&mut registry, id, state)?;
            print(&rust_strategy_factory::campaign_rotation::status(
                &registry, id,
            )?)
        }
        [
            "campaign-rotation-loop",
            database,
            id,
            policy,
            limits,
            maximum,
            wait,
        ] => {
            let maximum: usize = maximum
                .parse()
                .map_err(|_| "invalid rotation visit bound")?;
            let wait: u64 = wait.parse().map_err(|_| "invalid rotation wait bound")?;
            let limits: rust_strategy_factory::owned_process::Limits =
                read_operator_config(limits)?;
            let (_signals, shutdown) = checkpoint_shutdown()?;
            let supervision = rust_strategy_factory::corpus_campaign::Supervision {
                policy: Path::new(policy),
                limits: &limits,
                shutdown,
            };
            print(&rust_strategy_factory::campaign_rotation::run_wait(
                Path::new(database),
                id,
                &supervision,
                maximum,
                wait,
            )?)
        }
        [
            "campaign-rotation-run",
            database,
            id,
            policy,
            limits,
            maximum,
        ] => {
            let maximum: usize = maximum
                .parse()
                .map_err(|_| "invalid rotation visit bound")?;
            let limits: rust_strategy_factory::owned_process::Limits =
                read_operator_config(limits)?;
            let (_signals, shutdown) = checkpoint_shutdown()?;
            let supervision = rust_strategy_factory::corpus_campaign::Supervision {
                policy: Path::new(policy),
                limits: &limits,
                shutdown,
            };
            print(&rust_strategy_factory::campaign_rotation::run(
                Path::new(database),
                id,
                &supervision,
                maximum,
            )?)
        }
        [
            "corpus-campaign-supervise",
            database,
            id,
            policy,
            limits,
            maximum,
        ] => {
            let maximum: usize = maximum
                .parse()
                .map_err(|_| "invalid campaign owned visit bound")?;
            let limits: rust_strategy_factory::owned_process::Limits =
                read_operator_config(limits)?;
            let (_signals, shutdown) = checkpoint_shutdown()?;
            let supervision = rust_strategy_factory::corpus_campaign::Supervision {
                policy: Path::new(policy),
                limits: &limits,
                shutdown,
            };
            print(&rust_strategy_factory::corpus_campaign::supervise(
                Path::new(database),
                id,
                &supervision,
                maximum,
            )?)
        }
        ["corpus-campaign-run", database, id, maximum] => {
            let maximum: usize = maximum
                .parse()
                .map_err(|_| "invalid campaign round bound")?;
            let (_signals, shutdown) = checkpoint_shutdown()?;
            print(&rust_strategy_factory::corpus_campaign::run(
                Path::new(database),
                id,
                maximum,
                &shutdown,
            )?)
        }
        ["corpus-campaign-control", database, id, state] => {
            let mut registry = Registry::open(Path::new(database))?;
            rust_strategy_factory::corpus_campaign::control(&mut registry, id, state)?;
            print(&rust_strategy_factory::corpus_campaign::status(
                &registry, id,
            )?)
        }
        ["corpus-campaign-freeze", config, database, regions] => {
            let regions: usize = regions
                .parse()
                .map_err(|_| "invalid campaign region count")?;
            let id = rust_strategy_factory::corpus_campaign::register_file(
                Path::new(config),
                Path::new(database),
                regions,
            )?;
            print(&rust_strategy_factory::corpus_campaign::status(
                &Registry::open(Path::new(database))?,
                &id,
            )?)
        }
        ["corpus-campaign-status", database, id] => {
            print(&rust_strategy_factory::corpus_campaign::status(
                &Registry::open(Path::new(database))?,
                id,
            )?)
        }
        ["corpus-campaign-verify", database, id] => {
            print(&rust_strategy_factory::corpus_campaign::verify_inputs(
                &Registry::open(Path::new(database))?,
                id,
            )?)
        }
        ["corpus-search", config, database, max_rounds] => {
            let max_rounds: usize = max_rounds
                .parse()
                .map_err(|_| "invalid corpus search round limit")?;
            if !(1..=10000).contains(&max_rounds) {
                return Err("corpus search requires1..10000 rounds".into());
            }
            let file: CorpusSearchFile = read_operator_config(config)?;
            if let Some(guard) = &file.resource_guard {
                guard.validate()?;
            }
            let specification = Path::new(config)
                .parent()
                .unwrap_or(Path::new("."))
                .join(&file.specification);
            let spec = rust_strategy_factory::corpus::load(&specification)?;
            let costs = Costs::default();
            let (_signal_runtime, shutdown) = checkpoint_shutdown()?;
            eprintln!("corpus search ready; signals stop after the current bounded round");
            let mut rounds = 0;
            let mut last = None;
            let mut admission_observation = None;
            let mut disk_admission = None;
            let mut reason = "round-limit".to_owned();
            for _ in 0..max_rounds {
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    reason = "signal".into();
                    break;
                }
                if let Some(guard) = &file.disk_guard {
                    let decision = guard.check(Path::new(database))?;
                    let allowed = decision.allowed;
                    disk_admission = Some(decision);
                    if !allowed {
                        reason = "disk-pressure".into();
                        break;
                    }
                }
                if let Some(guard) = &file.resource_guard {
                    let decision = guard.check()?;
                    let allowed = decision.allowed;
                    admission_observation = Some(decision);
                    if !allowed {
                        reason = "resource-pressure".into();
                        break;
                    }
                }
                let progress = rust_strategy_factory::corpus_search::step(
                    Path::new(database),
                    rust_strategy_factory::corpus_search::SearchRun {
                        base: specification.parent().unwrap_or(Path::new(".")),
                        specification: &spec,
                        costs: &costs,
                        budget: file.budget,
                        seed: file.seed,
                        start_index: file.start_index,
                        maximum_candidates: file.maximum_candidates,
                        queue_width: file.queue_width,
                    },
                    file.limits,
                )?;
                rounds += 1;
                let advance = progress.kind == "advanced" && progress.status["state"] == "running";
                if !advance {
                    reason = if progress.kind == "advanced" {
                        progress.status["state"]
                            .as_str()
                            .unwrap_or("held")
                            .to_owned()
                    } else {
                        progress.kind.clone()
                    };
                }
                last = Some(progress);
                if !advance {
                    break;
                }
            }
            if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                reason = "signal".into();
            }
            print(
                &serde_json::json!({"rounds":rounds,"stop_reason":reason,"last":last,"admission_observation":admission_observation,"disk_admission":disk_admission,"limitations":"Bounded generated synthetic USD corpus search; generated slots are not new simulations and do not qualify strategies."}),
            )
        }
        [
            "corpus-supervise-region-owned",
            config,
            policy,
            database,
            maximum,
            wait,
            parent_limits,
            start_index,
            expected_source_id,
        ] => {
            let maximum: usize = maximum.parse().map_err(|_| "invalid round limit")?;
            let wait: u64 = wait.parse().map_err(|_| "invalid wait limit")?;
            let start_index: u64 = start_index
                .parse()
                .map_err(|_| "invalid region start index")?;
            let limits: rust_strategy_factory::owned_process::Limits =
                read_operator_config(parent_limits)?;
            let (_signals, shutdown) = checkpoint_shutdown()?;
            print(&rust_strategy_factory::owned_process::run_region(
                rust_strategy_factory::owned_process::RegionInput {
                    config: Path::new(config),
                    start_index,
                    expected_source_id,
                },
                Path::new(policy),
                Path::new(database),
                maximum,
                wait,
                &limits,
                shutdown,
            )?)
        }
        [
            "corpus-supervise-owned",
            config,
            policy,
            database,
            maximum,
            wait,
            parent_limits,
        ] => {
            let maximum: usize = maximum.parse().map_err(|_| "invalid round limit")?;
            let wait: u64 = wait.parse().map_err(|_| "invalid wait limit")?;
            let limits: rust_strategy_factory::owned_process::Limits =
                read_operator_config(parent_limits)?;
            let (_signals, shutdown) = checkpoint_shutdown()?;
            print(&rust_strategy_factory::owned_process::run(
                Path::new(config),
                Path::new(policy),
                Path::new(database),
                maximum,
                wait,
                &limits,
                shutdown,
            )?)
        }
        [
            command,
            config,
            policy,
            database,
            maximum,
            wait_ms,
            pin_args @ ..,
        ] if (*command == "corpus-supervise" && pin_args.is_empty())
            || (*command == "corpus-supervise-pinned" && pin_args.len() == 1) =>
        {
            let maximum: usize = maximum
                .parse()
                .map_err(|_| "invalid supervision round limit")?;
            let wait_ms: u64 = wait_ms
                .parse()
                .map_err(|_| "invalid supervision wait limit")?;
            if !(1..=10000).contains(&maximum) || wait_ms > 3_600_000 {
                return Err(
                    "supervision requires1..10000 rounds and0..3600000 total wait ms".into(),
                );
            }
            let mut loaded =
                rust_strategy_factory::operator_config::load_search(Path::new(config))?;
            let policy: rust_strategy_factory::retry_policy::Policy = read_operator_config(policy)?;
            policy.validate()?;
            if *command == "corpus-supervise-pinned" {
                if pin_args[0].len() > 4096 {
                    return Err("owned input pin exceeds 4096 bytes".into());
                }
                let pin: rust_strategy_factory::owned_process::InputPin =
                    serde_json::from_str(pin_args[0]).map_err(|e| e.to_string())?;
                pin.validate_and_apply(&mut loaded, &policy)?;
            }
            let (_signals, shutdown) = checkpoint_shutdown()?;
            eprintln!("corpus supervision ready; signals stop at a safe round/wait boundary");
            let mut rounds = 0usize;
            let mut checks = 0usize;
            let mut waited = std::time::Duration::ZERO;
            let budget = std::time::Duration::from_millis(wait_ms);
            let mut last = None;
            let mut reason = "round-limit";
            let mut session: Option<(Registry, rust_strategy_factory::supervision_session::Owner)> =
                None;
            let mut heartbeat = std::time::Instant::now();
            let mut resource = None;
            let mut disk = None;
            'session: while rounds < maximum && checks < maximum * 2 + 1 {
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    reason = "signal";
                    break;
                }
                if let Some(guard) = &loaded.file.disk_guard {
                    let decision = guard.check(Path::new(database))?;
                    let allowed = decision.allowed;
                    disk = Some(decision);
                    if !allowed {
                        reason = "disk-pressure";
                        break;
                    }
                }
                if let Some(guard) = &loaded.file.resource_guard {
                    let decision = guard.check()?;
                    let allowed = decision.allowed;
                    resource = Some(decision);
                    if !allowed {
                        reason = "resource-pressure";
                        break;
                    }
                }
                if session.is_none() {
                    let id = rust_strategy_factory::corpus_search::prepare_supervision(
                        Path::new(database),
                        &loaded.run(),
                        &policy,
                    )?;
                    let mut registry = Registry::open(Path::new(database))?;
                    let owner = rust_strategy_factory::supervision_session::start(
                        &mut registry,
                        &id,
                        std::process::id(),
                        maximum,
                        wait_ms,
                        chrono::Utc::now().timestamp_millis(),
                    )?;
                    let _ = writeln!(
                        std::io::stderr(),
                        "{}",
                        serde_json::json!({"supervision_session_id":owner.id})
                    );
                    session = Some((registry, owner));
                }
                if let Some((registry, owner)) = &mut session {
                    if rust_strategy_factory::supervision_session::stop_requested(registry, owner)?
                    {
                        reason = "session-stop-request";
                        break;
                    }
                    rust_strategy_factory::supervision_session::heartbeat(
                        registry,
                        owner,
                        rounds,
                        checks,
                        u64::try_from(waited.as_millis())
                            .map_err(|_| "wait observation overflow")?,
                        chrono::Utc::now().timestamp_millis(),
                    )?;
                    heartbeat = std::time::Instant::now();
                }
                let result = rust_strategy_factory::corpus_search::supervised_step(
                    Path::new(database),
                    loaded.run(),
                    loaded.file.limits,
                    &policy,
                )?;
                checks += 1;
                if let Some(progress) = &result.progress {
                    rounds += 1;
                    let advance = progress.kind == "advanced"
                        && (progress.status["state"] == "running"
                            || progress.status["state"] == "paused"
                                && progress.status["control"]["pause_origin"] == "worker_error");
                    if !advance {
                        reason = if progress.status["state"] == "exhausted" {
                            "exhausted"
                        } else {
                            "held-or-busy"
                        };
                        last = Some(result);
                        break;
                    }
                    last = Some(result);
                    continue;
                }
                let backoff = result
                    .decision
                    .as_ref()
                    .is_some_and(|d| d["kind"] == "backoff");
                if !backoff {
                    reason = if result.status["state"] == "exhausted" {
                        "exhausted"
                    } else {
                        "retry-held"
                    };
                    last = Some(result);
                    break;
                }
                let decision = result.decision.as_ref().ok_or("backoff decision missing")?;
                let deadline: i64 = decision["retry_at_ms"]
                    .as_str()
                    .ok_or("retry deadline missing")?
                    .parse()
                    .map_err(|_| "invalid retry deadline")?;
                let id = result.status["search_id"]
                    .as_str()
                    .ok_or("search identity missing")?
                    .to_owned();
                let epoch = result.status["control"]["epoch"].clone();
                last = Some(result);
                if budget.is_zero() {
                    reason = "retry-backoff";
                    break;
                }
                let watcher = Registry::open(Path::new(database))?;
                let start = std::time::Instant::now();
                while chrono::Utc::now().timestamp_millis() < deadline {
                    if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                        waited += start.elapsed();
                        reason = "signal";
                        break 'session;
                    }
                    if let Some((registry, owner)) = &mut session {
                        if rust_strategy_factory::supervision_session::stop_requested(
                            registry, owner,
                        )? {
                            waited += start.elapsed();
                            reason = "session-stop-request";
                            break 'session;
                        }
                        if heartbeat.elapsed() >= std::time::Duration::from_secs(1) {
                            rust_strategy_factory::supervision_session::heartbeat(
                                registry,
                                owner,
                                rounds,
                                checks,
                                u64::try_from((waited + start.elapsed()).as_millis())
                                    .map_err(|_| "wait observation overflow")?,
                                chrono::Utc::now().timestamp_millis(),
                            )?;
                            heartbeat = std::time::Instant::now();
                        }
                    }
                    let elapsed = waited + start.elapsed();
                    if elapsed >= budget {
                        waited = elapsed;
                        reason = "wait-budget";
                        break 'session;
                    }
                    let state =
                        rust_strategy_factory::corpus_search::control_status(&watcher, &id)?;
                    if state["epoch"] != epoch
                        || state["state"] != "paused"
                        || state["pause_origin"] != "worker_error"
                    {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50).min(budget - elapsed));
                }
                waited += start.elapsed();
            }
            if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                reason = "signal";
            }
            if checks > maximum * 2
                && rounds < maximum
                && !shutdown.load(std::sync::atomic::Ordering::Acquire)
            {
                reason = "supervision-check-limit";
            }
            let session_status = if let Some((registry, owner)) = &mut session {
                rust_strategy_factory::supervision_session::finish(
                    registry,
                    owner,
                    rounds,
                    checks,
                    u64::try_from(waited.as_millis()).map_err(|_| "wait observation overflow")?,
                    chrono::Utc::now().timestamp_millis(),
                    reason,
                )?;
                Some(rust_strategy_factory::supervision_session::status(
                    registry, &owner.id,
                )?)
            } else {
                None
            };
            print(
                &serde_json::json!({"session_status":session_status,"session_id":session.as_ref().map(|(_,owner)|&owner.id),"round_attempts":rounds.to_string(),"checks":checks.to_string(),"waited_ms":waited.as_millis().to_string(),"stop_reason":reason,"last":last,"resource_admission":resource,"disk_admission":disk,"limitations":"Explicit foreground session only. Round attempts and retry observations are not new simulations. No background worker or hard filesystem/OS deadline is implied."}),
            )
        }
        ["recovery-list", database] => {
            print(&rust_strategy_factory::recovery::list(Path::new(database))?)
        }
        ["recovery-backup", database, id] => print(&rust_strategy_factory::recovery::backup(
            Path::new(database),
            id,
        )?),
        ["recovery-verify", database, id] => print(&rust_strategy_factory::recovery::verify(
            Path::new(database),
            id,
        )?),
        ["recovery-restore", database, source, id] => print(
            &rust_strategy_factory::recovery::restore(Path::new(database), source, id)?,
        ),
        ["recovery-path", database, id] => print(
            &serde_json::json!({"path":rust_strategy_factory::recovery::path(Path::new(database),id)?.to_str().ok_or("non-UTF8 recovery path")?}),
        ),
        [
            kind @ ("corpus-owned-page" | "corpus-supervision-page"),
            database,
            after,
            limit,
        ] => {
            let limit: usize = limit.parse().map_err(|_| "invalid process page limit")?;
            if !(1..=128).contains(&limit) {
                return Err("process page limit must be1..128".into());
            }
            let registry = Registry::open(Path::new(database))?;
            let after = if *after == "--" { None } else { Some(*after) };
            if *kind == "corpus-owned-page" {
                print(&rust_strategy_factory::owned_history::page(
                    &registry, after, limit,
                )?)
            } else {
                print(&rust_strategy_factory::supervision_session::page(
                    &registry, after, limit,
                )?)
            }
        }
        ["corpus-owned-invocations", database] => print(
            &rust_strategy_factory::owned_history::list(&Registry::open(Path::new(database))?)?,
        ),
        ["corpus-owned-status", database, id] => {
            print(&rust_strategy_factory::owned_history::status(
                &Registry::open(Path::new(database))?,
                id,
            )?)
        }
        ["corpus-supervision-sessions", database] => {
            print(&rust_strategy_factory::supervision_session::list(
                &Registry::open(Path::new(database))?,
            )?)
        }
        ["corpus-supervision-status", database, id] => {
            print(&rust_strategy_factory::supervision_session::status(
                &Registry::open(Path::new(database))?,
                id,
            )?)
        }
        ["corpus-supervision-stop", database, id] => print(
            &serde_json::json!({"requested":rust_strategy_factory::supervision_session::request_stop(&mut Registry::open(Path::new(database))?,id,chrono::Utc::now().timestamp_millis())?}),
        ),
        ["corpus-retry-decisions", database, id, after, limit] => {
            print(&rust_strategy_factory::retry_policy::decisions(
                &Registry::open(Path::new(database))?,
                id,
                if *after == "-" { None } else { Some(after) },
                limit.parse().map_err(|_| "invalid retry decision limit")?,
            )?)
        }
        ["corpus-retry-configure", database, id, policy] => {
            let policy: rust_strategy_factory::retry_policy::Policy = read_operator_config(policy)?;
            let hash = rust_strategy_factory::retry_policy::configure(
                &mut Registry::open(Path::new(database))?,
                id,
                &policy,
            )?;
            print(
                &serde_json::json!({"search_id":id,"policy_hash":hash,"policy":{"backoff_ms":policy.backoff_ms.to_string(),"maximum_slots":policy.maximum_slots.to_string()},"workers_started":0}),
            )
        }
        ["corpus-retry-admit", database, id, epoch] => {
            let epoch: i64 = epoch.parse().map_err(|_| "invalid retry epoch")?;
            if epoch < 0 {
                return Err("retry epoch must be nonnegative".into());
            }
            print(&rust_strategy_factory::retry_policy::admit(
                &mut Registry::open(Path::new(database))?,
                id,
                epoch,
                chrono::Utc::now().timestamp_millis(),
            )?)
        }
        ["corpus-search-requeue", database, id, maximum_slots] => {
            print(&rust_strategy_factory::corpus_search::requeue_errors(
                &mut Registry::open(Path::new(database))?,
                id,
                maximum_slots
                    .parse()
                    .map_err(|_| "invalid requeue slot limit")?,
            )?)
        }
        ["corpus-search-status", database, id] => {
            print(&rust_strategy_factory::corpus_search::status(
                &Registry::open(Path::new(database))?,
                id,
            )?)
        }
        ["corpus-search-control", database, id, state] => {
            rust_strategy_factory::corpus_search::control(
                &mut Registry::open(Path::new(database))?,
                id,
                state,
            )?;
            print(&serde_json::json!({"ok":true}))
        }
        ["disk-status", database] => {
            print(&rust_strategy_factory::disk::observe(Path::new(database))?)
        }
        ["resource-status"] => print(&rust_strategy_factory::resource::observe()?),
        ["corpus-control", database, id, state] => {
            Registry::open(Path::new(database))?.set_corpus_control(
                id,
                state,
                chrono::Utc::now().timestamp_millis(),
            )?;
            print(&serde_json::json!({"ok":true}))
        }
        ["corpus-status", database, id] => {
            print(&Registry::open(Path::new(database))?.corpus_status(id)?)
        }
        ["corpus-source-facts", specification] => {
            let path = Path::new(specification);
            let spec = rust_strategy_factory::corpus::load(path)?;
            print(&rust_strategy_factory::corpus::source_facts(
                path.parent().unwrap_or(Path::new(".")),
                &spec,
            )?)
        }
        ["corpus-inspect", specification] => {
            let path = Path::new(specification);
            print(&rust_strategy_factory::corpus::inspect(
                path.parent().unwrap_or(Path::new(".")),
                &rust_strategy_factory::corpus::load(path)?,
            )?)
        }
        ["coverage", path, start, end, threshold] => {
            let interval = rust_strategy_factory::validation::Interval {
                start_ms: start.parse().map_err(|_| "invalid start milliseconds")?,
                end_exclusive_ms: end.parse().map_err(|_| "invalid end milliseconds")?,
            };
            print(&rust_strategy_factory::coverage::report(
                &read_dataset(path)?,
                interval,
                threshold
                    .parse()
                    .map_err(|_| "invalid gap threshold milliseconds")?,
                1000,
            )?)
        }
        ["inspect", path] => {
            let d: Dataset = read_dataset(path)?;
            d.verify()?;
            print(&d.manifest)
        }
        ["candles", path, tf] => {
            let d: Dataset = read_dataset(path)?;
            let tf: Timeframe = serde_json::from_value(serde_json::Value::String(tf.to_string()))
                .map_err(|e| e.to_string())?;
            print(&candles::build(&d, tf)?)
        }
        ["registry-artifacts", path] => print(&rust_strategy_factory::result_artifact::inventory(
            &Registry::open(Path::new(path))?,
        )?),
        ["registry-check", path] => {
            if !Path::new(path).is_file() {
                return Err("registry does not exist".into());
            }
            let registry = Registry::open(Path::new(path))?;
            registry.integrity()?;
            print(&registry.counts()?)
        }
        ["backup", path, destination] => {
            if !Path::new(path).is_file() {
                return Err("registry does not exist".into());
            }
            let registry = Registry::open(Path::new(path))?;
            registry.backup(Path::new(destination))?;
            println!("backup integrity verified");
            Ok(())
        }
        ["experiment", path, strategy_path, database] => {
            let d: Dataset = read_dataset(path)?;
            if d.profile.quote != "USD" {
                return Err(
                    "non-USD evaluation requires explicit event-aligned conversions".into(),
                );
            }
            let conversions = vec![Conversion::usd(); d.ticks.len()];
            let s: Strategy = read_json(strategy_path)?;
            let mut registry = Registry::open(Path::new(database))?;
            print(&research::evaluate(
                &mut registry,
                &research::Input {
                    dataset: d,
                    conversions,
                },
                &s,
                &Costs::default(),
                &research::compute_revision()?,
                4_000_000,
            )?)
        }
        ["improvement-jobs", database, id] => {
            if !Path::new(database).is_file() {
                return Err("registry does not exist".into());
            }
            let registry = Registry::open(Path::new(database))?;
            print(&research::improvement_jobs(&registry, id)?)
        }
        ["research-status", database, id] => {
            if !Path::new(database).is_file() {
                return Err("registry does not exist".into());
            }
            print(&research::status(
                &Registry::open(Path::new(database))?,
                id,
                chrono::Utc::now().timestamp_millis(),
            )?)
        }
        ["research-control", database, id, state] => {
            if !Path::new(database).is_file() {
                return Err("registry does not exist".into());
            }
            research::control(&Registry::open(Path::new(database))?, id, state)
        }
        ["batch-experiments", config, candidates, database] => {
            let (mut inputs, limits) = research_inputs(config)?;
            if inputs.len() != 1 || limits.improvement_children != 0 {
                return Err(
                    "batch-experiments requires one input and improvement_children=0".into(),
                );
            }
            let input = inputs.pop().ok_or("batch input missing")?;
            if input.dataset.ticks.len() > limits.max_total_events
                || input.dataset.manifest.class != data::DataClass::SyntheticTestOnly
                    && limits.training_window.is_none()
            {
                return Err(
                    "batch exceeds event budget or lacks explicit real-data training scope".into(),
                );
            }
            let candidates: Vec<Strategy> = read_json(candidates)?;
            let evaluation = limits
                .training_window
                .as_ref()
                .map(|window| -> Result<research::Evaluation> {
                    Ok(research::Evaluation {
                        window: window.clone(),
                        stage: rust_strategy_factory::storage::Stage::TickVerification,
                        protocol_hash: data::hash(&("training-window-v1", window))?,
                    })
                })
                .transpose()?;
            let report = research::evaluate_batch_report(
                &mut Registry::open(Path::new(database))?,
                &input,
                &candidates,
                &Costs::default(),
                &research::compute_revision()?,
                research::BatchOptions {
                    evaluation: research::EvaluationOptions {
                        feature_cells: limits.feature_cells,
                        evaluation: evaluation.as_ref(),
                        shadow_policy: limits.shadow_policy.as_ref(),
                        proof_policy: limits.proof_policy.as_ref(),
                        indexed_policy: limits.indexed_policy.as_ref(),
                    },
                    state_budget_bytes: limits.state_budget_bytes,
                },
            )?;
            print(
                &serde_json::json!({"new_attempts":report.executions.iter().filter(|e|!e.reused).count(),"reused":report.executions.iter().filter(|e|e.reused).count(),"executions":report.executions,
                "shared_index_initialized":report.shared_index_initialized,"shared_feature_builds":report.shared_feature_builds,"shared_cache_hits":report.shared_cache_hits,"primary_candles":report.primary_candles,"visible_candles":report.visible_candles}),
            )
        }
        ["research", config, database] | ["research", config, database, _] => {
            let max_steps = args
                .get(3)
                .map(|s| s.parse::<u64>().map_err(|_| "invalid step budget"))
                .transpose()?
                .unwrap_or(u64::MAX);
            let (_signal_runtime, shutdown) = checkpoint_shutdown()?;
            let mut driver = research_driver(config, database)?;
            driver.acquire(chrono::Utc::now().timestamp_millis())?;
            println!(
                "{}",
                serde_json::json!({"run_id":driver.id,"worker_count":1,"queue_capacity":1,"state":"coordinator_started"})
            );
            let mut completed = 0;
            while completed < max_steps && !shutdown.load(std::sync::atomic::Ordering::Acquire) {
                if driver.step(chrono::Utc::now().timestamp_millis())? {
                    completed += 1;
                } else {
                    let s = research::status(
                        &driver.registry,
                        &driver.id,
                        chrono::Utc::now().timestamp_millis(),
                    )?;
                    if s.state == "stopped" {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
            driver.release()?;
            print(&research::status(
                &driver.registry,
                &driver.id,
                chrono::Utc::now().timestamp_millis(),
            )?)
        }
        ["evaluate", path, strategy_path, output] => {
            let d: Dataset = read_dataset(path)?;
            if d.profile.quote != "USD" {
                return Err("non-USD evaluation requires explicit event-aligned conversion series through the Rust API; no inferred rates".into());
            }
            let s: Strategy = read_json(strategy_path)?;
            let c = candles::build(&d, s.timeframe)?;
            let mut f = Features::with_context(&c, 4_000_000, &d, s.timeframe)?;
            let mask = f.signal(&s.rule)?;
            let result = tick_engine::replay(
                &d,
                &c,
                &mask,
                &s,
                &Costs::default(),
                &vec![Conversion::usd(); d.ticks.len()],
            )?;
            save(output, &result)?;
            print(&result)
        }
        [] | ["--help"] | ["help"] => {
            println!(
                "Rust Strategy Factory — historical research only; SOFTWARE_INCOMPLETE\n\nvalidate-protocol <input-config.json> <plan.json> <registry.sqlite>\nreview-training <input-config.json> <registry.sqlite> <diagnosis-id>\ntraining-review-report <registry.sqlite> <review-id>\ndiagnose <registry.sqlite> <training-experiment-id>\ndiagnosis-report <registry.sqlite> <diagnosis-id>\nprotocol-report <registry.sqlite> <protocol-id>\nauth-init <new-auth.json> (password from stdin)\nserve <registry.sqlite> <auth.json> <port> [catalog.sqlite [review-config.json]]\nprofiles\nfixture <symbol> <new-dataset.json>\ngenerate\nimport <normalized.csv> <profile.json> <source-url-or-description> <new-dataset.json> <real|synthetic>\nimport-step <normalized.csv> <profile.json> <source> <import-directory> <real|synthetic> <chunk-events>\nimport-status <import-directory>\nimport-export <complete-import-directory> <new-dataset.rsf>\npack-blocks <dataset.json|dataset.rsf> <new-dataset.rsfb> <block-rows>\nblock-inspect <dataset.rsfb>\nblock-range <dataset.rsfb> <pinned-root> <start-row> <count>\nunpack-blocks <dataset.rsfb> <new-dataset.json>\npack-native <dataset.json|dataset.rsf|dataset.rsfz> <new-dataset.rsf>\npack-parquet <dataset.json|dataset.rsf|dataset.rsfz> <new-dataset.parquet>\nunpack-parquet <dataset.parquet> <new-dataset.json>\ncompress-native <dataset.rsf> <new-dataset.rsfz>\ndecompress-native <dataset.rsfz> <new-dataset.rsf>\nunpack-native <dataset.rsf> <new-dataset.json>\ninspect <dataset.json|dataset.rsf>\ncatalog-record <dataset> <catalog.sqlite>\ncatalog-entry <catalog.sqlite> <dataset-identity>\narchive-evidence <normalized.csv> <dataset> <catalog.sqlite> <identity>\ncalendar-evidence <dataset> <catalog.sqlite> <identity> <declaration.json>\nmachine-admit <archive.csv> <dataset> <catalog.sqlite> <identity> <calendar.json> <claim.json>\nmachine-report <catalog.sqlite> <admission-id>\nmachine-revalidate <catalog.sqlite> <admission-id> <archive.csv> <dataset>\nreview-authority-init <reviewers.json> <new-authority.secret>\ndata-review-decide <catalog.sqlite> <authority.secret> <reviewer> <request.json> (password from stdin)\ndata-review-state <catalog.sqlite> <authority.secret> <identity>\ndata-eligibility <catalog.sqlite> <authority.secret> <machine-id> <archive.csv> <dataset>\ncatalog-review <catalog.sqlite> <request.json>\ncatalog-review-report <catalog.sqlite> <review-hash>\ncatalog-reviews <catalog.sqlite> <dataset-identity> <after-hash-or--> <limit>\ncatalog-correction <dataset> <catalog.sqlite> <parent-identity> <evidence-reason>\ncatalog-list <catalog.sqlite> [after-identity-or-- limit]\ncatalog-backup <catalog.sqlite> <new-snapshot.sqlite>\ncatalog-restore <snapshot.sqlite> <new-catalog.sqlite>\ncorpus-create-bounded <new-corpus.json> <maximum-artifact-events> <chunk.rsf|chunk.rsfb>...\ncorpus-create <new-corpus.json> <chunk.rsf>...\nresource-status\ndisk-status <registry.sqlite-or-new-path>\ncorpus-fleet-init <fleet.json> <registry.sqlite>\ncorpus-fleet <registry.sqlite> <fleet-id> <maximum-visits>\ncorpus-fleet-supervise <registry.sqlite> <fleet-id> <policy.json> <parent-limits.json> <maximum-visits>\ncorpus-fleet-loop <registry.sqlite> <fleet-id> <policy.json> <parent-limits.json> <maximum-visits> <total-wait-ms>\ncorpus-fleet-status <registry.sqlite> <fleet-id>\ncorpus-fleet-control <registry.sqlite> <fleet-id> <running|paused|stopped>\ncampaign-rotation-init <config.json> <registry.sqlite>\ncampaign-rotation-loop <registry.sqlite> <rotation-id> <policy.json> <parent-limits.json> <max-visits> <total-wait-ms>\ncampaign-rotation-run <registry.sqlite> <rotation-id> <policy.json> <parent-limits.json> <max-visits>\ncampaign-rotation-status <registry.sqlite> <rotation-id>\ncampaign-rotation-control <registry.sqlite> <rotation-id> <running|paused|stopped>\ncorpus-campaign-supervise <registry.sqlite> <campaign-id> <policy.json> <parent-limits.json> <max-visits>\ncorpus-campaign-run <registry.sqlite> <campaign-id> <max-rounds>\ncorpus-campaign-control <registry.sqlite> <campaign-id> <running|paused|stopped>\ncorpus-campaign-freeze <search-config.json> <registry.sqlite> <regions>\ncorpus-campaign-status <registry.sqlite> <campaign-id>\ncorpus-campaign-verify <registry.sqlite> <campaign-id>\ncorpus-search <config.json> <registry.sqlite> <max-rounds>\nrecovery-list <registry.sqlite>\nrecovery-backup <registry.sqlite> <new-label>\nrecovery-verify <registry.sqlite> <label>\nrecovery-restore <registry.sqlite> <source-label> <new-label>\nrecovery-path <registry.sqlite> <label>\ncorpus-owned-page <registry.sqlite> <after-id-or--> <limit>\ncorpus-supervision-page <registry.sqlite> <after-id-or--> <limit>\ncorpus-owned-invocations <registry.sqlite>\ncorpus-owned-status <registry.sqlite> <invocation-id>\ncorpus-supervision-sessions <registry.sqlite>\ncorpus-supervision-status <registry.sqlite> <session-id>\ncorpus-supervision-stop <registry.sqlite> <session-id>\ncorpus-supervise-region-owned <config.json> <policy.json> <registry.sqlite> <max-rounds> <total-wait-ms> <parent-limits.json> <start-index> <expected-search-id>\ncorpus-supervise-owned <config.json> <policy.json> <registry.sqlite> <max-rounds> <total-wait-ms> <parent-limits.json>\ncorpus-supervise <config.json> <policy.json> <registry.sqlite> <max-rounds> <total-wait-ms>\ncorpus-retry-decisions <registry.sqlite> <search-id> <after-or--> <limit>\ncorpus-retry-configure <registry.sqlite> <search-id> <policy.json>\ncorpus-retry-admit <registry.sqlite> <search-id> <expected-epoch>\ncorpus-search-requeue <registry.sqlite> <search-id> <maximum-slots>\ncorpus-search-status <registry.sqlite> <search-id>\ncorpus-search-control <registry.sqlite> <search-id> <running|paused|stopped>\ncorpus-batch <config.json> <registry.sqlite> <max-rounds>\ncorpus-control <registry.sqlite> <experiment-id> <running|paused|stopped>\ncorpus-status <registry.sqlite> <experiment-id>\ncorpus-step <spec.json> <strategy.json> <registry.sqlite> <max-candles> <feature-cells> <max-receipts> <new-chunks> <lease-ms>\ncorpus-source-facts <ordered-corpus.json>\ncorpus-inspect <ordered-corpus.json>\ncoverage <dataset> <start-ms> <end-exclusive-ms> <gap-threshold-ms>\ncandles <dataset.json|dataset.rsf> <M1|M5|M15|M30|H1|H4|D1>\nevaluate <dataset.json|dataset.rsf> <strategy.json> <new-result.json>\nexperiment <dataset.json|dataset.rsf> <strategy.json> <registry.sqlite>\nregistry-check <registry.sqlite>\nregistry-artifacts <registry.sqlite>\nbackup <registry.sqlite> <new-backup.sqlite>\nresearch <config.json> <registry.sqlite> [max-steps]\nbatch-experiments <one-input-config.json> <same-timeframe-strategies.json> <registry.sqlite>\nimprovement-jobs <registry.sqlite> <run-id>\nresearch-status <registry.sqlite> <run-id>\nresearch-control <registry.sqlite> <run-id> <running|paused|stopped>\n\nSynthetic profiles do not establish broker feasibility. No strategy qualification is implemented."
            );
            Ok(())
        }
        _ => Err("unknown command or argument count; use --help".into()),
    }
}
fn main() {
    if let Err(e) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
