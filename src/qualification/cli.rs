//! Command-line front end for independent qualification (immutable decisions, evidence
//! gates and diverse boards). The trust root is operator-provisioned: `authority.secret`
//! credentials and the filesystem. Synthetic or diagnostic evidence can never qualify.
//!
//! The store revalidates live artifacts (registry experiment identity, frozen protocol
//! report, catalog eligibility) at Qualify time; those live arguments are optional for
//! Reject/Revoke, which do not rewrite history.
//!
//! See docs/RUNBOOKS/qualification.md for the full command contract.
use crate::Result;
use crate::print;
use crate::qualification::{self, Store};
use crate::read_dataset;
use crate::read_json;
use rust_strategy_factory::catalog::Catalog;
use rust_strategy_factory::review_authority::Authority;
use rust_strategy_factory::storage::Registry;
use std::io::Read;
use std::path::Path;

// === qualification-policy ===

pub fn policy() -> Result<()> {
    print(&qualification::Policy::owner_selected()?)
}

// === qualification-assess ===

pub fn assess(argc: usize, argv: &[String]) -> Result<()> {
    if argc != 3 {
        return Err("qualification-assess <bundle.json> <policy.json>".into());
    }
    let bundle: qualification::EvidenceBundle = read_json(&argv[1])?;
    let policy: qualification::Policy = read_json(&argv[2])?;
    let assessment = qualification::assess(&bundle, &policy)?;
    print(&assessment)
}

// === qualification-decide ===

/// Qualify requires complete gates plus live revalidation; Reject/Revoke only the
/// authenticated grant and head fencing.
pub fn decide(argc: usize, argv: &[String]) -> Result<()> {
    if !((3..=4).contains(&argc)) && false || (7..=11).contains(&argc) {
        return Err("qualification-decide <store.sqlite> <authority.secret> <reviewer> <request.json> <bundle.json> <policy.json> [registry.sqlite catalog.sqlite archive.csv dataset.json]".into());
    }
    let mut store = Store::open(Path::new(&argv[1]))?;
    let authority = Authority::load(Path::new(&argv[2]))?;
    let mut password = String::new();
    std::io::stdin()
        .read_to_string(&mut password)
        .map_err(|_| "review password input failed")?;
    let principal = authority
        .authenticate(&argv[3], password.trim())
        .map_err(|e| e.to_string())?;
    let request: qualification::Request = read_json(&argv[4])?;
    let bundle: qualification::EvidenceBundle = read_json(&argv[5])?;
    let policy: qualification::Policy = read_json(&argv[6])?;
    let mut registry: Option<Registry> = None;
    let mut catalog: Option<Catalog> = None;
    let mut archive: Option<&Path> = None;
    let mut dataset: Option<rust_strategy_factory::data::Dataset> = None;
    if argc == 11 {
        registry = Some(Registry::open(Path::new(&argv[7]))?);
        catalog = Some(Catalog::open(Path::new(&argv[8]))?);
        archive = Some(Path::new(&argv[9]));
        dataset = Some(read_dataset(&argv[10])?);
    }
    let live = if argc == 11 {
        Some(qualification::LiveEvidence {
            registry: registry.as_ref().unwrap(),
            catalog: catalog.as_ref().unwrap(),
            authority: &authority,
            archive: archive.as_ref().unwrap(),
            dataset: dataset.as_ref().unwrap(),
        })
    } else {
        None
    };
    let event = match request.decision {
        qualification::DecisionKind::Qualify => store
            .decide(
                &authority,
                &principal,
                request,
                &bundle,
                &policy,
                live.as_ref(),
            )
            .map_err(|e| e.to_string())?,
        qualification::DecisionKind::Reject | qualification::DecisionKind::Revoke => store
            .decide(&authority, &principal, request, &bundle, &policy, None)
            .map_err(|e| e.to_string())?,
    };
    print(&event.record)
}

// === qualification-board ===

pub fn board(argc: usize, argv: &[String]) -> Result<()> {
    if argc < 3 || argc > 4 {
        return Err("qualification-board <store.sqlite> <authority.secret> [instrument]".into());
    }
    let mut store = Store::open(Path::new(&argv[1]))?;
    let authority = Authority::load(Path::new(&argv[2]))?;
    let view = if argc == 3 {
        std::iter::once(store.board(&argv[3], &authority)?)
            .collect::<Vec<qualification::BoardView>>()
    } else {
        store.boards(&authority)?
    };
    print(&view)
}

// === qualification-scope ===

pub fn scope(argc: usize, argv: &[String]) -> Result<()> {
    if argc != 5 {
        return Err("qualification-scope <store.sqlite> <authority.secret> <instrument> <concept-id>".into());
    }
    let mut store = Store::open(Path::new(&argv[1]))?;
    let authority = Authority::load(Path::new(&argv[2]))?;
    let state = store
        .scope_state(&argv[3], &argv[4], &authority)
        .map_err(|e| e.to_string())?;
    print(&state)
}

// === qualification-history ===

pub fn history(argc: usize, argv: &[String]) -> Result<()> {
    if argc != 3 {
        return Err("qualification-history <store.sqlite> <filter.json>".into());
    }
    let mut store = Store::open(Path::new(&argv[1]))?;
    let filter: qualification::HistoryFilter = read_json(&argv[2])?;
    let page = store.history(&filter).map_err(|e| e.to_string())?;
    print(&page)
}

// === qualification-leaders ===

pub fn leaders(argc: usize, argv: &[String]) -> Result<()> {
    if !(3..=4).contains(&argc) {
        return Err("qualification-leaders <store.sqlite> <instrument> [after-sequence-or-- [limit]]".into());
    }
    let mut store = Store::open(Path::new(&argv[1]))?;
    let instrument = &argv[2];
    let (after, limit) = if argc == 3 {
        match &argv[2][..] {
            "--" => (None, 200),
            s => {
                let n = s
                    .parse()
                    .map_err(|_| "leaders after-sequence must be an integer or --")?;
                (Some(n), 200)
            }
        }
    } else {
        let after = if argv[3] == "--" {
            None
        } else {
            Some(
                argv[3]
                    .parse()
                    .map_err(|_| "leaders after-sequence must be an integer or --")?,
            )
        };
        let limit = argv[4]
            .parse()
            .map_err(|_| "leaders limit must be an integer 1..200")?;
        (after, limit)
    };
    if !(1..=200).contains(&limit) {
        return Err("leaders limit must be 1..200".into());
    }
    let (snapshots, next) = store
        .leaders(instrument, after, limit)
        .map_err(|e| e.to_string())?;
    print(&(snapshots, next))
}

// === qualification-backup ===

pub fn backup(argc: usize, argv: &[String]) -> Result<()> {
    if argc != 3 {
        return Err("qualification-backup <store.sqlite> <new-backup.sqlite>".into());
    }
    let mut store = Store::open(Path::new(&argv[1]))?;
    store
        .backup(Path::new(&argv[2]))
        .map_err(|e| e.to_string())?;
    Ok(())
}
