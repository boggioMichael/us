use syrup_core::knowledge::{Fact, FactKind, SourceKind, SourceRef, SpoilerLevel};
use syrup_core::observation::TextItem;
use syrup_core::state::ConceptUnit;
use syrup_core::{ConceptValue, Rect, UiKind, UiRegion};

use super::*;

fn state(t: u64, health: f64, intensity: f32) -> GameState {
    let mut s = GameState { timestamp_ms: t, scene: SceneKind::Gameplay, ..Default::default() };
    s.activity.intensity = intensity;
    s.concepts.insert(
        "health".into(),
        ConceptValue {
            name: "health".into(),
            value: Some(health),
            unit: ConceptUnit::Fraction,
            source: "region:1".into(),
            confidence: Confidence::new(0.9),
            evidence: vec!["red bar".into()],
            ..Default::default()
        },
    );
    s
}

fn ctx<'a>(t: u64, s: &'a GameState, tr: &'a [Transition], p: Option<&'a GameModel>) -> CoachContext<'a> {
    CoachContext {
        now_ms: t,
        identity: None,
        state: s,
        transitions: tr,
        observation: None,
        knowledge: None,
        player: p,
        announcements: &[],
        extra: Vec::new(),
    }
}

#[test]
fn low_health_is_said_once_then_not_repeated_too_soon() {
    let mut c = CoachEngine::new(CoachConfig::default());
    let s = state(10_000, 0.2, 0.1);
    let out = c.step(&ctx(10_000, &s, &[], None));
    assert_eq!(out.shown.len(), 1, "{:?}", out.suppressed);
    assert!(out.shown[0].text.contains("20%"), "{}", out.shown[0].text);
    assert_eq!(out.shown[0].kind, AdviceKind::ImmediateWarning);
    let s = state(15_000, 0.18, 0.1);
    let out = c.step(&ctx(15_000, &s, &[], None));
    assert!(out.shown.is_empty());
    assert!(out.suppressed.iter().any(|(_, why)| why.contains("ago")), "{:?}", out.suppressed);
}

#[test]
fn critical_goes_through_when_busy_and_small_talk_does_not() {
    let mut c = CoachEngine::new(CoachConfig::default());
    let s = state(1000, 0.1, 0.9);
    let level_up = Transition {
        ts_ms: 1000,
        kind: TransitionKind::LevelUp,
        subject: "level".into(),
        from: None,
        to: None,
        detail: String::new(),
        confidence: Confidence::new(0.8),
    };
    let out = c.step(&ctx(1000, &s, std::slice::from_ref(&level_up), None));
    assert_eq!(out.shown.len(), 1);
    assert_eq!(out.shown[0].urgency, Urgency::Critical);
    // The level-up waits for a calmer moment rather than interrupting now.
    let calm = state(10_000, 0.9, 0.05);
    let out = c.step(&ctx(10_000, &calm, &[], None));
    assert!(out.shown.iter().any(|a| a.topic == "level-up"), "{:?} / {:?}", out.shown, out.suppressed);
}

#[test]
fn feedback_mutes_explains_and_doubts() {
    let mut c = CoachEngine::new(CoachConfig::default());
    let mut player = GameModel::default();
    let s = state(10_000, 0.2, 0.1);
    let shown = c.step(&ctx(10_000, &s, &[], Some(&player))).shown.remove(0);
    let explain = c.feedback(
        &Feedback { advice_id: shown.id, topic: shown.topic.clone(), kind: FeedbackKind::Explain, at_ms: 10_500 },
        &mut player,
    );
    match explain {
        FeedbackEffect::Explain(a) => assert!(a.text.contains("health at 20%"), "{}", a.text),
        e => panic!("{e:?}"),
    }
    assert_eq!(
        c.feedback(
            &Feedback { advice_id: shown.id, topic: shown.topic.clone(), kind: FeedbackKind::Wrong, at_ms: 11_000 },
            &mut player
        ),
        FeedbackEffect::DoubtConcept("health".into())
    );
    c.feedback(
        &Feedback {
            advice_id: shown.id,
            topic: shown.topic.clone(),
            kind: FeedbackKind::StopSuggesting,
            at_ms: 11_000,
        },
        &mut player,
    );
    let s = state(120_000, 0.2, 0.1);
    let out = c.step(&ctx(120_000, &s, &[], Some(&player)));
    assert!(out.shown.is_empty());
    assert!(out.suppressed.iter().any(|(_, why)| why.contains("muted")), "{:?}", out.suppressed);
}

fn boss_scene() -> (GameState, Observation) {
    let mut s = state(5000, 0.9, 0.1);
    s.concepts.insert(
        "boss_health".into(),
        ConceptValue {
            name: "boss_health".into(),
            value: Some(0.8),
            source: "region:7".into(),
            confidence: Confidence::new(0.7),
            ..Default::default()
        },
    );
    let rect = Rect::new(280, 58, 400, 18);
    let obs = Observation {
        timestamp_ms: 5000,
        frame_size: (960, 540),
        ui_regions: vec![UiRegion {
            id: 7,
            rect,
            norm: rect.to_norm(960, 540),
            kind: UiKind::Bar { fill: 0.8, color: [150, 60, 200], vertical: false },
            stability: 1.0,
            confidence: Confidence::new(0.9),
            appearance: 1,
        }],
        text: vec![TextItem {
            text: "Mossy King".into(),
            rect: Rect::new(430, 34, 100, 20),
            region: None,
            confidence: Confidence::new(0.9),
            engine: "test".into(),
            fresh: true,
        }],
        ..Default::default()
    };
    (s, obs)
}

fn graph() -> KnowledgeGraph {
    let mut g = KnowledgeGraph::new("sky");
    let fact = |claim: &str, kind: FactKind, spoiler: SpoilerLevel| Fact {
        id: String::new(),
        claim: claim.into(),
        subject: Some("Mossy King".into()),
        kind,
        source: SourceRef {
            kind: SourceKind::Wiki,
            title: "Sky wiki: Mossy King".into(),
            url: "https://skymeadowonline.fandom.com/wiki/Mossy_King".into(),
        },
        retrieved_at: "2026-09-29T00:00:00Z".into(),
        game_version: None,
        confidence: Confidence::new(0.76),
        spoiler,
        stale: false,
    };
    g.add_fact(fact("The Mossy King is weak to fire.", FactKind::Fact, SpoilerLevel::None));
    g.add_fact(fact(
        "Defeating the Mossy King reveals that the king is actually the lost gardener.",
        FactKind::Fact,
        SpoilerLevel::Major,
    ));
    g
}

#[test]
fn a_boss_gets_a_looked_up_tip_or_a_research_request() {
    let (s, obs) = boss_scene();
    let mut c = CoachEngine::new(CoachConfig::default());
    let mut cx = ctx(5000, &s, &[], None);
    cx.observation = Some(&obs);
    let out = c.step(&cx);
    assert_eq!(out.research, vec![("Mossy King".to_string(), "a boss appeared".to_string())]);
    let g = graph();
    let mut c = CoachEngine::new(CoachConfig::default());
    cx.knowledge = Some(&g);
    let out = c.step(&cx);
    let tip = out.shown.iter().find(|a| a.origin == "knowledge").unwrap_or_else(|| panic!("{:?}", out.suppressed));
    assert!(tip.text.contains("weak to fire") && tip.text.starts_with("Wiki:"), "{}", tip.text);
    assert!(tip.why.iter().any(|w| w.contains("fandom.com")), "provenance in why: {:?}", tip.why);
    assert!(!out.shown.iter().any(|a| a.text.contains("gardener")), "no story spoilers on normal");
}

#[test]
fn the_spoiler_policy_softens_or_withholds() {
    let (s, obs) = boss_scene();
    let g = graph();
    for (policy, expect) in [(SpoilerPolicy::HintsOnly, Some("Hint:")), (SpoilerPolicy::None, None)] {
        let mut c = CoachEngine::new(CoachConfig { spoilers: policy, ..Default::default() });
        let mut cx = ctx(5000, &s, &[], None);
        cx.observation = Some(&obs);
        cx.knowledge = Some(&g);
        let out = c.step(&cx);
        let tip = out.shown.iter().chain(out.suppressed.iter().map(|(a, _)| a)).find(|a| a.origin == "knowledge");
        match expect {
            Some(prefix) => assert!(tip.is_some_and(|t| t.text.starts_with(prefix)), "{policy:?}: {tip:?}"),
            None => assert!(tip.is_none(), "{policy:?}: {tip:?}"),
        }
    }
}
