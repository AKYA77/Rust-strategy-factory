use rust_strategy_factory::storage::{Claim, ExperimentKey, Outcome, Registry, Stage};
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
fn run(args: &[String]) -> Result<()> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
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
        ["inspect", path] => {
            let d: Dataset = read_json(path)?;
            d.verify()?;
            print(&d.manifest)
        }
        ["candles", path, tf] => {
            let d: Dataset = read_json(path)?;
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
            let d: Dataset = read_json(path)?;
            d.verify()?;
            if d.profile.quote != "USD" {
                return Err(
                    "CLI requires USD quote; use explicit event conversions through library".into(),
                );
            }
            let s: Strategy = read_json(strategy_path)?;
            let costs = Costs::default();
            let key = ExperimentKey {
                concept_hash: s.identity()?,
                instrument: d.profile.symbol.clone(),
                dataset_hash: d.manifest.content_hash.clone(),
                start_ms: d.manifest.first_ms,
                end_exclusive_ms: d.manifest.last_ms.checked_add(1).ok_or("range overflow")?,
                signal_version: "closed-publication-next-event-v1".into(),
                indicator_version: "integer-sma-v1".into(),
                cost_hash: data::hash(&costs)?,
                fill_policy: "full-l1-assumption-target-at-limit-preflight-v2".into(),
                profile_hash: d.manifest.profile_hash.clone(),
                engine_version: "ordered-l1-v2-marked-margin".into(),
                protocol_hash: data::hash(&"training-only-conservative-signals-v1")?,
                code_revision: format!(
                    "binary-blake3:{}",
                    data::file_hash(&std::env::current_exe().map_err(|e| e.to_string())?)?
                ),
                configuration_hash: data::hash(&(4_000_000_usize, &costs))?,
                stage: Stage::TickVerification,
            };
            let mut registry = Registry::open(Path::new(database))?;
            let id = registry.register(&key)?;
            let now = chrono::Utc::now().timestamp_millis();
            match registry.claim(&id, now, 300_000)? {
                Claim::Busy => Err("experiment already leased to an active worker".into()),
                Claim::Reused(result) => {
                    print(&serde_json::json!({"experiment_id":id,"reused":true,"result":result}))
                }
                Claim::Acquired(lease) => {
                    let run = || -> Result<serde_json::Value> {
                        let c = candles::build(&d, s.timeframe)?;
                        let mut features = Features::new(&c, 4_000_000)?;
                        let screen = strategies::screen(&mut features, &s)?;
                        if !screen.requires_tick_verification {
                            return Ok(serde_json::json!({"screen":screen,"tick_replay":null}));
                        }
                        let mask = features.signal(&s.rule)?;
                        let replay = tick_engine::replay(
                            &d,
                            &c,
                            &mask,
                            &s,
                            &costs,
                            &vec![Conversion::usd(); d.ticks.len()],
                        )?;
                        Ok(serde_json::json!({"screen":screen,"tick_replay":replay}))
                    };
                    match run() {
                        Ok(result) => {
                            let outcome = if result["tick_replay"].is_null() {
                                Outcome::Rejected
                            } else {
                                Outcome::Completed
                            };
                            registry.finish(
                                &lease,
                                chrono::Utc::now().timestamp_millis(),
                                outcome,
                                &result,
                            )?;
                            print(
                                &serde_json::json!({"experiment_id":id,"reused":false,"result":result}),
                            )
                        }
                        Err(error) => {
                            registry.finish(
                                &lease,
                                chrono::Utc::now().timestamp_millis(),
                                Outcome::Failed,
                                &serde_json::json!({"error":error}),
                            )?;
                            Err(error)
                        }
                    }
                }
            }
        }
        ["evaluate", path, strategy_path, output] => {
            let d: Dataset = read_json(path)?;
            if d.profile.quote != "USD" {
                return Err("non-USD evaluation requires explicit event-aligned conversion series through the Rust API; no inferred rates".into());
            }
            let s: Strategy = read_json(strategy_path)?;
            let c = candles::build(&d, s.timeframe)?;
            let mut f = Features::new(&c, 4_000_000)?;
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
                "Rust Strategy Factory — historical research only; SOFTWARE_INCOMPLETE\n\nprofiles\ngenerate\nimport <normalized.csv> <profile.json> <source-url-or-description> <new-dataset.json> <real|synthetic>\ninspect <dataset.json>\ncandles <dataset.json> <M1|M5|M15|M30|H1|H4|D1>\nevaluate <dataset.json> <strategy.json> <new-result.json>\nexperiment <dataset.json> <strategy.json> <registry.sqlite>\nregistry-check <registry.sqlite>\nbackup <registry.sqlite> <new-backup.sqlite>\n\nSynthetic profiles do not establish broker feasibility. No strategy qualification is implemented."
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
