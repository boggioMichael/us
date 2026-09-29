//! The coach: what Syrup says, and when.
//!
//! Most of what happens produces no advice. Universal rules turn state,
//! transitions, knowledge and the player model into candidates, each with a
//! kind, an urgency, a confidence, the evidence behind it ("why?") and a
//! spoiler level; plugins add their own. The [`policy::SpoilerPolicy`] drops
//! or softens knowledge-based advice; the [`policy::InterruptionPolicy`]
//! decides what is worth interrupting for, given how busy the player is, how
//! recently Syrup spoke, what the player thought of this kind of advice, and
//! what they muted. Feedback (useful, wrong, explain, stop, research) changes
//! the weights for this player and game.
//!
//! Syrup talks in short sentences. "Health low. Back off." Not "Based on my
//! comprehensive analysis".

pub mod policy;

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use syrup_core::knowledge::{FactKind, SpoilerLevel};
use syrup_core::{
    Advice, AdviceKind, Confidence, Expression, Feedback, FeedbackKind, GameIdentity, GameState, Observation,
    SceneKind, Transition, TransitionKind, Urgency,
};
use syrup_knowledge::KnowledgeGraph;
use syrup_player::GameModel;

pub use policy::{InterruptionPolicy, Moment, SpoilerPolicy};

/// Things other modules want Syrup to mention.
#[derive(Debug, Clone, PartialEq)]
pub enum Announcement {
    /// The game was recognised (or is new).
    Game { identity: GameIdentity, first_time: bool },
    /// A concept is now known ("that red bar is health").
    Learned { concept: String, place: String, kind: String },
    /// Research finished.
    Researched { topic: String, facts: usize },
    /// The player model noticed a habit or a repeated mistake.
    Insight(String),
}

pub struct CoachContext<'a> {
    pub now_ms: u64,
    pub identity: Option<&'a GameIdentity>,
    pub state: &'a GameState,
    pub transitions: &'a [Transition],
    pub observation: Option<&'a Observation>,
    pub knowledge: Option<&'a KnowledgeGraph>,
    pub player: Option<&'a GameModel>,
    pub announcements: &'a [Announcement],
    /// Advice from plugins, judged like the rest.
    pub extra: Vec<Advice>,
}

#[derive(Debug, Clone, Default)]
pub struct CoachOutput {
    pub candidates: Vec<Advice>,
    pub shown: Vec<Advice>,
    pub suppressed: Vec<(Advice, String)>,
    /// Topics worth looking up (topic, why).
    pub research: Vec<(String, String)>,
}

/// What a piece of feedback asks the rest of Syrup to do.
#[derive(Debug, Clone, PartialEq)]
pub enum FeedbackEffect {
    Nothing,
    /// Show this explanation now.
    Explain(Advice),
    /// Look this up.
    Research(String),
    /// The concept this advice rested on may be wrong.
    DoubtConcept(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoachConfig {
    pub policy: InterruptionPolicy,
    pub spoilers: SpoilerPolicy,
    /// Say what Syrup is learning ("that red bar is health").
    pub narrate_learning: bool,
}

impl Default for CoachConfig {
    fn default() -> Self {
        CoachConfig { policy: InterruptionPolicy::default(), spoilers: SpoilerPolicy::Normal, narrate_learning: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoachStats {
    pub candidates: u64,
    pub shown: u64,
    pub suppressed: u64,
}

pub struct CoachEngine {
    pub cfg: CoachConfig,
    next_id: u64,
    last_spoken: Option<u64>,
    spoken: Vec<(String, u64, Urgency)>,
    queue: VecDeque<Advice>,
    /// Advice given lately, for feedback and "why?".
    pub recent: VecDeque<Advice>,
    boss: Option<String>,
    asked: Vec<String>,
    pub stats: CoachStats,
}

fn pick<'a>(id: u64, options: &[&'a str]) -> &'a str {
    options[(id as usize) % options.len()]
}

fn pct(f: f64) -> String {
    format!("{:.0}%", (f * 100.0).clamp(0.0, 100.0))
}

impl CoachEngine {
    pub fn new(cfg: CoachConfig) -> Self {
        CoachEngine {
            cfg,
            next_id: 1,
            last_spoken: None,
            spoken: Vec::new(),
            queue: VecDeque::new(),
            recent: VecDeque::new(),
            boss: None,
            asked: Vec::new(),
            stats: CoachStats::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn advice(
        &mut self,
        now: u64,
        topic: &str,
        kind: AdviceKind,
        urgency: Urgency,
        text: String,
        why: Vec<String>,
        confidence: f32,
        expression: Expression,
        ttl_ms: u64,
    ) -> Advice {
        let id = self.next_id;
        self.next_id += 1;
        Advice {
            id,
            topic: topic.into(),
            kind,
            urgency,
            text,
            why,
            confidence: Confidence::new(confidence),
            spoiler: SpoilerLevel::None,
            rests_on: Vec::new(),
            expression,
            origin: "syrup".into(),
            created_ms: now,
            expires_ms: now + ttl_ms,
        }
    }

    /// The universal rules: candidates for this moment.
    pub fn candidates(&mut self, ctx: &CoachContext) -> (Vec<Advice>, Vec<(String, String)>) {
        let now = ctx.now_ms;
        let s = ctx.state;
        let mut out = Vec::new();
        let mut research = Vec::new();
        let id = self.next_id;

        for a in ctx.announcements {
            match a {
                Announcement::Game { identity, first_time } => {
                    let (text, expr) = if !identity.is_known() {
                        (
                            pick(
                                id,
                                &[
                                    "New game. I'm watching and learning.",
                                    "Don't know this one yet. Learning it with you.",
                                ],
                            )
                            .to_string(),
                            Expression::Thinking,
                        )
                    } else if *first_time {
                        (format!("{}? New to me. Learning it.", identity.title), Expression::Excited)
                    } else {
                        (format!("{} again. I remember this one.", identity.title), Expression::Excited)
                    };
                    let why = identity
                        .evidence
                        .iter()
                        .map(|e| format!("{}: {}", e.signal.replace('_', " "), e.detail))
                        .collect();
                    out.push(self.advice(
                        now,
                        "game",
                        AdviceKind::Status,
                        Urgency::Educational,
                        text,
                        why,
                        identity.confidence.value().max(0.6),
                        expr,
                        30_000,
                    ));
                }
                Announcement::Learned { concept, place, kind } if self.cfg.narrate_learning => {
                    let what = if kind == "bar" {
                        format!("That bar at the {place} is {}.", concept.replace('_', " "))
                    } else {
                        format!("That number at the {place} is {}.", concept.replace('_', " "))
                    };
                    let a = self.advice(
                        now,
                        &format!("learned:{concept}"),
                        AdviceKind::Learning,
                        Urgency::Educational,
                        format!("{what} I think."),
                        vec![format!("learned {concept}")],
                        0.6,
                        Expression::Thinking,
                        20_000,
                    );
                    out.push(a);
                }
                Announcement::Researched { topic, facts } if *facts > 0 => {
                    let a = self.advice(
                        now,
                        &format!("research:{topic}"),
                        AdviceKind::Status,
                        Urgency::Educational,
                        format!("Looked up {topic}. Found {facts} things."),
                        vec![format!("{facts} facts from the web")],
                        0.7,
                        Expression::Researching,
                        20_000,
                    );
                    out.push(a);
                }
                Announcement::Insight(what) => {
                    let text = match what.as_str() {
                        "fights at low health" => "You keep fighting on low health. Heal earlier.".to_string(),
                        "dies from full health in seconds" => {
                            "Big hits take you from full. Watch for the wind-up.".to_string()
                        }
                        "keeps dying in the same place" => {
                            "Same spot, again. Try a different approach here.".to_string()
                        }
                        "lets the timer run low" => {
                            "You often wait until the timer is nearly out. Decide sooner.".to_string()
                        }
                        w if w.starts_with("runs out of") => format!("You {w} often. Pace yourself."),
                        w => format!("I noticed: you {w}."),
                    };
                    let a = self.advice(
                        now,
                        &format!("habit:{what}"),
                        AdviceKind::MechanicalCorrection,
                        Urgency::Opportunistic,
                        text,
                        vec![format!("seen several times: {what}")],
                        0.7,
                        Expression::Thinking,
                        60_000,
                    );
                    out.push(a);
                }
                _ => {}
            }
        }

        let playing = s.scene == SceneKind::Gameplay;
        // Resources running low.
        for (name, low, critical) in [
            ("health", 0.25, 0.15),
            ("shield", 0.15, 0.05),
            ("mana", 0.15, 0.05),
            ("stamina", 0.12, 0.04),
            ("energy", 0.12, 0.04),
        ] {
            let Some(c) = s.concept(name).filter(|c| c.confidence.at_least(0.5)) else { continue };
            let Some(f) = c.fraction() else { continue };
            if !playing || f > low {
                continue;
            }
            let crit = name == "health" && f <= critical && s.activity.intensity > 0.2;
            let text = match name {
                "health" if crit => {
                    pick(id, &["Health critical. Get out.", "Almost dead. Back off now.", "Health very low. Heal!"])
                        .to_string()
                }
                "health" => format!("{} Health at {}.", pick(id, &["Careful.", "Watch it.", "Heads up."]), pct(f)),
                "mana" => pick(id, &["Mana almost gone.", "Low on mana."]).to_string(),
                other => format!("{} almost gone.", capitalize(other)),
            };
            let why = std::iter::once(format!("{name} at {} ({})", pct(f), c.source))
                .chain(c.evidence.iter().take(3).map(|e| format!("it's {name} because: {e}")))
                .collect();
            let urgency = if crit { Urgency::Critical } else { Urgency::Important };
            out.push(self.advice(
                now,
                &format!("low:{name}"),
                AdviceKind::ImmediateWarning,
                urgency,
                text,
                why,
                c.confidence.value(),
                Expression::Warning,
                4000,
            ));
        }
        // Timer running out.
        if let Some(c) = s.concept("timer").filter(|c| c.confidence.at_least(0.5)) {
            let secs = c.value.filter(|_| c.unit == syrup_core::state::ConceptUnit::Seconds);
            let f = c.fraction();
            let low = secs.is_some_and(|v| v > 0.0 && v <= 3.0)
                || f.is_some_and(|f| f > 0.0 && f < 0.2 && c.trend == syrup_core::Trend::Falling);
            if low && playing {
                let a = self.advice(
                    now,
                    "timer",
                    AdviceKind::ImmediateWarning,
                    Urgency::Important,
                    pick(id, &["Time's almost up.", "Clock's running out. Decide."]).into(),
                    vec![format!("timer from {}", c.source)],
                    c.confidence.value(),
                    Expression::Warning,
                    3000,
                );
                out.push(a);
            }
        }
        // Ammo.
        if let Some(c) = s.concept("ammo").filter(|c| c.confidence.at_least(0.5))
            && let Some(v) = c.value
            && v <= 3.0
            && playing
        {
            let text = if v <= 0.0 { "Out of ammo.".to_string() } else { format!("{} shots left.", v as i64) };
            let a = self.advice(
                now,
                "low:ammo",
                AdviceKind::Tactical,
                Urgency::Important,
                text,
                vec![format!("ammo read from {}", c.source)],
                c.confidence.value(),
                Expression::Warning,
                4000,
            );
            out.push(a);
        }

        // A boss: its name, and what is known about it.
        let boss_name = self.boss_name(ctx);
        if let Some(name) = &boss_name
            && self.boss.as_deref() != Some(name.as_str())
        {
            self.boss = Some(name.clone());
            match self.knowledge_tip(ctx, name) {
                Some(tip) => out.push(tip),
                None => {
                    if !self.asked.contains(name) {
                        self.asked.push(name.clone());
                        research.push((name.clone(), "a boss appeared".into()));
                    }
                    let a = self.advice(
                        now,
                        &format!("boss:{name}"),
                        AdviceKind::Status,
                        Urgency::Opportunistic,
                        format!("{name}. New to me. Looking it up."),
                        vec!["a big bar at the top with a name".into()],
                        0.6,
                        Expression::Researching,
                        8000,
                    );
                    out.push(a);
                }
            }
        }
        if s.concept("boss_health").is_none() {
            self.boss = None;
        }

        for t in ctx.transitions {
            match t.kind {
                TransitionKind::PlayerDied => {
                    let repeated = ctx.transitions.iter().any(|x| x.kind == TransitionKind::RepeatedFailure);
                    if repeated {
                        let topic = boss_name.clone().or_else(|| self.boss.clone());
                        let tip = topic.as_ref().and_then(|b| self.knowledge_tip(ctx, b));
                        match tip {
                            Some(mut tip) => {
                                tip.text = format!("Again here. {}", tip.text);
                                tip.urgency = Urgency::Important;
                                out.push(tip);
                            }
                            None => {
                                let a = self.advice(
                                    now,
                                    "repeat-death",
                                    AdviceKind::Tactical,
                                    Urgency::Important,
                                    pick(
                                        id,
                                        &[
                                            "Third time here. Watch its pattern before you commit.",
                                            "Same place again. Try another approach: wait, watch, then go.",
                                        ],
                                    )
                                    .into(),
                                    vec![t.detail.clone(), "several deaths in a similar place".into()],
                                    0.7,
                                    Expression::Thinking,
                                    15_000,
                                );
                                out.push(a);
                                if let Some(b) = topic
                                    && !self.asked.contains(&b)
                                {
                                    self.asked.push(b.clone());
                                    research.push((b, "died here several times".into()));
                                }
                            }
                        }
                    } else {
                        let text = pick(id, &["Down. Shake it off.", "Unlucky. Again?", "That one hurt. Go again."])
                            .to_string();
                        let a = self.advice(
                            now,
                            "death",
                            AdviceKind::Learning,
                            Urgency::Opportunistic,
                            text,
                            vec!["the death screen".into()],
                            0.6,
                            Expression::Confused,
                            8000,
                        );
                        out.push(a);
                    }
                }
                TransitionKind::Victory => {
                    let a = self.advice(
                        now,
                        "victory",
                        AdviceKind::PostGame,
                        Urgency::Opportunistic,
                        pick(id, &["You won. Nice.", "Victory. Well played."]).into(),
                        vec!["a victory screen".into()],
                        0.8,
                        Expression::Proud,
                        15_000,
                    );
                    out.push(a);
                }
                TransitionKind::LevelUp => {
                    let a = self.advice(
                        now,
                        "level-up",
                        AdviceKind::Learning,
                        Urgency::Opportunistic,
                        pick(id, &["Level up. Nice.", "Leveled up!"]).into(),
                        vec![if t.detail.is_empty() { "the level went up".into() } else { t.detail.clone() }],
                        0.75,
                        Expression::Proud,
                        15_000,
                    );
                    out.push(a);
                }
                TransitionKind::ObjectiveChanged => {
                    let a = self.advice(
                        now,
                        "objective",
                        AdviceKind::ObjectiveReminder,
                        Urgency::Educational,
                        format!("New objective: {}.", t.detail.trim_end_matches('.')),
                        vec!["the objective text changed".into()],
                        0.6,
                        Expression::Neutral,
                        15_000,
                    );
                    out.push(a);
                }
                _ => {}
            }
        }
        (out, research)
    }

    /// The name printed with a boss's bar, if a boss is on screen.
    fn boss_name(&self, ctx: &CoachContext) -> Option<String> {
        let c = ctx.state.concept("boss_health").filter(|c| c.confidence.at_least(0.4))?;
        let obs = ctx.observation?;
        let id: u32 = c.source.strip_prefix("region:")?.parse().ok()?;
        let region = obs.region(id)?;
        obs.text
            .iter()
            .filter(|t| {
                t.rect.bottom() <= region.rect.y + 4
                    && region.rect.y - t.rect.bottom() < region.rect.h as i32 * 4
                    && t.rect.center().0 > region.rect.x as f32
                    && t.rect.center().0 < region.rect.right() as f32
            })
            .map(|t| t.text.trim().to_string())
            .find(|t| t.chars().filter(|c| c.is_alphabetic()).count() >= 3 && !t.chars().any(|c| c.is_ascii_digit()))
    }

    /// The best looked-up fact about `topic`, as advice (softened or withheld by the spoiler policy).
    fn knowledge_tip(&mut self, ctx: &CoachContext, topic: &str) -> Option<Advice> {
        let spoilers = self.cfg.spoilers;
        if spoilers == SpoilerPolicy::None {
            return None;
        }
        let graph = ctx.knowledge?;
        let facts = graph.about(topic, spoilers.max_spoiler());
        let fact = facts
            .into_iter()
            .find(|f| f.kind != FactKind::Speculation || spoilers == SpoilerPolicy::FullInformation)?;
        let label = match fact.kind {
            FactKind::CurrentPatch => "Patch notes",
            FactKind::CommunityConsensus => "Players say",
            FactKind::Speculation => "Rumour",
            FactKind::Inference => "I think",
            FactKind::Fact => "Wiki",
        };
        let text = if spoilers == SpoilerPolicy::HintsOnly {
            format!("Hint: I read something about {topic}. Think about its weaknesses.")
        } else {
            format!("{label}: {}", fact.claim.trim())
        };
        let stale = if fact.stale { " (from an older version)" } else { "" };
        let why = vec![
            format!("{} — {}{}", fact.source.title, fact.source.url, stale),
            format!("{} ({})", fact.kind.word(), fact.confidence),
        ];
        let mut a = self.advice(
            ctx.now_ms,
            &format!("boss:{topic}"),
            AdviceKind::Strategic,
            Urgency::Opportunistic,
            text,
            why,
            fact.confidence.value(),
            Expression::Researching,
            20_000,
        );
        a.spoiler = fact.spoiler;
        a.rests_on = vec![fact.kind];
        a.origin = "knowledge".into();
        Some(a)
    }

    /// One step: rules and plugin candidates, judged; returns what to say now.
    pub fn step(&mut self, ctx: &CoachContext) -> CoachOutput {
        let (mut candidates, research) = self.candidates(ctx);
        candidates.extend(ctx.extra.iter().cloned());
        self.stats.candidates += candidates.len() as u64;
        let mut out = CoachOutput { candidates: candidates.clone(), research, ..Default::default() };
        // Earlier candidates still fresh get another chance.
        let mut pool: Vec<Advice> = self.queue.drain(..).filter(|a| a.expires_ms > ctx.now_ms).collect();
        pool.extend(candidates);
        let moment = Moment {
            now_ms: ctx.now_ms,
            intensity: ctx.state.activity.intensity,
            scene: ctx.state.scene,
            last_spoken_ms: self.last_spoken,
            spoken: &self.spoken,
            player: ctx.player,
        };
        let mut scored: Vec<(f32, Advice)> = Vec::new();
        for a in pool {
            if let Err(why) = self.cfg.spoilers.allows(&a) {
                out.suppressed.push((a, why));
                continue;
            }
            match self.cfg.policy.value(&a, &moment) {
                Ok(v) => scored.push((v, a)),
                Err(why) => {
                    // Worth keeping for a calmer moment?
                    let wait = why.contains("busy")
                        || why.starts_with("spoke")
                        || why.contains("just spoke")
                        || why.contains("this minute");
                    if wait && a.urgency >= Urgency::Opportunistic && self.queue.len() < 4 {
                        self.queue.push_back(a);
                    } else {
                        out.suppressed.push((a, why));
                    }
                }
            }
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut first = true;
        for (_, a) in scored {
            if first {
                first = false;
                self.last_spoken = Some(ctx.now_ms);
                self.spoken.push((a.topic.clone(), ctx.now_ms, a.urgency));
                if self.spoken.len() > 200 {
                    self.spoken.remove(0);
                }
                self.recent.push_back(a.clone());
                while self.recent.len() > 50 {
                    self.recent.pop_front();
                }
                out.shown.push(a);
            } else if a.urgency >= Urgency::Opportunistic && self.queue.len() < 4 {
                self.queue.push_back(a);
            } else {
                out.suppressed.push((a, "something more important was said".into()));
            }
        }
        self.stats.shown += out.shown.len() as u64;
        self.stats.suppressed += out.suppressed.len() as u64;
        out
    }

    /// The player's reaction to a piece of advice.
    pub fn feedback(&mut self, fb: &Feedback, player: &mut GameModel) -> FeedbackEffect {
        let advice = self.recent.iter().find(|a| a.id == fb.advice_id).cloned();
        let kind_key = advice.as_ref().map(|a| format!("kind:{:?}", a.kind));
        let topic_key = format!("topic:{}", fb.topic);
        match fb.kind {
            FeedbackKind::Useful => {
                if let Some(k) = &kind_key {
                    player.scale_weight(k, 1.15);
                }
                player.scale_weight(&topic_key, 1.2);
                FeedbackEffect::Nothing
            }
            FeedbackKind::Wrong => {
                if let Some(k) = &kind_key {
                    player.scale_weight(k, 0.85);
                }
                player.scale_weight(&topic_key, 0.5);
                match fb.topic.strip_prefix("low:") {
                    Some(concept) => FeedbackEffect::DoubtConcept(concept.to_string()),
                    None => FeedbackEffect::Nothing,
                }
            }
            FeedbackKind::StopSuggesting => {
                player.muted_topics.insert(fb.topic.clone());
                FeedbackEffect::Nothing
            }
            FeedbackKind::Ignore => {
                if let Some(k) = &kind_key {
                    player.scale_weight(k, 0.97);
                }
                FeedbackEffect::Nothing
            }
            FeedbackKind::Explain => match advice {
                Some(a) => {
                    let text = if a.why.is_empty() {
                        "I just had a feeling. Not much evidence.".to_string()
                    } else {
                        format!("Because {}.", a.why.join("; "))
                    };
                    let mut e = self.advice(
                        fb.at_ms,
                        &format!("why:{}", a.topic),
                        AdviceKind::Status,
                        Urgency::Important,
                        text,
                        a.why.clone(),
                        1.0,
                        Expression::Thinking,
                        20_000,
                    );
                    e.origin = "explain".into();
                    FeedbackEffect::Explain(e)
                }
                None => FeedbackEffect::Nothing,
            },
            FeedbackKind::Research => {
                FeedbackEffect::Research(fb.topic.split(':').next_back().unwrap_or(&fb.topic).to_string())
            }
        }
    }

    /// The post-game summary.
    pub fn summary(
        &mut self,
        now: u64,
        title: &str,
        player: Option<&GameModel>,
        deaths: u32,
        minutes: f64,
        learned: &[String],
    ) -> Advice {
        let mut lines =
            vec![format!("{title}: {minutes:.0} min, {deaths} {}.", if deaths == 1 { "death" } else { "deaths" })];
        if !learned.is_empty() {
            lines.push(format!("I learned: {}.", learned.join(", ")));
        }
        if let Some(p) = player {
            lines.extend(p.highlights().into_iter().take(2));
            if let Some((habit, n)) = p.habits.iter().max_by_key(|(_, n)| **n)
                && *n >= 3
            {
                lines.push(format!("Habit to work on: {habit} ({n} times)."));
            }
        }
        self.advice(
            now,
            "summary",
            AdviceKind::PostGame,
            Urgency::Educational,
            lines.join(" "),
            vec!["this session".into()],
            0.9,
            Expression::Proud,
            u64::MAX / 2,
        )
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
