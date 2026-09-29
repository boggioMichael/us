//! Plugins: optional, per-game help. The universal system runs the same
//! without any.
//!
//! A [`GamePlugin`] can recognise its game, add to what perception saw, add
//! or confirm concepts, bring known interface elements, entities and seed
//! facts, name its sources, pick Syrup's hat, add scene words, and propose
//! advice. Every hook defaults to nothing, so a plugin can be a single
//! method. Most plugins need no code at all: a `plugin.json` (see
//! `plugins/maplestory/plugin.json`) is a complete [`DataPlugin`].

pub mod data;
pub mod maplestory;

use syrup_core::knowledge::{Fact, KnowledgeNode, KnowledgeSource};
use syrup_core::{
    Advice, Confidence, Frame, GameIdentity, GameProfile, GameState, IdentityCues, Observation, SceneKind,
    VisualIdentity,
};
use syrup_state::ElementHint;

pub use data::{DataPlugin, PluginSpec};

/// What a plugin sees when it proposes advice.
pub struct PluginContext<'a> {
    pub now_ms: u64,
    pub identity: &'a GameIdentity,
    pub observation: &'a Observation,
    pub state: &'a GameState,
    pub profile: Option<&'a GameProfile>,
}

pub trait GamePlugin: Send {
    fn id(&self) -> &str;

    fn name(&self) -> &str;

    /// How sure the plugin is that this is its game.
    fn detect(&self, _cues: &IdentityCues, identity: &GameIdentity) -> Option<Confidence> {
        (identity.game_id == self.id()).then_some(identity.confidence)
    }

    /// Adds to what perception saw (a game-specific reading, say).
    fn parse_observation(&mut self, _frame: &Frame, _obs: &mut Observation) {}

    /// Adds or confirms concepts.
    fn extract_state(&mut self, _obs: &Observation, _state: &mut GameState) {}

    /// Interface elements the game is known to have.
    fn known_regions(&self) -> Vec<ElementHint> {
        Vec::new()
    }

    fn known_entities(&self) -> Vec<KnowledgeNode> {
        Vec::new()
    }

    /// Facts to start the knowledge graph with.
    fn seed_facts(&self) -> Vec<Fact> {
        Vec::new()
    }

    fn knowledge_sources(&self) -> Vec<KnowledgeSource> {
        Vec::new()
    }

    fn visual_theme(&self) -> Option<VisualIdentity> {
        None
    }

    /// Words that mean a particular screen in this game.
    fn scene_words(&self) -> Vec<(SceneKind, String)> {
        Vec::new()
    }

    fn genres(&self) -> Vec<String> {
        Vec::new()
    }

    fn advice(&mut self, _ctx: &PluginContext) -> Vec<Advice> {
        Vec::new()
    }
}

/// Keeps the plugins and knows which one (if any) is active.
pub struct PluginManager {
    plugins: Vec<Box<dyn GamePlugin>>,
    active: Option<usize>,
}

impl Default for PluginManager {
    fn default() -> Self {
        PluginManager::with_builtins()
    }
}

impl PluginManager {
    pub fn empty() -> Self {
        PluginManager { plugins: Vec::new(), active: None }
    }

    /// The plugins that ship with Syrup.
    pub fn with_builtins() -> Self {
        let mut m = PluginManager::empty();
        m.add(Box::new(maplestory::MapleStory::new()));
        m
    }

    pub fn add(&mut self, p: Box<dyn GamePlugin>) {
        self.plugins.retain(|x| x.id() != p.id());
        self.plugins.push(p);
    }

    /// Loads every `*/plugin.json` under `dir` (data plugins); returns their ids.
    pub fn load_dir(&mut self, dir: &std::path::Path) -> Vec<String> {
        let mut ids = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else { return ids };
        for e in entries.flatten() {
            let path = e.path().join("plugin.json");
            if let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(p) = DataPlugin::from_json(&text)
            {
                ids.push(p.id().to_string());
                self.add(Box::new(p));
            }
        }
        ids
    }

    pub fn ids(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.id()).collect()
    }

    /// Chooses the plugin for this game (or none); returns its id.
    pub fn activate_for(&mut self, cues: &IdentityCues, identity: &GameIdentity) -> Option<String> {
        let best = self
            .plugins
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.detect(cues, identity).map(|c| (i, c)))
            .filter(|(_, c)| c.at_least(0.5))
            .max_by(|a, b| a.1.value().total_cmp(&b.1.value()));
        self.active = best.map(|(i, _)| i);
        self.active.map(|i| self.plugins[i].id().to_string())
    }

    pub fn deactivate(&mut self) {
        self.active = None;
    }

    pub fn active(&mut self) -> Option<&mut (dyn GamePlugin + 'static)> {
        let i = self.active?;
        Some(self.plugins[i].as_mut())
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active.map(|i| self.plugins[i].id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(id: &str) -> GameIdentity {
        GameIdentity {
            game_id: id.into(),
            title: id.into(),
            confidence: Confidence::new(0.9),
            version: None,
            platform: None,
            evidence: Vec::new(),
            confirmed: false,
        }
    }

    #[test]
    fn the_right_plugin_activates_and_none_for_other_games() {
        let mut m = PluginManager::with_builtins();
        assert_eq!(m.activate_for(&IdentityCues::default(), &identity("maplestory")).as_deref(), Some("maplestory"));
        assert!(m.active().is_some());
        assert_eq!(m.activate_for(&IdentityCues::default(), &identity("elden-ring")), None);
        assert!(m.active().is_none());
    }

    #[test]
    fn plugins_load_from_a_folder() {
        let dir = std::env::temp_dir().join(format!("syrup-plugins-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mine")).unwrap();
        std::fs::write(
            dir.join("mine/plugin.json"),
            r#"{ "id": "my-game", "name": "My Game", "games": ["my-game"], "hat": "pirate_hat" }"#,
        )
        .unwrap();
        let mut m = PluginManager::empty();
        assert_eq!(m.load_dir(&dir), vec!["my-game".to_string()]);
        m.activate_for(&IdentityCues::default(), &identity("my-game"));
        assert_eq!(m.active().unwrap().visual_theme().unwrap().hat, syrup_core::Hat::PirateHat);
        let _ = std::fs::remove_dir_all(dir);
    }
}
