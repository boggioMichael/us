//! A plugin made only of data: a `plugin.json`.

use std::collections::BTreeMap;

use serde::Deserialize;
use syrup_core::advice::{AdviceKind, Expression, Urgency};
use syrup_core::knowledge::{
    Fact, FactKind, KnowledgeNode, KnowledgeSource, NodeKind, SourceKind, SourceRef, SpoilerLevel, node_id,
};
use syrup_core::util::now_iso;
use syrup_core::{Advice, Confidence, GameIdentity, Hat, IdentityCues, NormRect, SceneKind, VisualIdentity};
use syrup_state::ElementHint;

use crate::{GamePlugin, PluginContext};

#[derive(Debug, Clone, Deserialize)]
pub struct ElementSpec {
    pub concept: String,
    pub kind: String,
    /// x, y, w, h as fractions of the frame.
    pub norm: [f32; 4],
    #[serde(default)]
    pub hue: Option<[f32; 2]>,
    #[serde(default = "half")]
    pub confidence: f32,
}

fn half() -> f32 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceSpec {
    pub kind: SourceKind,
    pub provider: String,
    pub locator: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EntitySpec {
    pub name: String,
    pub kind: NodeKind,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FactSpec {
    pub claim: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default = "fact_kind")]
    pub kind: FactKind,
    #[serde(default)]
    pub spoiler: SpoilerLevel,
}

fn fact_kind() -> FactKind {
    FactKind::Fact
}

#[derive(Debug, Clone, Deserialize)]
pub struct TipSpec {
    /// Say it when any of these words is on screen.
    pub when_text: Vec<String>,
    pub text: String,
    #[serde(default = "opportunistic")]
    pub urgency: Urgency,
}

fn opportunistic() -> Urgency {
    Urgency::Opportunistic
}

/// Everything a `plugin.json` can say.
#[derive(Debug, Clone, Deserialize)]
pub struct PluginSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub about: String,
    /// Game ids (as the recogniser names them) this plugin is for.
    #[serde(default)]
    pub games: Vec<String>,
    #[serde(default)]
    pub hat: Option<Hat>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub sources: Vec<SourceSpec>,
    #[serde(default)]
    pub elements: Vec<ElementSpec>,
    #[serde(default)]
    pub scene_words: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub entities: Vec<EntitySpec>,
    #[serde(default)]
    pub facts: Vec<FactSpec>,
    #[serde(default)]
    pub tips: Vec<TipSpec>,
}

pub struct DataPlugin {
    pub spec: PluginSpec,
    said: BTreeMap<usize, u64>,
    next_id: u64,
}

impl DataPlugin {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let spec: PluginSpec = serde_json::from_str(text).map_err(|e| e.to_string())?;
        Ok(DataPlugin { spec, said: BTreeMap::new(), next_id: 1_000_000 })
    }

    fn source(&self) -> SourceRef {
        SourceRef {
            kind: SourceKind::Plugin,
            title: format!("{} plugin", self.spec.name),
            url: format!("plugin://{}", self.spec.id),
        }
    }
}

fn scene_kind(name: &str) -> Option<SceneKind> {
    Some(match name {
        "gameplay" => SceneKind::Gameplay,
        "menu" => SceneKind::Menu,
        "loading" => SceneKind::Loading,
        "dialogue" => SceneKind::Dialogue,
        "cutscene" => SceneKind::Cutscene,
        "defeat" => SceneKind::Defeat,
        "victory" => SceneKind::Victory,
        _ => return None,
    })
}

impl GamePlugin for DataPlugin {
    fn id(&self) -> &str {
        &self.spec.id
    }

    fn name(&self) -> &str {
        &self.spec.name
    }

    fn detect(&self, _cues: &IdentityCues, identity: &GameIdentity) -> Option<Confidence> {
        (self.spec.games.contains(&identity.game_id) || identity.game_id == self.spec.id).then_some(identity.confidence)
    }

    fn known_regions(&self) -> Vec<ElementHint> {
        self.spec
            .elements
            .iter()
            .map(|e| ElementHint {
                norm: NormRect { x: e.norm[0], y: e.norm[1], w: e.norm[2], h: e.norm[3] },
                kind: e.kind.clone(),
                concept: e.concept.clone(),
                confidence: e.confidence,
                corrected: false,
            })
            .collect()
    }

    fn known_entities(&self) -> Vec<KnowledgeNode> {
        self.spec
            .entities
            .iter()
            .map(|e| KnowledgeNode {
                id: node_id(&e.name),
                name: e.name.clone(),
                kind: e.kind,
                aliases: Vec::new(),
                summary: e.summary.clone(),
                facts: Vec::new(),
                provenance: format!("{} plugin", self.spec.name),
            })
            .collect()
    }

    fn seed_facts(&self) -> Vec<Fact> {
        let source = self.source();
        self.spec
            .facts
            .iter()
            .enumerate()
            .map(|(i, f)| Fact {
                id: format!("{}-{}", self.spec.id, i + 1),
                claim: f.claim.clone(),
                subject: f.subject.clone(),
                kind: f.kind,
                source: source.clone(),
                retrieved_at: now_iso(),
                game_version: None,
                confidence: Confidence::new(
                    SourceKind::Plugin.authority() * if f.kind == FactKind::Fact { 0.95 } else { 0.8 },
                ),
                spoiler: f.spoiler,
                stale: false,
            })
            .collect()
    }

    fn knowledge_sources(&self) -> Vec<KnowledgeSource> {
        self.spec
            .sources
            .iter()
            .map(|s| KnowledgeSource { kind: s.kind, provider: s.provider.clone(), locator: s.locator.clone() })
            .collect()
    }

    fn visual_theme(&self) -> Option<VisualIdentity> {
        self.spec.hat.map(|hat| VisualIdentity { hat, accent: None })
    }

    fn scene_words(&self) -> Vec<(SceneKind, String)> {
        self.spec
            .scene_words
            .iter()
            .filter_map(|(k, words)| scene_kind(k).map(|kind| words.iter().map(move |w| (kind, w.clone()))))
            .flatten()
            .collect()
    }

    fn genres(&self) -> Vec<String> {
        self.spec.genres.clone()
    }

    fn advice(&mut self, ctx: &PluginContext) -> Vec<Advice> {
        let text = ctx.observation.all_text_lower();
        let mut out = Vec::new();
        for (i, tip) in self.spec.tips.iter().enumerate() {
            let hit = tip.when_text.iter().find(|w| syrup_state::has_word(&text, &w.to_lowercase()));
            let Some(word) = hit else { continue };
            // One tip at a time, not more than once a minute.
            if self.said.get(&i).is_some_and(|t| ctx.now_ms.saturating_sub(*t) < 60_000) || !out.is_empty() {
                continue;
            }
            self.said.insert(i, ctx.now_ms);
            self.next_id += 1;
            out.push(Advice {
                id: self.next_id,
                topic: format!("{}:tip:{i}", self.spec.id),
                kind: AdviceKind::Tactical,
                urgency: tip.urgency,
                text: tip.text.clone(),
                why: vec![format!("\"{word}\" on screen"), format!("from the {} plugin", self.spec.name)],
                confidence: Confidence::new(0.7),
                spoiler: SpoilerLevel::None,
                rests_on: vec![FactKind::CommunityConsensus],
                expression: Expression::Excited,
                origin: format!("plugin:{}", self.spec.id),
                created_ms: ctx.now_ms,
                expires_ms: ctx.now_ms + 8000,
            });
        }
        out
    }
}
