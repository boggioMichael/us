//! The player model: what Syrup has learned about how this person plays.
//!
//! Skills are estimates, not scores: each is a Beta distribution updated by
//! what happens (surviving a low-health moment is a success for resource
//! management; dying at full health in two seconds is a failure for
//! survival), reported as a mean *and* how sure Syrup is, and as "not enough
//! seen yet" while the evidence is thin. The model also keeps habits,
//! repeated mistakes, and how much each kind of advice has helped (from the
//! player's feedback), per game and overall.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use syrup_core::util::now_iso;
use syrup_core::{GameState, Transition, TransitionKind};

/// A skill estimate: Beta(alpha, beta), starting from Beta(1, 1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub alpha: f32,
    pub beta: f32,
}

impl Default for Skill {
    fn default() -> Self {
        Skill { alpha: 1.0, beta: 1.0 }
    }
}

impl Skill {
    pub fn success(&mut self, w: f32) {
        self.alpha += w.max(0.0);
    }

    pub fn failure(&mut self, w: f32) {
        self.beta += w.max(0.0);
    }

    pub fn mean(&self) -> f32 {
        self.alpha / (self.alpha + self.beta)
    }

    /// Observations behind the estimate.
    pub fn evidence(&self) -> f32 {
        self.alpha + self.beta - 2.0
    }

    /// How sure the estimate is, 0..1.
    pub fn confidence(&self) -> f32 {
        let n = self.evidence();
        n / (n + 6.0)
    }

    pub fn label(&self) -> &'static str {
        if self.evidence() < 4.0 {
            return "not enough seen yet";
        }
        match self.mean() {
            m if m >= 0.75 => "strong",
            m if m >= 0.55 => "solid",
            m if m >= 0.35 => "developing",
            _ => "struggling",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mistake {
    pub what: String,
    pub count: u32,
    pub last_at: String,
}

/// What Syrup knows about the player in one game.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GameModel {
    pub skills: BTreeMap<String, Skill>,
    pub habits: BTreeMap<String, u32>,
    pub mistakes: Vec<Mistake>,
    pub deaths: u32,
    pub wins: u32,
    pub level_ups: u32,
    pub sessions: u32,
    pub play_ms: u64,
    /// How much each kind of advice is worth to this player (1 = neutral).
    pub advice_weights: BTreeMap<String, f32>,
    /// Topics the player asked Syrup to stop suggesting.
    pub muted_topics: BTreeSet<String>,
}

impl GameModel {
    pub fn skill(&mut self, name: &str) -> &mut Skill {
        self.skills.entry(name.to_string()).or_default()
    }

    pub fn weight(&self, key: &str) -> f32 {
        self.advice_weights.get(key).copied().unwrap_or(1.0)
    }

    pub fn scale_weight(&mut self, key: &str, factor: f32) {
        let w = self.advice_weights.entry(key.to_string()).or_insert(1.0);
        *w = (*w * factor).clamp(0.15, 2.5);
    }

    fn habit(&mut self, name: &str) -> u32 {
        let n = self.habits.entry(name.to_string()).or_insert(0);
        *n += 1;
        *n
    }

    fn mistake(&mut self, what: &str) -> u32 {
        match self.mistakes.iter_mut().find(|m| m.what == what) {
            Some(m) => {
                m.count += 1;
                m.last_at = now_iso();
                m.count
            }
            None => {
                self.mistakes.push(Mistake { what: what.to_string(), count: 1, last_at: now_iso() });
                1
            }
        }
    }

    /// Skill estimates worth mentioning: sure enough, strongest or weakest first.
    pub fn highlights(&self) -> Vec<String> {
        let mut v: Vec<(&String, &Skill)> = self.skills.iter().filter(|(_, s)| s.evidence() >= 4.0).collect();
        v.sort_by(|a, b| (a.1.mean() - 0.5).abs().total_cmp(&(b.1.mean() - 0.5).abs()).reverse());
        v.into_iter()
            .take(3)
            .map(|(k, s)| format!("{}: {} ({:.0}% sure)", k.replace('_', " "), s.label(), s.confidence() * 100.0))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlayerModel {
    pub player_id: String,
    pub games: BTreeMap<String, GameModel>,
    /// Across all games.
    pub overall: BTreeMap<String, Skill>,
}

impl PlayerModel {
    pub fn new(player_id: &str) -> Self {
        PlayerModel { player_id: player_id.into(), ..Default::default() }
    }

    pub fn game(&mut self, game_id: &str) -> &mut GameModel {
        self.games.entry(game_id.to_string()).or_default()
    }

    fn both(&mut self, game_id: &str, skill: &str, success: bool, w: f32) {
        let s = self.game(game_id).skill(skill);
        if success {
            s.success(w)
        } else {
            s.failure(w)
        }
        let o = self.overall.entry(skill.to_string()).or_default();
        // Overall learns more slowly: games differ.
        if success { o.success(w * 0.5) } else { o.failure(w * 0.5) }
    }
}

/// Turns what happens in the game into evidence about the player.
#[derive(Debug, Default)]
pub struct PlayerTracker {
    low_health_since: Option<u64>,
    fights_low_flagged: bool,
    high_health: VecDeque<(u64, f64)>,
    intense_since: Option<u64>,
    survived_credit_at: u64,
    last_ms: Option<u64>,
    died_at: Option<u64>,
    repeated: bool,
}

impl PlayerTracker {
    pub fn new() -> Self {
        PlayerTracker::default()
    }

    /// Updates the model; returns insights worth telling the coach about
    /// (habits that just became clear).
    pub fn update(
        &mut self,
        model: &mut PlayerModel,
        game_id: &str,
        state: &GameState,
        transitions: &[Transition],
    ) -> Vec<String> {
        let now = state.timestamp_ms;
        let dt = self.last_ms.map(|t| now.saturating_sub(t).min(2000)).unwrap_or(0);
        self.last_ms = Some(now);
        model.game(game_id).play_ms += dt;
        let mut insights = Vec::new();
        let health = state.fraction_of("health", 0.45);
        let intense = state.activity.intensity > 0.35;
        // Health history, for "died from full health in seconds".
        if let Some(h) = health {
            self.high_health.push_back((now, h));
            while self.high_health.front().is_some_and(|(t, _)| now.saturating_sub(*t) > 5000) {
                self.high_health.pop_front();
            }
            if h < 0.25 {
                let since = *self.low_health_since.get_or_insert(now);
                if intense && now.saturating_sub(since) > 3000 && !self.fights_low_flagged {
                    self.fights_low_flagged = true;
                    let n = model.game(game_id).habit("fights at low health");
                    if n == 3 {
                        insights.push("fights at low health".into());
                    }
                }
            } else {
                if self.low_health_since.take().is_some() && h >= 0.5 {
                    // Came back from low health without dying.
                    model.both(game_id, "resource_management", true, 1.0);
                    model.both(game_id, "recovery", true, 0.5);
                }
                self.fights_low_flagged = false;
            }
        }
        // Long stretches of intense action survived.
        if intense {
            let since = *self.intense_since.get_or_insert(now);
            if now.saturating_sub(since) > 60_000 && now.saturating_sub(self.survived_credit_at) > 60_000 {
                self.survived_credit_at = now;
                model.both(game_id, "survival", true, 0.5);
            }
        } else {
            self.intense_since = None;
        }
        for t in transitions {
            match t.kind {
                TransitionKind::PlayerDied => {
                    let g = model.game(game_id);
                    g.deaths += 1;
                    model.both(game_id, "survival", false, 1.0);
                    let was_full = self.high_health.iter().any(|(ts, h)| now.saturating_sub(*ts) <= 4000 && *h > 0.8);
                    if was_full {
                        let n = model.game(game_id).habit("dies from full health in seconds");
                        model.both(game_id, "survival", false, 0.5);
                        if n == 2 {
                            insights.push("dies from full health in seconds".into());
                        }
                    }
                    if self.low_health_since.is_some() {
                        model.both(game_id, "resource_management", false, 1.0);
                    }
                    self.low_health_since = None;
                    self.died_at = Some(now);
                }
                TransitionKind::RepeatedFailure => {
                    self.repeated = true;
                    let n = model.game(game_id).mistake("keeps dying in the same place");
                    if n == 1 {
                        insights.push("keeps dying in the same place".into());
                    }
                }
                TransitionKind::Victory => {
                    model.game(game_id).wins += 1;
                    if self.repeated {
                        model.both(game_id, "persistence", true, 1.5);
                        self.repeated = false;
                    }
                    model.both(game_id, "survival", true, 0.5);
                }
                TransitionKind::LevelUp => {
                    model.game(game_id).level_ups += 1;
                    model.both(game_id, "progression", true, 0.5);
                }
                TransitionKind::SceneChanged
                    if self.died_at.is_some_and(|d| now.saturating_sub(d) < 30_000)
                        && state.scene == syrup_core::SceneKind::Gameplay =>
                {
                    // Back in after a death.
                    model.both(game_id, "persistence", true, 0.3);
                    self.died_at = None;
                }
                TransitionKind::ResourceEmpty
                    if t.subject == "mana" || t.subject == "stamina" || t.subject == "energy" =>
                {
                    model.both(game_id, "resource_management", false, 0.3);
                    let n = model.game(game_id).habit(&format!("runs out of {}", t.subject));
                    if n == 4 {
                        insights.push(format!("runs out of {}", t.subject));
                    }
                }
                TransitionKind::CounterIncreased if t.subject == "objective" => {
                    model.both(game_id, "objective_focus", true, 0.2)
                }
                TransitionKind::ResourceLow if t.subject == "timer" => {
                    model.both(game_id, "timing", false, 0.3);
                    let n = model.game(game_id).habit("lets the timer run low");
                    if n == 3 {
                        insights.push("lets the timer run low".into());
                    }
                }
                _ => {}
            }
        }
        insights
    }
}

#[cfg(test)]
mod tests {
    use syrup_core::state::ConceptUnit;
    use syrup_core::{ConceptValue, Confidence};

    use super::*;

    fn state(t: u64, health: f64, intensity: f32) -> GameState {
        let mut s = GameState { timestamp_ms: t, ..Default::default() };
        s.activity.intensity = intensity;
        s.concepts.insert(
            "health".into(),
            ConceptValue {
                name: "health".into(),
                value: Some(health),
                unit: ConceptUnit::Fraction,
                confidence: Confidence::new(0.9),
                ..Default::default()
            },
        );
        s
    }

    fn died(t: u64) -> Transition {
        Transition {
            ts_ms: t,
            kind: TransitionKind::PlayerDied,
            subject: "player".into(),
            from: None,
            to: None,
            detail: String::new(),
            confidence: Confidence::new(0.9),
        }
    }

    #[test]
    fn skills_are_estimates_with_confidence() {
        let mut s = Skill::default();
        assert_eq!(s.label(), "not enough seen yet");
        for _ in 0..8 {
            s.success(1.0);
        }
        s.failure(1.0);
        assert_eq!(s.label(), "strong");
        assert!(s.confidence() > 0.5 && s.confidence() < 1.0);
    }

    #[test]
    fn deaths_low_health_and_recoveries_teach_the_model() {
        let mut m = PlayerModel::new("me");
        let mut t = PlayerTracker::new();
        // Low health during a fight, then recovered: good resource management.
        for i in 0..6 {
            t.update(&mut m, "g", &state(i * 1000, 0.15, 0.6), &[]);
        }
        t.update(&mut m, "g", &state(7000, 0.8, 0.2), &[]);
        assert!(m.games["g"].skills["resource_management"].mean() > 0.5);
        assert_eq!(m.games["g"].habits.get("fights at low health"), Some(&1));
        // Two deaths from full health in seconds: a habit worth saying.
        let mut insights = Vec::new();
        for k in 0..2u64 {
            let base = 20_000 + k * 20_000;
            t.update(&mut m, "g", &state(base, 1.0, 0.8), &[]);
            insights.extend(t.update(&mut m, "g", &state(base + 2000, 0.0, 0.8), &[died(base + 2000)]));
        }
        assert_eq!(m.games["g"].deaths, 2);
        assert!(insights.iter().any(|i| i.contains("full health")), "{insights:?}");
        assert!(m.games["g"].skills["survival"].mean() < 0.5);
        let json = serde_json::to_string(&m).unwrap();
        let back: PlayerModel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }
}
