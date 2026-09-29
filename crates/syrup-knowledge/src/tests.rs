//! Research against recorded responses (a fictional game's wiki, store page
//! and news, in `fixtures/`), so the whole pipeline runs offline.

use std::path::PathBuf;
use std::sync::Arc;

use syrup_core::knowledge::{FactKind, Relation, SourceKind, SpoilerLevel};

use super::*;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn question(topic: Option<&str>) -> ResearchQuestion {
    ResearchQuestion {
        game_id: "sky-meadow-online".into(),
        game_title: "Sky Meadow Online".into(),
        topic: topic.map(|t| t.to_string()),
        reason: "test".into(),
        sources: Vec::new(),
        version: Some("1.3".into()),
    }
}

#[test]
fn a_new_game_is_researched_from_its_store_page_encyclopedia_and_wiki() {
    let fx = Arc::new(Fixtures::new(fixtures()));
    let agent = ResearchAgent::new(fx.clone());
    let out = agent.research(&question(None));
    let missing = fx.missing.lock().unwrap().clone();
    assert!(out.facts.len() >= 4, "facts: {:#?}\nmissing: {missing:#?}\nfailures: {:#?}", out.facts, out.failures);
    // Sources were discovered and are remembered.
    let providers: Vec<&str> = out.sources_found.iter().map(|s| s.provider.as_str()).collect();
    assert!(
        providers.contains(&"mediawiki") && providers.contains(&"steam") && providers.contains(&"wikipedia"),
        "{providers:?}"
    );
    // Genres for the profile (and Syrup's hat).
    assert!(out.genres.contains(&"mmorpg".to_string()), "{:?}", out.genres);
    // Patch notes are current-patch facts, with the version.
    assert!(out.facts.iter().any(|f| f.kind == FactKind::CurrentPatch && f.source.kind == SourceKind::PatchNotes));
    assert_eq!(out.latest_version.as_deref(), Some("1.3.1"));
    // Every fact says where it came from.
    assert!(out.facts.iter().all(|f| f.source.url.starts_with("https://") && !f.retrieved_at.is_empty()));
}

#[test]
fn a_boss_is_researched_with_relations_spoilers_and_versions() {
    let fx = Arc::new(Fixtures::new(fixtures()));
    let mut q = question(Some("Mossy King"));
    q.sources.push(syrup_core::knowledge::KnowledgeSource {
        kind: SourceKind::Wiki,
        provider: "mediawiki".into(),
        locator: "https://skymeadowonline.fandom.com/api.php".into(),
    });
    let out = ResearchAgent::new(fx.clone()).research(&q);
    let missing = fx.missing.lock().unwrap().clone();
    assert!(!out.facts.is_empty(), "missing: {missing:#?} failures: {:?}", out.failures);
    assert!(
        out.relations.iter().any(|(a, r, b, _)| a == "Mossy King" && *r == Relation::WeakAgainst && b.contains("fire")),
        "{:?}",
        out.relations
    );
    assert!(out.facts.iter().any(|f| f.kind == FactKind::CommunityConsensus));
    assert!(out.facts.iter().any(|f| f.kind == FactKind::Speculation));
    // A fact about an old version is flagged.
    assert!(out.facts.iter().any(|f| f.stale), "{:#?}", out.facts);
    let mut g = KnowledgeGraph::new("sky-meadow-online");
    let added = apply(&mut g, &q, &out);
    assert_eq!(added, out.facts.len());
    let best = g.about("Mossy King", SpoilerLevel::Mild);
    assert!(!best.is_empty());
    assert!(!best[0].stale && best[0].kind != FactKind::Speculation, "{:?}", best[0]);
    assert!(
        g.about("Mossy King", SpoilerLevel::None).len() < g.about("Mossy King", SpoilerLevel::Major).len()
            || !g.facts.iter().any(|f| f.spoiler > SpoilerLevel::None)
    );
    assert_eq!(g.researched.len(), 1);
}

#[test]
fn with_research_off_nothing_is_fetched() {
    let out = ResearchAgent::new(Arc::new(Offline)).research(&question(None));
    assert!(out.facts.is_empty());
    assert!(out.failures.iter().any(|f| f.contains("research is off")));
}

#[test]
fn the_worker_runs_off_the_loop() {
    let mut w = ResearchWorker::start(Arc::new(Fixtures::new(fixtures())));
    w.ask(question(None));
    let done = w.wait(std::time::Duration::from_secs(10));
    assert_eq!(done.len(), 1);
    assert_eq!(w.pending(), 0);
}
