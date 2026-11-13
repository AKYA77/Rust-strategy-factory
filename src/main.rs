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
fn read_dataset(path: &str) -> Result<Dataset> {
    if Path::new(path).extension().is_some_and(|e| e == "rsf") {
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
        ["serve", database, auth, port] => {
            let port = port.parse::<u16>().map_err(|_| "invalid loopback port")?;
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?
                .block_on(rust_strategy_factory::dashboard::serve(
                    Path::new(database),
                    Path::new(auth),
                    port,
                ))
        }
        ["fixture", symbol, output] => {
            let dataset = data::synthetic_fixture(symbol)?;
            save(output, &dataset)?;
            print(&dataset.manifest)
        }
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
                "Rust Strategy Factory — historical research only; SOFTWARE_INCOMPLETE\n\nvalidate-protocol <input-config.json> <plan.json> <registry.sqlite>\nreview-training <input-config.json> <registry.sqlite> <diagnosis-id>\ntraining-review-report <registry.sqlite> <review-id>\ndiagnose <registry.sqlite> <training-experiment-id>\ndiagnosis-report <registry.sqlite> <diagnosis-id>\nprotocol-report <registry.sqlite> <protocol-id>\nauth-init <new-auth.json> (password from stdin)\nserve <registry.sqlite> <auth.json> <port>\nprofiles\nfixture <symbol> <new-dataset.json>\ngenerate\nimport <normalized.csv> <profile.json> <source-url-or-description> <new-dataset.json> <real|synthetic>\npack-native <dataset.json|dataset.rsf> <new-dataset.rsf>\nunpack-native <dataset.rsf> <new-dataset.json>\ninspect <dataset.json|dataset.rsf>\ncandles <dataset.json|dataset.rsf> <M1|M5|M15|M30|H1|H4|D1>\nevaluate <dataset.json|dataset.rsf> <strategy.json> <new-result.json>\nexperiment <dataset.json|dataset.rsf> <strategy.json> <registry.sqlite>\nregistry-check <registry.sqlite>\nbackup <registry.sqlite> <new-backup.sqlite>\nresearch <config.json> <registry.sqlite> [max-steps]\nimprovement-jobs <registry.sqlite> <run-id>\nresearch-status <registry.sqlite> <run-id>\nresearch-control <registry.sqlite> <run-id> <running|paused|stopped>\n\nSynthetic profiles do not establish broker feasibility. No strategy qualification is implemented."
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
