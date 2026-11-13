use rust_strategy_factory::{
    contracts::Profile,
    data::{self, DataClass},
    import_job, native,
};
use std::{fs, path::PathBuf};
struct Fixture {
    root: PathBuf,
    input: PathBuf,
    job: PathBuf,
}
impl Fixture {
    fn new(label: &str, rows: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("factory-import-{label}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let input = root.join("source.csv");
        let job = root.join("job");
        fs::write(&input, format!("timestamp_ms,sequence,bid,ask\n{rows}")).unwrap();
        Self { root, input, job }
    }
    fn step(&self, n: usize) -> Result<import_job::Progress, String> {
        import_job::step(
            &self.input,
            &self.job,
            Profile::synthetic("EURUSD").unwrap(),
            "SYNTHETIC_TEST_ONLY import fixture",
            true,
            n,
        )
    }
    fn checkpoint(&self) -> (String, String) {
        let c = rusqlite::Connection::open(self.job.join("normalized-import.sqlite")).unwrap();
        c.query_row("SELECT body,hash FROM import_state", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
    }
    fn restore(&self, state: &(String, String)) {
        let c = rusqlite::Connection::open(self.job.join("normalized-import.sqlite")).unwrap();
        c.execute(
            "UPDATE import_state SET body=?1,hash=?2",
            rusqlite::params![state.0, state.1],
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
const ROWS: &str = "0,18446744073709551611,1.00000,1.00010\n60000,18446744073709551612,1.00020,1.00030\n60000,18446744073709551613,1.00040,1.00050\n120000,18446744073709551614,1.00060,1.00070\n121000,18446744073709551615,1.00080,1.00090\n";
#[test]
fn checkpoint_resume_preserves_complete_source_order_and_category_without_republication() {
    let f = Fixture::new("resume", ROWS);
    let source = fs::read(&f.input).unwrap();
    let p = f.step(2).unwrap();
    assert_eq!(p.events, 2);
    assert!(!p.complete);
    let first = fs::read(f.job.join(&p.chunks[0].file)).unwrap();
    let p = f.step(2).unwrap();
    assert_eq!(p.events, 4);
    assert!(!p.complete);
    let p = f.step(2).unwrap();
    assert_eq!(p.events, 5);
    assert!(p.complete);
    assert_eq!(p.chunks.len(), 3);
    let expected = data::import_csv(
        &f.input,
        Profile::synthetic("EURUSD").unwrap(),
        "whole-source reference",
        true,
        1 << 20,
        100,
    )
    .unwrap();
    let mut actual = Vec::new();
    for chunk in &p.chunks {
        let d = native::read(&f.job.join(&chunk.file)).unwrap();
        assert_eq!(
            d.manifest.archive_hash,
            blake3::hash(&source).to_hex().to_string()
        );
        assert_eq!(d.manifest.class, DataClass::SyntheticTestOnly);
        actual.extend(d.ticks);
    }
    assert_eq!(actual, expected.ticks);
    assert_eq!(actual.last().unwrap().sequence, u64::MAX);
    let state = f.checkpoint();
    let repeated = f.step(2).unwrap();
    assert!(repeated.complete);
    assert_eq!(f.checkpoint(), state);
    assert_eq!(fs::read(f.job.join("chunk-000000.rsf")).unwrap(), first);
    assert_eq!(fs::read(&f.input).unwrap(), source);
    assert!(
        import_job::status(&f.job)
            .unwrap()
            .limitations
            .contains("not historical completeness")
    );
}
#[test]
fn changed_revision_profile_category_source_or_chunk_configuration_refuses() {
    let f = Fixture::new("revision", ROWS);
    f.step(2).unwrap();
    let state = f.checkpoint();
    assert!(f.step(3).is_err());
    let mut p = Profile::synthetic("EURUSD").unwrap();
    p.version = "changed".into();
    assert!(
        import_job::step(
            &f.input,
            &f.job,
            p,
            "SYNTHETIC_TEST_ONLY import fixture",
            true,
            2
        )
        .is_err()
    );
    assert!(
        import_job::step(
            &f.input,
            &f.job,
            Profile::synthetic("EURUSD").unwrap(),
            "changed source",
            true,
            2
        )
        .is_err()
    );
    assert!(
        import_job::step(
            &f.input,
            &f.job,
            Profile::synthetic("EURUSD").unwrap(),
            "SYNTHETIC_TEST_ONLY import fixture",
            false,
            2
        )
        .is_err()
    );
    fs::write(&f.input, format!("timestamp_ms,sequence,bid,ask\n{ROWS}\n")).unwrap();
    assert!(f.step(2).is_err());
    assert_eq!(f.checkpoint(), state);
}
#[test]
fn failed_tail_and_cross_chunk_order_never_advance_or_publish_bad_rows() {
    for (name, tail) in [
        ("order", "60000,2,1.00000,1.00010\n"),
        ("crossed", "60000,3,1.00020,1.00010\n"),
        ("malformed", "bad,3,1.00000,1.00010\n"),
    ] {
        let f = Fixture::new(
            name,
            &format!("0,1,1.00000,1.00010\n60000,2,1.00000,1.00010\n{tail}"),
        );
        f.step(2).unwrap();
        let state = f.checkpoint();
        assert!(f.step(2).is_err());
        assert_eq!(f.checkpoint(), state);
        assert!(!f.job.join("chunk-000001.rsf").exists());
        assert!(!import_job::status(&f.job).unwrap().complete);
    }
}
#[test]
fn crash_window_orphan_is_reused_exactly_and_conflicts_or_missing_artifacts_refuse() {
    let f = Fixture::new("orphan", ROWS);
    f.step(2).unwrap();
    let prior = f.checkpoint();
    f.step(2).unwrap();
    let path = f.job.join("chunk-000001.rsf");
    let bytes = fs::read(&path).unwrap();
    // Simulate publication surviving while the checkpoint transaction rolls back.
    f.restore(&prior);
    let p = f.step(2).unwrap();
    assert_eq!(p.events, 4);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    f.restore(&prior);
    let mut damaged = bytes.clone();
    damaged[0] ^= 1;
    fs::write(&path, &damaged).unwrap();
    assert!(f.step(2).is_err());
    assert_eq!(fs::read(&path).unwrap(), damaged);
    assert_eq!(f.checkpoint(), prior);
    fs::write(&path, bytes).unwrap();
    f.step(2).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(import_job::status(&f.job).is_err());
    assert!(f.step(2).is_err());
}
#[test]
fn checkpoint_corruption_and_unrelated_ledger_are_preserved_and_refused() {
    let f = Fixture::new("corruption", ROWS);
    f.step(2).unwrap();
    let state = f.checkpoint();
    let mut changed = state.clone();
    changed.0 = changed.0.replace("\"events\":2", "\"events\":1");
    f.restore(&changed);
    assert!(import_job::status(&f.job).is_err());
    assert!(f.step(2).is_err());
    let other = Fixture::new("unrelated", ROWS);
    fs::create_dir(&other.job).unwrap();
    let c = rusqlite::Connection::open(other.job.join("normalized-import.sqlite")).unwrap();
    c.execute_batch("CREATE TABLE unrelated(value);INSERT INTO unrelated VALUES('preserve');")
        .unwrap();
    assert!(other.step(2).is_err());
    assert_eq!(
        c.query_row("SELECT value FROM unrelated", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
}
#[test]
fn empty_and_budget_refusals_and_real_parsing_never_imply_approval() {
    let empty = Fixture::new("empty", "");
    assert!(empty.step(2).is_err());
    assert!(!import_job::status(&empty.job).unwrap().complete);
    let f = Fixture::new("budgets", ROWS);
    assert!(f.step(0).is_err());
    assert!(f.step(100001).is_err());
    assert!(!f.job.exists());
    let file = fs::OpenOptions::new().write(true).open(&f.input).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    drop(file);
    assert!(f.step(2).is_err());
    assert!(!f.job.exists());
    let real = Fixture::new("real", ROWS);
    let p = import_job::step(
        &real.input,
        &real.job,
        Profile::synthetic("EURUSD").unwrap(),
        "Fixture only; exercise real parsing classification, not acquired quotes",
        false,
        10,
    )
    .unwrap();
    assert!(p.complete);
    assert_eq!(
        native::read(&real.job.join(&p.chunks[0].file))
            .unwrap()
            .manifest
            .class,
        DataClass::RealStructurallyValid
    );
}

#[test]
fn competing_writer_refuses_without_advancing_checkpoint() {
    let f = Fixture::new("writer", ROWS);
    f.step(2).unwrap();
    let prior = f.checkpoint();
    let c = rusqlite::Connection::open(f.job.join("normalized-import.sqlite")).unwrap();
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(f.step(2).is_err());
    assert_eq!(f.checkpoint(), prior);
    c.execute_batch("ROLLBACK").unwrap();
    assert_eq!(f.step(2).unwrap().events, 4);
}

#[test]
fn separate_cli_processes_resume_and_inspect_the_same_job() {
    let f = Fixture::new("cli", ROWS);
    let profile = f.root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&Profile::synthetic("EURUSD").unwrap()).unwrap(),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    for (events, complete) in [(3, false), (5, true)] {
        let out = std::process::Command::new(binary)
            .args([
                "import-step",
                f.input.to_str().unwrap(),
                profile.to_str().unwrap(),
                "SYNTHETIC_TEST_ONLY CLI import",
                f.job.to_str().unwrap(),
                "synthetic",
                "3",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let p: import_job::Progress = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(p.events, events);
        assert_eq!(p.complete, complete);
    }
    let out = std::process::Command::new(binary)
        .args(["import-status", f.job.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let p: import_job::Progress = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(p.events, 5);
    assert!(p.complete);
    assert_eq!(p.chunks.len(), 2);
}

#[test]
fn csv_quoted_crlf_blank_tail_and_no_final_newline_resume_exactly() {
    for (name, text) in [
        (
            "crlf",
            "timestamp_ms,sequence,bid,ask\r\n\"0\",\"1\",\"1.00000\",\"1.00010\"\r\n60000,2,1.00020,1.00030\r\n\r\n",
        ),
        (
            "no-newline",
            "timestamp_ms,sequence,bid,ask\n0,1,1.00000,1.00010\n60000,2,1.00020,1.00030",
        ),
    ] {
        let f = Fixture::new(name, "");
        fs::write(&f.input, text).unwrap();
        let mut p = f.step(1).unwrap();
        assert_eq!(p.events, 1);
        for _ in 0..3 {
            if p.complete {
                break;
            }
            p = f.step(1).unwrap();
        }
        assert!(p.complete);
        assert_eq!(p.events, 2);
        assert_eq!(p.chunks.len(), 2);
        assert_eq!(p.next_byte, text.len() as u64);
        let expected = data::import_csv(
            &f.input,
            Profile::synthetic("EURUSD").unwrap(),
            "whole reference",
            true,
            1 << 20,
            10,
        )
        .unwrap();
        let actual = p
            .chunks
            .iter()
            .flat_map(|chunk| native::read(&f.job.join(&chunk.file)).unwrap().ticks)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected.ticks);
    }
}

#[test]
fn complete_assembly_recomputes_cross_chunk_diagnostics_and_refuses_partial_or_damaged() {
    let f = Fixture::new(
        "assemble",
        "0,1,1.00000,1.00010\n60000,2,1.00000,1.00010\n8000000,3,1.00000,1.00010\n8000000,4,1.00000,1.00010\n16000000,5,1.00000,1.00010\n",
    );
    f.step(2).unwrap();
    assert!(import_job::assemble(&f.job).is_err());
    f.step(2).unwrap();
    f.step(2).unwrap();
    let d = import_job::assemble(&f.job).unwrap();
    let expected = data::import_csv(
        &f.input,
        Profile::synthetic("EURUSD").unwrap(),
        "SYNTHETIC_TEST_ONLY import fixture",
        true,
        1 << 20,
        10,
    )
    .unwrap();
    assert_eq!(d.ticks, expected.ticks);
    assert_eq!(d.manifest.content_hash, expected.manifest.content_hash);
    assert_eq!(d.manifest.archive_hash, expected.manifest.archive_hash);
    assert_eq!(d.manifest.timestamp_ties, 1);
    assert_eq!(d.manifest.identical_quote_events, 4);
    assert_eq!(d.manifest.gaps_over_hour, 2);
    assert_eq!(d.manifest.class, DataClass::SyntheticTestOnly);
    assert_eq!(
        d.manifest.parser_version,
        "normalized-csv-chunks-assembled-v1"
    );
    assert_ne!(
        data::dataset_identity(&d.manifest).unwrap(),
        data::dataset_identity(&expected.manifest).unwrap()
    );
    assert_eq!(
        data::hash(&d).unwrap(),
        data::hash(&import_job::assemble(&f.job).unwrap()).unwrap()
    );
    fs::remove_file(f.job.join("chunk-000001.rsf")).unwrap();
    assert!(import_job::assemble(&f.job).is_err());
}

#[test]
fn assembled_source_preserves_nonzero_authoritative_replay_across_chunks_and_gaps() {
    use rust_strategy_factory::{
        candles::{self, Timeframe},
        contracts::{Conversion, Side},
        strategies::{Features, Rule, Strategy},
        tick_engine::{self, Costs},
    };
    let original = data::synthetic_fixture("EURUSD").unwrap();
    let f = Fixture::new("replay", "");
    let mut csv = String::from("timestamp_ms,sequence,bid,ask\n");
    for t in &original.ticks {
        csv.push_str(&format!(
            "{},{},{}.{:05},{}.{:05}\n",
            t.timestamp_ms,
            t.sequence,
            t.bid / 100000,
            t.bid % 100000,
            t.ask / 100000,
            t.ask % 100000
        ));
    }
    fs::write(&f.input, csv).unwrap();
    let mut p = f.step(333).unwrap();
    while !p.complete {
        p = f.step(333).unwrap();
    }
    let assembled = import_job::assemble(&f.job).unwrap();
    assert_eq!(assembled.ticks, original.ticks);
    let strategy = Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 50,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    };
    let replay = |d: &data::Dataset| {
        let c = candles::build(d, Timeframe::M1).unwrap();
        let mut features = Features::with_context(&c, 1_000_000, d, Timeframe::M1).unwrap();
        let mask = features.signal(&strategy.rule).unwrap();
        tick_engine::replay(
            d,
            &c,
            &mask,
            &strategy,
            &Costs::default(),
            &vec![Conversion::usd(); d.ticks.len()],
        )
        .unwrap()
    };
    let expected = replay(&original);
    assert!(!expected.trades.is_empty());
    assert_eq!(
        data::hash(&replay(&assembled)).unwrap(),
        data::hash(&expected).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn actual_sigkill_after_checkpoint_initialization_recovers_the_exact_source() {
    use std::{
        os::unix::process::ExitStatusExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let f = Fixture::new("sigkill", "");
    let mut csv = String::from("timestamp_ms,sequence,bid,ask\n");
    for i in 0..200000 {
        csv.push_str(&format!("{},{i},1.00000,1.00010\n", i * 1000));
    }
    fs::write(&f.input, &csv).unwrap();
    let profile = f.root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&Profile::synthetic("EURUSD").unwrap()).unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "import-step",
            f.input.to_str().unwrap(),
            profile.to_str().unwrap(),
            "SYNTHETIC_TEST_ONLY import fixture",
            f.job.to_str().unwrap(),
            "synthetic",
            "100000",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut initialized = false;
    while Instant::now() < deadline {
        if let Ok(c) = rusqlite::Connection::open_with_flags(
            f.job.join("normalized-import.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) && let Ok(body) =
            c.query_row("SELECT body FROM import_state WHERE singleton=1", [], |r| {
                r.get::<_, String>(0)
            })
        {
            let p: import_job::Progress = serde_json::from_str(&body).unwrap();
            if p.events == 0 {
                initialized = true;
                break;
            }
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let _ = child.kill();
    let status = child.wait().unwrap();
    assert!(initialized, "worker must initialize before kill");
    assert_eq!(status.signal(), Some(9));
    let before = import_job::status(&f.job).unwrap();
    assert!(before.events <= 100000);
    assert!(!before.complete);
    let mut p = f.step(100000).unwrap();
    while !p.complete {
        p = f.step(100000).unwrap();
    }
    assert_eq!(p.events, 200000);
    let d = import_job::assemble(&f.job).unwrap();
    let expected = data::import_csv(
        &f.input,
        Profile::synthetic("EURUSD").unwrap(),
        "whole source reference",
        true,
        64 * 1024 * 1024,
        200000,
    )
    .unwrap();
    assert_eq!(d.ticks, expected.ticks);
    assert_eq!(fs::read(&f.input).unwrap(), csv.as_bytes());
}
