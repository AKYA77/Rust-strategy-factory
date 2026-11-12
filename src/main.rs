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
    // create_new forbids overwriting valid existing archives or evidence.
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
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
                "Rust Strategy Factory — historical research only; SOFTWARE_INCOMPLETE\n\nprofiles\ngenerate\nimport <normalized.csv> <profile.json> <source-url-or-description> <new-dataset.json> <real|synthetic>\ninspect <dataset.json>\ncandles <dataset.json> <M1|M5|M15|M30|H1|H4|D1>\nevaluate <dataset.json> <strategy.json> <new-result.json>\n\nSynthetic profiles do not establish broker feasibility. No strategy qualification is implemented."
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
