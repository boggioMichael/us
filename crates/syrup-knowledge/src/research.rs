//! The research agent: from "I don't know this" to facts with provenance.
//!
//! need research → search → collect sources → rank (official and patch
//! notes, then databases, wikis, encyclopedias, community) → extract claims
//! → classify each (fact, current patch, community consensus, speculation)
//! and its spoiler level → check it against the game's current version →
//! update the graph. Sources: the game's store page and news on Steam,
//! Wikipedia, and the game's community wiki (any MediaWiki: Fandom, wiki.gg,
//! independent wikis), found from the profile or discovered by search.
//! Only the search terms (the game's name and the topic) leave the computer.

use std::sync::{Arc, OnceLock};

use regex::Regex;
use serde_json::Value;
use syrup_core::Confidence;
use syrup_core::knowledge::{Fact, FactKind, KnowledgeSource, NodeKind, Relation, SourceKind, SourceRef, SpoilerLevel};
use syrup_core::util::{normalize_words, now_iso};

use crate::fetch::{Fetcher, urlencode};
use crate::graph::{KnowledgeGraph, version_newer};

/// What to look up.
#[derive(Debug, Clone, PartialEq)]
pub struct ResearchQuestion {
    pub game_id: String,
    pub game_title: String,
    /// A thing in the game (a boss, an item, a word seen on screen); none for the game itself.
    pub topic: Option<String>,
    /// Why Syrup asks ("new game", "died here three times", "the player asked").
    pub reason: String,
    /// Sources the profile already knows.
    pub sources: Vec<KnowledgeSource>,
    /// The game's current version, when known.
    pub version: Option<String>,
}

impl ResearchQuestion {
    pub fn describe(&self) -> String {
        match &self.topic {
            Some(t) => format!("{} in {}", t, self.game_title),
            None => self.game_title.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResearchOutcome {
    pub facts: Vec<Fact>,
    pub relations: Vec<(String, Relation, String, Confidence)>,
    pub genres: Vec<String>,
    pub sources_used: Vec<SourceRef>,
    /// Sources found along the way, for the profile to remember.
    pub sources_found: Vec<KnowledgeSource>,
    /// The newest version mentioned in patch notes.
    pub latest_version: Option<String>,
    pub failures: Vec<String>,
}

const GENRES: &[(&str, &str)] = &[
    ("massively multiplayer online role-playing", "mmorpg"),
    ("massively multiplayer", "mmorpg"),
    ("mmorpg", "mmorpg"),
    ("action role-playing", "action_rpg"),
    ("action rpg", "action_rpg"),
    ("role-playing", "rpg"),
    ("rpg", "rpg"),
    ("first-person shooter", "fps"),
    ("third-person shooter", "shooter"),
    ("shooter", "shooter"),
    ("battle royale", "battle_royale"),
    ("metroidvania", "metroidvania"),
    ("roguelike", "roguelike"),
    ("roguelite", "roguelike"),
    ("deck-building", "deckbuilder"),
    ("deckbuilding", "deckbuilder"),
    ("card game", "card"),
    ("card", "card"),
    ("poker", "poker"),
    ("racing", "racing"),
    ("sports", "sports"),
    ("football", "football"),
    ("soccer", "soccer"),
    ("survival horror", "horror"),
    ("horror", "horror"),
    ("survival", "survival"),
    ("farming", "farming"),
    ("life simulation", "life_sim"),
    ("puzzle", "puzzle"),
    ("real-time strategy", "rts"),
    ("strategy", "strategy"),
    ("turn-based", "turn_based"),
    ("sandbox", "sandbox"),
    ("open world", "open_world"),
    ("open-world", "open_world"),
    ("stealth", "stealth"),
    ("fighting", "fighting"),
    ("rhythm", "rhythm"),
    ("platform game", "platformer"),
    ("platformer", "platformer"),
    ("side-scrolling", "side_scroller"),
    ("side-scroller", "side_scroller"),
    ("hack and slash", "hack_and_slash"),
    ("soulslike", "soulslike"),
    ("souls-like", "soulslike"),
    ("fantasy", "fantasy"),
    ("science fiction", "sci_fi"),
    ("space", "space"),
    ("pirate", "pirate"),
    ("mystery", "mystery"),
    ("detective", "detective"),
    ("multiplayer online battle arena", "moba"),
    ("moba", "moba"),
    ("tactical", "tactical"),
    ("adventure", "adventure"),
    ("simulation", "simulation"),
];

/// Genre tags named in a text.
pub fn genres_in(text: &str) -> Vec<String> {
    let t = text.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for (phrase, tag) in GENRES {
        if contains_word(&t, phrase) && !out.iter().any(|g| g == tag) {
            out.push(tag.to_string());
        }
    }
    out
}

fn contains_word(hay: &str, phrase: &str) -> bool {
    let mut from = 0;
    while let Some(i) = hay[from..].find(phrase) {
        let s = from + i;
        let e = s + phrase.len();
        let before = hay[..s].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = hay[e..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = e;
    }
    false
}

/// Text from HTML: scripts, styles, tables, infoboxes and references dropped, tags removed, entities decoded.
pub fn strip_html(html: &str) -> String {
    static DROP: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static SPACE: OnceLock<Regex> = OnceLock::new();
    let drop = DROP.get_or_init(|| {
        Regex::new(r#"(?is)<script.*?</script>|<style.*?</style>|<table.*?</table>|<aside.*?</aside>|<sup[^>]*class="[^"]*reference[^"]*".*?</sup>|<figure.*?</figure>|<div[^>]*class="[^"]*(?:toc|navbox|infobox)[^"]*".*?</div>"#).expect("valid pattern")
    });
    let tag = TAG.get_or_init(|| Regex::new(r"(?s)<[^>]+>").expect("valid pattern"));
    let space = SPACE.get_or_init(|| Regex::new(r"[ \t\r\f\v]+").expect("valid pattern"));
    let s = drop.replace_all(html, " ");
    let s = s.replace("</p>", "\n").replace("<br>", "\n").replace("<br/>", "\n").replace("<li>", "\n");
    let s = tag.replace_all(&s, " ");
    let s = decode_entities(&s);
    let s = space.replace_all(&s, " ");
    s.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

fn decode_entities(s: &str) -> String {
    static NUM: OnceLock<Regex> = OnceLock::new();
    let num = NUM.get_or_init(|| Regex::new(r"&#(x?)([0-9a-fA-F]+);").expect("valid pattern"));
    let s = num.replace_all(s, |c: &regex::Captures| {
        let v = if &c[1] == "x" { u32::from_str_radix(&c[2], 16) } else { c[2].parse() };
        v.ok().and_then(char::from_u32).map(|c| c.to_string()).unwrap_or_default()
    });
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

/// Sentences of a text (of a reasonable length).
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        let chars: Vec<char> = para.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            cur.push(*c);
            let end = matches!(c, '.' | '!' | '?')
                && chars.get(i + 1).is_none_or(|n| n.is_whitespace())
                && chars.get(i + 2).is_none_or(|n| n.is_uppercase() || n.is_ascii_digit() || *n == '"');
            // "e.g." and "St." are not ends.
            let abbrev = cur.ends_with("e.g.")
                || cur.ends_with("i.e.")
                || cur.ends_with(" vs.")
                || cur.ends_with(" St.")
                || cur.ends_with(" Dr.")
                || cur.ends_with(" Mr.");
            if end && !abbrev {
                let s = cur.trim().to_string();
                if (20..=320).contains(&s.chars().count()) {
                    out.push(s);
                }
                cur.clear();
            }
        }
        let s = cur.trim().to_string();
        if (20..=320).contains(&s.chars().count()) && s.ends_with(['.', '!', '?']) {
            out.push(s);
        }
    }
    out
}

/// What kind of claim a sentence makes.
pub fn classify(sentence: &str, source: SourceKind) -> FactKind {
    let s = sentence.to_lowercase();
    let speculative = [
        "might",
        "may be",
        "possibly",
        "rumor",
        "rumour",
        "speculat",
        "theor",
        "unconfirmed",
        "allegedly",
        "it is believed",
        "some believe",
        "fans think",
    ];
    let consensus = [
        "players often",
        "players usually",
        "most players",
        "players generally",
        "it is recommended",
        "recommended to",
        "the community",
        "commonly",
        "is considered",
        "widely regarded",
        "popular strategy",
        "many players",
    ];
    if speculative.iter().any(|w| s.contains(w)) {
        FactKind::Speculation
    } else if source == SourceKind::PatchNotes {
        FactKind::CurrentPatch
    } else if consensus.iter().any(|w| s.contains(w)) {
        FactKind::CommunityConsensus
    } else {
        FactKind::Fact
    }
}

/// How much a sentence gives away.
pub fn spoiler_level(sentence: &str) -> SpoilerLevel {
    let s = sentence.to_lowercase();
    let major = [
        "final boss",
        "true ending",
        "the ending",
        "ending",
        "is killed",
        "dies",
        "death of",
        "betray",
        "reveals that",
        "revealed to be",
        "twist",
        "secret ending",
        "is actually",
    ];
    let mild = ["boss", "secret", "hidden", "later in the game", "unlock", "late game", "endgame"];
    if major.iter().any(|w| s.contains(w)) {
        SpoilerLevel::Major
    } else if mild.iter().any(|w| s.contains(w)) {
        SpoilerLevel::Mild
    } else {
        SpoilerLevel::None
    }
}

fn version_of(sentence: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:v|ver\.?|version|patch|update|hotfix)\s*(\d+(?:\.\d+){1,3})\b").expect("valid pattern")
    });
    re.captures(sentence).map(|c| c[1].to_string())
}

/// Relations stated in a sentence ("X is weak to Y", "X drops Y"...).
pub fn relations_in(sentence: &str) -> Vec<(String, Relation, String)> {
    static PATTERNS: OnceLock<Vec<(Regex, Relation)>> = OnceLock::new();
    let pats = PATTERNS.get_or_init(|| {
        let p = |s: &str, r: Relation| (Regex::new(s).expect("valid pattern"), r);
        let name = r"((?:the\s+)?[A-Z][\w'\-]*(?:\s+(?:of\s+|the\s+)?[A-Z][\w'\-]*){0,4})";
        let thing = r"([\w'\-]+(?:\s+[\w'\-]+){0,3})";
        vec![
            p(
                &format!(r"{name}\s+(?:is|are)\s+(?:especially\s+|very\s+)?weak\s+(?:to|against)\s+{thing}"),
                Relation::WeakAgainst,
            ),
            p(
                &format!(r"{name}\s+(?:is|are)\s+(?:very\s+)?(?:effective|strong)\s+against\s+{thing}"),
                Relation::EffectiveAgainst,
            ),
            p(&format!(r"{name}\s+(?:can\s+)?drops?\s+{thing}"), Relation::Drops),
            p(
                &format!(r"{name}\s+(?:is|are|can be)\s+(?:found|located)\s+(?:in|at|on)\s+{thing}"),
                Relation::LocatedIn,
            ),
            p(&format!(r"{name}\s+(?:is|are)\s+(?:required|needed)\s+(?:for|to)\s+{thing}"), Relation::RequiredFor),
            p(&format!(r"{name}\s+(?:counters?|beats?)\s+{thing}"), Relation::Counters),
            p(&format!(r"{name}\s+unlocks?\s+{thing}"), Relation::Unlocks),
        ]
    });
    let mut out = Vec::new();
    for (re, rel) in pats {
        for c in re.captures_iter(sentence) {
            let a = c[1].trim().trim_start_matches("the ").trim_start_matches("The ").to_string();
            let b = c[2].trim().trim_end_matches(['.', ',']).to_string();
            if a.len() >= 3 && b.len() >= 2 {
                out.push((a, *rel, b));
            }
        }
    }
    out
}

pub struct ResearchAgent {
    fetcher: Arc<dyn Fetcher>,
}

impl ResearchAgent {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        ResearchAgent { fetcher }
    }

    fn json(&self, url: &str, out: &mut ResearchOutcome) -> Option<Value> {
        match self.fetcher.get(url) {
            Ok(body) => match serde_json::from_str::<Value>(&body) {
                Ok(v) => Some(v),
                Err(_) => {
                    out.failures.push(format!("not JSON: {url}"));
                    None
                }
            },
            Err(e) => {
                out.failures.push(e);
                None
            }
        }
    }

    fn fact(&self, claim: &str, subject: Option<&str>, source: &SourceRef, version: Option<&str>) -> Fact {
        let kind = classify(claim, source.kind);
        let base = source.kind.authority();
        let conf = base
            * match kind {
                FactKind::Fact | FactKind::CurrentPatch => 0.95,
                FactKind::CommunityConsensus => 0.8,
                FactKind::Inference => 0.6,
                FactKind::Speculation => 0.4,
            };
        let own_version = version_of(claim);
        let stale = match (version, own_version.as_deref()) {
            (Some(now), Some(v)) => version_newer(now, v),
            _ => false,
        };
        Fact {
            id: String::new(),
            claim: claim.to_string(),
            subject: subject.map(|s| s.to_string()),
            kind,
            source: source.clone(),
            retrieved_at: now_iso(),
            game_version: own_version,
            confidence: Confidence::new(if stale { conf * 0.6 } else { conf }),
            spoiler: spoiler_level(claim),
            stale,
        }
    }

    /// Looks the question up and returns what was found (it does not touch any graph).
    pub fn research(&self, q: &ResearchQuestion) -> ResearchOutcome {
        let mut out = ResearchOutcome::default();
        let title = q.game_title.trim();
        let find = |provider: &str| q.sources.iter().find(|s| s.provider == provider).map(|s| s.locator.clone());
        // Where to look: known sources, else found by search.
        let overview = q.topic.is_none();
        let wiki_api = find("mediawiki").or_else(|| self.discover_wiki(title, &mut out));
        let steam = find("steam").or_else(|| if overview { self.discover_steam(title, &mut out) } else { None });
        let wikipedia =
            find("wikipedia").or_else(|| if overview { self.discover_wikipedia(title, &mut out) } else { None });
        match &q.topic {
            None => {
                if let Some(article) = &wikipedia {
                    self.wikipedia_summary(article, q, &mut out);
                }
                if let Some(app) = &steam {
                    self.steam_details(app, q, &mut out);
                    self.steam_news(app, q, &mut out);
                }
                if let Some(api) = &wiki_api {
                    self.wiki_topic(api, title, None, q, &mut out, 3);
                }
            }
            Some(topic) => {
                if let Some(api) = &wiki_api {
                    self.wiki_topic(api, topic, Some(topic), q, &mut out, 8);
                } else {
                    out.failures.push(format!("no wiki known for {title}"));
                }
            }
        }
        // Rank: most authoritative and confident first.
        out.facts.sort_by(|a, b| b.confidence.value().total_cmp(&a.confidence.value()));
        out
    }

    fn discover_wiki(&self, title: &str, out: &mut ResearchOutcome) -> Option<String> {
        let compact: String = title.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let hyphen = syrup_core::knowledge::node_id(title);
        for slug in [compact, hyphen] {
            if slug.is_empty() {
                continue;
            }
            let api = format!("https://{slug}.fandom.com/api.php");
            let url = format!("{api}?action=query&list=search&srsearch={}&format=json&srlimit=1", urlencode(title));
            if let Ok(body) = self.fetcher.get(&url)
                && serde_json::from_str::<Value>(&body).ok().is_some_and(|v| v.pointer("/query/search").is_some())
            {
                out.sources_found.push(KnowledgeSource {
                    kind: SourceKind::Wiki,
                    provider: "mediawiki".into(),
                    locator: api.clone(),
                });
                return Some(api);
            }
        }
        out.failures.push(format!("no community wiki found for {title}"));
        None
    }

    fn discover_steam(&self, title: &str, out: &mut ResearchOutcome) -> Option<String> {
        let url = format!("https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US", urlencode(title));
        let v = self.json(&url, out)?;
        let want = normalize_words(title);
        let items = v.get("items")?.as_array()?;
        let hit = items
            .iter()
            .find(|i| i.get("name").and_then(|n| n.as_str()).map(normalize_words).is_some_and(|n| n == want))?;
        let id = hit.get("id")?.as_u64()?.to_string();
        out.sources_found.push(KnowledgeSource {
            kind: SourceKind::GameDatabase,
            provider: "steam".into(),
            locator: id.clone(),
        });
        Some(id)
    }

    fn discover_wikipedia(&self, title: &str, out: &mut ResearchOutcome) -> Option<String> {
        let url = format!(
            "https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={}&format=json&srlimit=3",
            urlencode(&format!("{title} video game"))
        );
        let v = self.json(&url, out)?;
        let want = normalize_words(title);
        let hits = v.pointer("/query/search")?.as_array()?;
        let t = hits
            .iter()
            .filter_map(|h| h.get("title").and_then(|t| t.as_str()))
            .find(|t| normalize_words(t).starts_with(&want))?;
        out.sources_found.push(KnowledgeSource {
            kind: SourceKind::Encyclopedia,
            provider: "wikipedia".into(),
            locator: t.to_string(),
        });
        Some(t.to_string())
    }

    fn wikipedia_summary(&self, article: &str, q: &ResearchQuestion, out: &mut ResearchOutcome) {
        let url =
            format!("https://en.wikipedia.org/api/rest_v1/page/summary/{}", urlencode(&article.replace(' ', "_")));
        let Some(v) = self.json(&url, out) else { return };
        let page = v.pointer("/content_urls/desktop/page").and_then(|p| p.as_str()).unwrap_or(&url).to_string();
        let source = SourceRef { kind: SourceKind::Encyclopedia, title: format!("Wikipedia: {article}"), url: page };
        let description = v.get("description").and_then(|d| d.as_str()).unwrap_or_default();
        let extract = v.get("extract").and_then(|d| d.as_str()).unwrap_or_default();
        for g in genres_in(&format!("{description} {extract}")) {
            if !out.genres.contains(&g) {
                out.genres.push(g);
            }
        }
        for s in sentences(extract).into_iter().take(3) {
            out.facts.push(self.fact(&s, Some(&q.game_title), &source, q.version.as_deref()));
        }
        out.sources_used.push(source);
    }

    fn steam_details(&self, app: &str, q: &ResearchQuestion, out: &mut ResearchOutcome) {
        let url = format!("https://store.steampowered.com/api/appdetails?appids={app}&l=english");
        let Some(v) = self.json(&url, out) else { return };
        let Some(data) = v.get(app).and_then(|a| a.get("data")) else {
            out.failures.push(format!("Steam has no details for app {app}"));
            return;
        };
        let source = SourceRef {
            kind: SourceKind::Official,
            title: "Steam store page".into(),
            url: format!("https://store.steampowered.com/app/{app}"),
        };
        if let Some(d) = data.get("short_description").and_then(|d| d.as_str()) {
            let text = strip_html(d);
            for s in sentences(&text).into_iter().take(2) {
                out.facts.push(self.fact(&s, Some(&q.game_title), &source, q.version.as_deref()));
            }
        }
        if let Some(gs) = data.get("genres").and_then(|g| g.as_array()) {
            let names: Vec<&str> = gs.iter().filter_map(|g| g.get("description").and_then(|d| d.as_str())).collect();
            for g in genres_in(&names.join(", ")) {
                if !out.genres.contains(&g) {
                    out.genres.push(g);
                }
            }
        }
        out.sources_used.push(source);
    }

    fn steam_news(&self, app: &str, q: &ResearchQuestion, out: &mut ResearchOutcome) {
        let url = format!(
            "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid={app}&count=8&maxlength=600&format=json"
        );
        let Some(v) = self.json(&url, out) else { return };
        let Some(items) = v.pointer("/appnews/newsitems").and_then(|i| i.as_array()) else { return };
        for item in items {
            let title = item.get("title").and_then(|t| t.as_str()).unwrap_or_default();
            let lower = title.to_lowercase();
            if !["patch", "update", "hotfix", "balance", "changes"].iter().any(|w| lower.contains(w)) {
                continue;
            }
            let link = item.get("url").and_then(|u| u.as_str()).unwrap_or_default().to_string();
            let date =
                item.get("date").and_then(|d| d.as_u64()).map(syrup_core::util::iso_from_unix).unwrap_or_default();
            let source = SourceRef {
                kind: SourceKind::PatchNotes,
                title: format!("{title} ({})", date.get(0..10).unwrap_or("")),
                url: link,
            };
            if let Some(ver) = version_of(title)
                && out.latest_version.as_deref().is_none_or(|v| version_newer(&ver, v))
            {
                out.latest_version = Some(ver);
            }
            let body = strip_html(item.get("contents").and_then(|c| c.as_str()).unwrap_or_default());
            let mut claims: Vec<String> = sentences(&body).into_iter().take(2).collect();
            if claims.is_empty() {
                claims.push(format!("{title}."));
            }
            for c in claims {
                let mut f = self.fact(&c, Some(&q.game_title), &source, q.version.as_deref());
                if f.game_version.is_none() {
                    f.game_version = version_of(title);
                }
                out.facts.push(f);
            }
            out.sources_used.push(source);
        }
    }

    fn wiki_topic(
        &self,
        api: &str,
        query: &str,
        topic: Option<&str>,
        q: &ResearchQuestion,
        out: &mut ResearchOutcome,
        max_facts: usize,
    ) {
        let url = format!("{api}?action=query&list=search&srsearch={}&format=json&srlimit=2", urlencode(query));
        let Some(v) = self.json(&url, out) else { return };
        let pages: Vec<String> = v
            .pointer("/query/search")
            .and_then(|s| s.as_array())
            .map(|a| a.iter().filter_map(|h| h.get("title").and_then(|t| t.as_str()).map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if pages.is_empty() {
            out.failures.push(format!("the wiki has nothing on {query}"));
            return;
        }
        let words: Vec<String> =
            normalize_words(query).split(' ').filter(|w| w.len() >= 3).map(|w| w.to_string()).collect();
        let base = api.trim_end_matches("api.php").trim_end_matches('/');
        let wiki_base = if base.ends_with("/w") || base.ends_with("/mediawiki") {
            base.rsplit_once('/').map(|x| x.0).unwrap_or(base).to_string()
        } else {
            base.to_string()
        };
        let mut added = 0;
        for page in pages.iter().take(if topic.is_some() { 2 } else { 1 }) {
            let url = format!(
                "{api}?action=parse&page={}&prop=text&format=json&formatversion=2&redirects=1",
                urlencode(page)
            );
            let Some(v) = self.json(&url, out) else { continue };
            let html = v
                .pointer("/parse/text")
                .and_then(|t| {
                    t.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| t.get("*").and_then(|s| s.as_str()).map(|s| s.to_string()))
                })
                .unwrap_or_default();
            if html.is_empty() {
                continue;
            }
            let text = strip_html(&html);
            let text: String = text.chars().take(20_000).collect();
            let source = SourceRef {
                kind: SourceKind::Wiki,
                title: format!("{} wiki: {page}", q.game_title),
                url: format!("{wiki_base}/wiki/{}", page.replace(' ', "_")),
            };
            let strategy = [
                "weak",
                "avoid",
                "dodge",
                "use ",
                "recommended",
                "strategy",
                "tip",
                "attack",
                "phase",
                "resist",
                "immune",
                "drops",
                "heal",
                "block",
                "parry",
                "stagger",
                "cooldown",
                "damage",
                "fight",
                "players",
                "potion",
                "found in",
            ];
            // On the topic's own page every sentence is about it ("it", "its second phase").
            let own_page = normalize_words(page) == normalize_words(query);
            let mut picked = Vec::new();
            for s in sentences(&text) {
                let n = normalize_words(&s);
                let about =
                    own_page || words.is_empty() || words.iter().any(|w| format!(" {n} ").contains(&format!(" {w} ")));
                let useful = strategy.iter().any(|k| s.to_lowercase().contains(k));
                if (topic.is_none() && picked.len() < 2) || (about && useful) || (about && picked.is_empty()) {
                    picked.push(s);
                }
                if picked.len() >= max_facts {
                    break;
                }
            }
            for s in picked {
                for (a, rel, b) in relations_in(&s) {
                    out.relations.push((a, rel, b, Confidence::new(SourceKind::Wiki.authority() * 0.8)));
                }
                out.facts.push(self.fact(&s, topic.or(Some(page.as_str())), &source, q.version.as_deref()));
                added += 1;
            }
            out.sources_used.push(source);
        }
        if added == 0 {
            out.failures.push(format!("nothing useful about {query} on the wiki"));
        }
    }
}

/// Adds an outcome to a game's graph; returns how many facts were new.
pub fn apply(graph: &mut KnowledgeGraph, q: &ResearchQuestion, outcome: &ResearchOutcome) -> usize {
    if let Some(topic) = &q.topic {
        let kind = if topic.to_lowercase().contains("king") || topic.to_lowercase().contains("boss") {
            NodeKind::Boss
        } else {
            NodeKind::Concept
        };
        graph.node(topic, kind, "research");
    }
    let mut added = 0;
    for f in &outcome.facts {
        if graph.add_fact(f.clone()) {
            added += 1;
        }
    }
    for (a, rel, b, conf) in &outcome.relations {
        graph.node(a, NodeKind::Concept, "research");
        graph.node(b, NodeKind::Concept, "research");
        graph.relate(a, *rel, b, *conf, None, "research");
    }
    if let Some(v) = &q.version {
        graph.mark_stale(v);
    }
    graph.researched.push(crate::graph::ResearchRecord {
        question: q.describe(),
        at: now_iso(),
        facts_added: added,
        sources: outcome.sources_used.iter().map(|s| s.url.clone()).collect(),
        ok: !outcome.facts.is_empty(),
        note: outcome.failures.join("; "),
    });
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_tools() {
        assert_eq!(genres_in("2017 Metroidvania action-adventure game"), vec!["metroidvania", "adventure"]);
        assert_eq!(genres_in("a massively multiplayer online role-playing game"), vec!["mmorpg", "rpg"]);
        let html = r#"<p>The <b>Mossy King</b> is weak to fire.<sup class="reference">[1]</sup> Players often fight it on the left ledge.</p><table><tr><td>HP</td></tr></table><aside>box</aside>"#;
        let text = strip_html(html);
        assert!(!text.contains("[1]") && !text.contains("box") && !text.contains("HP"), "{text}");
        let s = sentences(&text);
        assert_eq!(s.len(), 2, "{s:?}");
        assert_eq!(classify(&s[1], SourceKind::Wiki), FactKind::CommunityConsensus);
        assert_eq!(classify("It might have a second phase.", SourceKind::Wiki), FactKind::Speculation);
        assert_eq!(spoiler_level("The final boss is revealed to be your brother."), SpoilerLevel::Major);
        let r = relations_in(&s[0]);
        assert_eq!(r, vec![("Mossy King".to_string(), Relation::WeakAgainst, "fire".to_string())]);
        assert_eq!(version_of("Patch 1.4.2 notes"), Some("1.4.2".into()));
        assert_eq!(decode_entities("Tom &amp; Jerry &#8211; &#x41;"), "Tom & Jerry – A");
    }
}
