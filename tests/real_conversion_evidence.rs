//! Real-corpus conversion evidence for USD-base, non-USD-quote instruments.
//!
//! Reads actual retained `.rsf` ticks and the corpus manifest, and verifies that the
//! existing conversion primitive is exact, strictly event-aligned, provenance-bound and
//! never the USD identity sentinel for USDJPY / USDCHF / USDCAD.
//!
//! This is offline diagnostic evidence only. It grants no calendar completeness,
//! profile/broker approval, source licensing, research eligibility or qualification.

use rust_strategy_factory::{
    contracts::{Conversion, Profile},
    native,
};
use std::{collections::BTreeSet, fs, path::Path, path::PathBuf};

const CORPUS: &str = "/root/workspace/historical-migration-output/full-100k";
const MONTH: &str = "202503";
const INSTRUMENTS: [&str; 3] = ["USDJPY", "USDCHF", "USDCAD"];
const CHUNK_EVENTS: usize = 100_000;
const SAMPLE_LIMIT: usize = 64;

fn corpus_root() -> Option<PathBuf> {
    let root = PathBuf::from(CORPUS);
    if root.is_dir() { Some(root) } else { None }
}

fn is_hex_64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn corpus_manifest(root: &Path, symbol: &str) -> serde_json::Value {
    let month = root.join(format!("{symbol}-{MONTH}"));
    let text = fs::read_to_string(month.join("manifest.json")).expect("corpus manifest readable");
    serde_json::from_str(&text).expect("corpus manifest JSON")
}

fn corpus_profile(manifest: &serde_json::Value) -> Profile {
    let profile: Profile = serde_json::from_value(manifest["profile"].clone())
        .expect("corpus profile is a valid Profile");
    profile.validate().expect("corpus profile validates");
    profile
}

fn verify_instrument(root: &Path, symbol: &str) {
    let manifest = corpus_manifest(root, symbol);
    let profile = corpus_profile(&manifest);
    assert_eq!(profile.symbol, symbol);
    assert_eq!(profile.base, "USD");
    assert_ne!(
        profile.quote, "USD",
        "real corpus profile must be non-USD quote"
    );
    assert_eq!(
        manifest["class"].as_str(),
        Some("REAL_STRUCTURALLY_VALID"),
        "this evidence never upgrades the declared class"
    );

    let chunk = &manifest["chunks"][0];
    let file = chunk["file"].as_str().expect("chunk file name");
    let declared_events = chunk["events"].as_u64().expect("chunk events") as usize;
    let declared_hash = chunk["dataset_hash"].as_str().expect("chunk dataset hash");
    let month = root.join(format!("{symbol}-{MONTH}"));
    let path = month.join(file);

    // Bounded verified read: exact file length, header/manifest binding, payload checksum,
    // dataset verification and a fingerprint of the exact consumed bytes.
    let (dataset, consumed_hash) =
        native::read_bounded(&path, CHUNK_EVENTS).expect("verified native corpus chunk");
    assert_eq!(
        dataset.ticks.len(),
        declared_events,
        "declared event count binding"
    );
    assert_eq!(
        dataset.profile, profile,
        "native metadata profile equals manifest profile"
    );
    assert!(
        is_hex_64(&consumed_hash),
        "consumed-byte fingerprint is 64 hex chars"
    );

    // Re-read fingerprint determinism (same exact bytes, same provenance identity).
    let (again, consumed_hash_again) =
        native::read_bounded(&path, CHUNK_EVENTS).expect("verified native corpus chunk re-read");
    assert_eq!(
        consumed_hash_again, consumed_hash,
        "consumed-byte fingerprint is deterministic"
    );
    assert_eq!(again.ticks.len(), dataset.ticks.len());

    // Provenance bindings from the immutable migration manifest: source/archive,
    // dataset identity, quote stream, output receipt and per-chunk declaration.
    let provenance = [
        manifest["source"]["parquet_sha256"]
            .as_str()
            .expect("parquet sha"),
        manifest["source"]["dataset_hash"]
            .as_str()
            .expect("source dataset hash"),
        manifest["quote_stream_sha256"]
            .as_str()
            .expect("quote stream hash"),
        manifest["output_hash"].as_str().expect("output hash"),
        declared_hash,
    ];
    for value in provenance {
        assert!(is_hex_64(value), "manifest provenance hash is 64 hex chars");
    }

    // Deterministic bounded sample spanning the whole verified chunk.
    let n = dataset.ticks.len();
    let stride = std::cmp::max(1, n / SAMPLE_LIMIT);
    let mut sampled = 0usize;
    let mut index = 0usize;
    let mut rates = BTreeSet::new();
    while index < n && sampled < SAMPLE_LIMIT {
        let tick = &dataset.ticks[index];
        let price = tick.bid;
        let conv = Conversion::usd_account_from_usd_base_price(&profile, price, tick.timestamp_ms)
            .expect("conversion derived from the instrument's own real quote");

        // Strict event alignment and every declared validation safeguard.
        assert_eq!(
            conv.as_of_ms, tick.timestamp_ms,
            "conversion is the tick's own event time"
        );
        conv.validate(&profile, tick.timestamp_ms, 0)
            .expect("strict event-aligned conversion passes all safeguards");

        // Exact integer arithmetic: usd_num/quote_den == price_scale/price, reduced.
        assert_eq!(
            i128::from(conv.usd_num) * i128::from(price),
            i128::from(conv.quote_den) * i128::from(profile.price_scale),
            "exact cross-product equality for {symbol} tick {index}"
        );

        // No USD identity conversion for a non-USD quote.
        assert!(!(conv.usd_num == 1 && conv.quote_den == 1));
        assert!(
            Conversion::usd()
                .validate(&profile, tick.timestamp_ms, 0)
                .is_err(),
            "identity sentinel must be refused for {symbol}"
        );
        // Prior-only: future and stale conversions are refused; bounded staleness accepted.
        assert!(conv.validate(&profile, tick.timestamp_ms - 1, 0).is_err());
        assert!(conv.validate(&profile, tick.timestamp_ms + 1, 0).is_err());
        assert!(conv.validate(&profile, tick.timestamp_ms + 1, 1).is_ok());

        rates.insert((conv.usd_num, conv.quote_den));
        sampled += 1;
        index += stride;
    }
    assert_eq!(sampled, SAMPLE_LIMIT, "bounded sample is fully covered");
    assert!(
        rates.len() > 1,
        "real quotes yield more than one exact rate for {symbol}"
    );

    println!(
        "REAL_CONVERSION_EVIDENCE symbol={symbol} month={MONTH} chunk={file} events={n} sampled={sampled} \
         distinct_rates={} consumed_blake3={consumed_hash} parquet_sha256={} quote_stream_sha256={} \
         class=REAL_STRUCTURALLY_VALID eligibility=none",
        rates.len(),
        manifest["source"]["parquet_sha256"].as_str().unwrap(),
        manifest["quote_stream_sha256"].as_str().unwrap(),
    );
}

/// Negative control — the exact scale divergence this evidence guards against.
/// The synthetic USDJPY fixture uses price_scale 1000 / tick 1; the retained corpus uses
/// price_scale 1,000,000 / tick 1000. Applying the synthetic convention to a real corpus
/// tick silently produces a 1000x-wrong rate while still passing `Conversion::validate`.
#[test]
fn real_corpus_ticks_require_the_corpus_profile_not_the_synthetic_profile() {
    let Some(root) = corpus_root() else {
        if std::env::var_os("RSF_REAL_CORPUS_REQUIRED").is_some() {
            panic!("real corpus missing at {CORPUS} while RSF_REAL_CORPUS_REQUIRED is set");
        }
        eprintln!("SKIP: retained corpus not present at {CORPUS}");
        return;
    };
    let manifest = corpus_manifest(&root, "USDJPY");
    let real = corpus_profile(&manifest);
    let synthetic = Profile::synthetic("USDJPY").expect("synthetic USDJPY profile");
    assert_ne!(real.price_scale, synthetic.price_scale);
    assert_ne!(real.tick, synthetic.tick);

    let chunk = &manifest["chunks"][0];
    let path = root
        .join("USDJPY-202503")
        .join(chunk["file"].as_str().unwrap());
    let (dataset, _) = native::read_bounded(&path, CHUNK_EVENTS).expect("verified native chunk");
    let tick = &dataset.ticks[0];

    let real_rate =
        Conversion::usd_account_from_usd_base_price(&real, tick.bid, tick.timestamp_ms).unwrap();
    let wrong_rate =
        Conversion::usd_account_from_usd_base_price(&synthetic, tick.bid, tick.timestamp_ms)
            .expect("synthetic scale still validates: the hazard is silent");
    // Both pass their own validation, but the synthetic-profile rate is not the real rate.
    real_rate.validate(&real, tick.timestamp_ms, 0).unwrap();
    wrong_rate
        .validate(&synthetic, tick.timestamp_ms, 0)
        .unwrap();
    assert_ne!(
        real_rate, wrong_rate,
        "synthetic scale must never be applied to a real corpus quote"
    );
    println!(
        "NEGATIVE_CONTROL symbol=USDJPY real_rate={}/{} synthetic_rate={}/{} silent_validation=yes",
        real_rate.usd_num, real_rate.quote_den, wrong_rate.usd_num, wrong_rate.quote_den
    );
}

#[test]
fn real_corpus_non_usd_conversion_is_exact_event_aligned_and_provenance_bound() {
    let Some(root) = corpus_root() else {
        if std::env::var_os("RSF_REAL_CORPUS_REQUIRED").is_some() {
            panic!("real corpus missing at {CORPUS} while RSF_REAL_CORPUS_REQUIRED is set");
        }
        eprintln!("SKIP: retained corpus not present at {CORPUS}");
        return;
    };
    for symbol in INSTRUMENTS {
        verify_instrument(&root, symbol);
    }
}
