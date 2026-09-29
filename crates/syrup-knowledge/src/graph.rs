//! The knowledge graph: facts with provenance, and the nodes and edges
//! read from them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use syrup_core::Confidence;
use syrup_core::knowledge::{Fact, FactKind, KnowledgeEdge, KnowledgeNode, NodeKind, Relation, SpoilerLevel, node_id};
use syrup_core::util::normalize_words;

/// One research run, for the record ("what did Syrup look up, and when?").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchRecord {
    pub question: String,
    pub at: String,
    pub facts_added: usize,
    pub sources: Vec<String>,
    pub ok: bool,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    pub game_id: String,
    pub nodes: BTreeMap<String, KnowledgeNode>,
    pub edges: Vec<KnowledgeEdge>,
    pub facts: Vec<Fact>,
    pub researched: Vec<ResearchRecord>,
}

/// "3.1.2" newer than "3.1"? Missing parts count as zero.
pub fn version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| s.split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

fn similar(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_words(a), normalize_words(b));
    if a == b {
        return true;
    }
    let wa: std::collections::BTreeSet<&str> = a.split(' ').collect();
    let wb: std::collections::BTreeSet<&str> = b.split(' ').collect();
    let inter = wa.intersection(&wb).count() as f32;
    let union = wa.union(&wb).count().max(1) as f32;
    inter / union > 0.85
}

impl KnowledgeGraph {
    pub fn new(game_id: &str) -> Self {
        KnowledgeGraph { game_id: game_id.into(), ..Default::default() }
    }

    /// Adds a fact unless an equivalent one is already known; returns whether it was new.
    pub fn add_fact(&mut self, mut fact: Fact) -> bool {
        if let Some(existing) = self.facts.iter_mut().find(|f| similar(&f.claim, &fact.claim)) {
            // A better-sourced or newer statement of the same claim replaces the old one.
            if fact.confidence.value() > existing.confidence.value() {
                fact.id = existing.id.clone();
                *existing = fact;
            }
            return false;
        }
        if fact.id.is_empty() {
            fact.id = format!("f{}", self.facts.len() + 1);
        }
        if let Some(subject) = &fact.subject {
            let id = node_id(subject);
            if let Some(node) = self.nodes.get_mut(&id)
                && !node.facts.contains(&fact.id)
            {
                node.facts.push(fact.id.clone());
            }
        }
        self.facts.push(fact);
        true
    }

    pub fn node(&mut self, name: &str, kind: NodeKind, provenance: &str) -> &mut KnowledgeNode {
        let id = node_id(name);
        self.nodes.entry(id.clone()).or_insert_with(|| KnowledgeNode {
            id,
            name: name.trim().to_string(),
            kind,
            aliases: Vec::new(),
            summary: None,
            facts: Vec::new(),
            provenance: provenance.into(),
        })
    }

    pub fn relate(
        &mut self,
        from: &str,
        rel: Relation,
        to: &str,
        confidence: Confidence,
        fact: Option<String>,
        provenance: &str,
    ) {
        let (from, to) = (node_id(from), node_id(to));
        if let Some(e) = self.edges.iter_mut().find(|e| e.from == from && e.rel == rel && e.to == to) {
            e.confidence = e.confidence.combine(confidence);
            return;
        }
        self.edges.push(KnowledgeEdge { from, rel, to, confidence, fact, provenance: provenance.into() });
    }

    /// Facts newer versions have overtaken are marked stale.
    pub fn mark_stale(&mut self, current_version: &str) {
        for f in self.facts.iter_mut() {
            if let Some(v) = &f.game_version
                && version_newer(current_version, v)
            {
                f.stale = true;
            }
        }
    }

    /// The facts about `topic`, best first: relevance, then how trustworthy
    /// and current they are. Stale facts rank lower; spoilers above `max_spoiler` are left out.
    pub fn about(&self, topic: &str, max_spoiler: SpoilerLevel) -> Vec<&Fact> {
        let words: Vec<String> =
            normalize_words(topic).split(' ').filter(|w| w.len() >= 3).map(|w| w.to_string()).collect();
        let id = node_id(topic);
        let mut scored: Vec<(f32, &Fact)> = self
            .facts
            .iter()
            .filter(|f| f.spoiler <= max_spoiler)
            .filter_map(|f| {
                let claim = normalize_words(&f.claim);
                let subj = f.subject.as_deref().map(node_id).is_some_and(|s| s == id);
                let hits = words.iter().filter(|w| format!(" {claim} ").contains(&format!(" {w} "))).count();
                if !subj && hits == 0 {
                    return None;
                }
                let relevance = if subj { 1.0 } else { hits as f32 / words.len().max(1) as f32 };
                let kind = match f.kind {
                    FactKind::Fact | FactKind::CurrentPatch => 1.0,
                    FactKind::CommunityConsensus => 0.8,
                    FactKind::Inference => 0.6,
                    FactKind::Speculation => 0.3,
                };
                let stale = if f.stale { 0.5 } else { 1.0 };
                Some((relevance * kind * stale * f.confidence.value(), f))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().map(|(_, f)| f).collect()
    }

    /// Everything related to a node.
    pub fn neighbours(&self, name: &str) -> Vec<&KnowledgeEdge> {
        let id = node_id(name);
        self.edges.iter().filter(|e| e.from == id || e.to == id).collect()
    }
}

#[cfg(test)]
mod tests {
    use syrup_core::knowledge::{SourceKind, SourceRef};

    use super::*;

    fn fact(claim: &str, kind: FactKind, conf: f32, version: Option<&str>) -> Fact {
        Fact {
            id: String::new(),
            claim: claim.into(),
            subject: None,
            kind,
            source: SourceRef { kind: SourceKind::Wiki, title: "wiki".into(), url: "https://example.org".into() },
            retrieved_at: "2026-09-29T00:00:00Z".into(),
            game_version: version.map(|v| v.into()),
            confidence: Confidence::new(conf),
            spoiler: SpoilerLevel::None,
            stale: false,
        }
    }

    #[test]
    fn facts_dedupe_rank_and_go_stale() {
        let mut g = KnowledgeGraph::new("sky-meadow");
        assert!(g.add_fact(fact("The Mossy King is weak to fire.", FactKind::Fact, 0.8, Some("1.2"))));
        assert!(!g.add_fact(fact("The Mossy King is weak to fire", FactKind::Fact, 0.7, None)));
        assert!(g.add_fact(fact("Some say the Mossy King fears music.", FactKind::Speculation, 0.4, None)));
        let about = g.about("Mossy King", SpoilerLevel::None);
        assert_eq!(about.len(), 2);
        assert!(about[0].claim.contains("fire"));
        g.mark_stale("1.3.0");
        assert!(g.facts[0].stale);
        assert!(version_newer("1.10", "1.9") && !version_newer("2.0", "2.0.0"));
        g.relate("Mossy King", Relation::WeakAgainst, "fire", Confidence::new(0.7), None, "wiki");
        assert_eq!(g.neighbours("fire").len(), 1);
    }
}
