# Syrup Universal: architecture

Syrup watches a game the player is playing, works out what the screen shows,
learns the game (from what it sees and from what it reads online), learns the
player, and coaches. It knows nothing about a game in advance: everything it
knows about a particular game is learned into that game's profile, or brought
by an optional plugin.

Every decision below was made in this order of priority:
**universality > modularity > correctness > debuggability > realtime
performance > visual polish.** The test applied at every step: *would this
still work if tomorrow the player launched a game nobody has ever seen?*

## Boundaries

Syrup sees what the player sees and says things. Nothing else.

- Input: screen pixels of the game window (and, later, the audio the player hears).
- Output: short text and speech, marks drawn in its own window.
- Never: reading or writing game memory, injecting code, hooking the
  renderer, touching network traffic, private game APIs, or sending input to
  the game. There is no input automation anywhere in the codebase; the
  Windows features it links are listed in each crate's `Cargo.toml`, and
  none of them sends input.

## The pipeline

```
 CaptureService ──FrameCaptured──▶ FrameSampler ──(sampled frame)──▶ SceneAnalyzer
   (window/screen,                  (change probe,                    ├─ SceneChangeDetector
    video, synthetic)                adaptive budget)                  ├─ MotionDetector (+ tracker)
                                                                       ├─ StabilityMap → UiRegionDetector
                                                                       ├─ BarDetector
                                                                       ├─ TextDetector + OCR backend
                                                                       ├─ IconDetector (recurring elements)
                                                                       ├─ MinimapDetector
                                                                       ├─ DialogueDetector / MenuDetector
                                                                       ├─ CharacterDetector
                                                                       └─ GenericVisionModel (optional)
                                                                                │
                                                                         Observation (IR)
                                                                                │
                      ┌─────────────────────────────────────────────────────────┤
                      ▼                                                         ▼
               GameRecognizer ─GameIdentified─▶ ProfileStore          TemporalStateTracker
               (title, exe, OCR,               (load/create,           (tracks, trends,
                HUD signature,                  learn, persist)         transitions)
                confirmations)                        │                         │
                                                      ▼                         ▼
                                   PluginManager ──▶ GameStateEngine ◀──────────┘
                                   (optional game     (concepts: what each
                                    plugins)           element probably is)
                                                      │
                     ┌────────────────────────────────┼─────────────────────────┐
                     ▼                                ▼                         ▼
               PlayerModel                      CoachEngine ◀── GameKnowledgeEngine
               (skills with                   (rules, plugins,       ▲   (graph, facts
                confidence)                    InterruptionPolicy,    │    with provenance)
                     │                         SpoilerPolicy)         │
                     ▼                                │         ResearchAgent (async:
               MemoryStore ◀──────────────────────────┤         search, rank, extract,
               (session, player,                      ▼         validate versions)
                game, episodes)               OverlayUI + VoiceEngine
                                              (Syrup + hat, bubble,
                                               feedback) ──FeedbackReceived──▶ Coach, Profile, PlayerModel
```

(The diagram names the design's parts. In the MVP the detectors live in
`syrup-perception` as a few modules rather than one type each, and the
optional generic vision model is a slot for later: nothing in the MVP needs a
model.)

Everything that happens is also an `Event` on the `EventBus`
(`FrameCaptured`, `SceneChanged`, `GameIdentified`, `UiElementDiscovered`,
`StateChanged`, `PlayerDied`, `ObjectiveChanged`, `ResearchRequested`,
`KnowledgeUpdated`, `AdviceGenerated`, `AdviceShown`, `AdviceSuppressed`,
`FeedbackReceived`, `ProfileUpdated`, `PluginActivated`, `Note`; see
[the protocol](../protocols/events.md)). The bus keeps a numbered
ring of recent events; the devtools page and the session timeline read from
it, and nothing downstream has to know which module produced an event.

The realtime loop is one thread: capture, sample, analyse, track, decide,
draw. Research runs on a worker thread and comes back as events; live, OCR
does too. The loop's latency never depends on the network or a slow engine.

## The crates

| crate | responsibility |
|---|---|
| `syrup-core` | the shared vocabulary: geometry (`Rect`, `NormRect`), `Confidence`, `Detection<T>`, the observation IR, game-state and profile types, advice and feedback types, events and the event bus, data paths |
| `syrup-capture` | `FrameSource`: a game window or the screen on Windows, a video (through ffmpeg), a folder of images, a synthetic game; window enumeration and picking the game window; cutting a recording's taskbar off |
| `syrup-perception` | `FrameSampler` and `SceneAnalyzer`: stability, bars, text (Windows OCR, Tesseract, or none), regions, motion, scenes; finding the taskbar in screen recordings |
| `syrup-state` | `StateEngine`: per-element histories, trends and transitions, and which element is probably which concept |
| `syrup-recognition` | `Recognizer` and the built-in catalog of known games |
| `syrup-knowledge` | the `KnowledgeGraph` (facts with provenance and versions), the `ResearchAgent` and its worker thread |
| `syrup-memory` | `MemoryStore`: profiles (game memory), session timelines, episodes, player memory, and deletion; `ProfileLearner` |
| `syrup-player` | `PlayerModel` and `PlayerTracker`: skill estimates with confidence, habits, repeated mistakes |
| `syrup-coach` | `CoachEngine`: advice rules, `InterruptionPolicy`, `SpoilerPolicy`, learning from feedback, Syrup's voice (short lines) |
| `syrup-paint` | a small software painter (antialiased shapes, embedded DejaVu fonts) |
| `syrup-avatar` | Syrup composed from layers (face, hat, medallion, expression prop), blinking and speaking |
| `syrup-ui` | the overlay (a portable painter, and on Windows a click-through window with feedback buttons), and the voice (the system's speech on Windows) |
| `syrup-plugins` | `GamePlugin`, `PluginManager`, and the built-in plugins (`maplestory`) |
| `syrup-runtime` | everything wired together: `Runtime::on_frame`, the frame loop (`live::run`), the devtools server |
| `syrup-testgames` | three synthetic games with exact ground truth, and the scorer that measures Syrup against it |
| `apps/desktop` | the `syrup` program (`live`, `replay`, `simulate`, `research`, `profiles`, `forget`, `avatar`, `windows`) and `syrup-testgame` |
| `apps/devtools` | the developer page (TypeScript), served by `syrup` on `localhost` |

## The universal intermediate representation

Perception never says "HP". It says what it saw:

```json
{
  "frame": 812, "timestamp_ms": 40600, "frame_size": [1280, 720],
  "scene": { "kind": "gameplay", "confidence": 0.8 },
  "objects":   [{ "id": 17, "rect": [612, 380, 44, 60], "velocity": [3.5, 0], "age": 42, "confidence": 0.7 }],
  "text":      [{ "text": "HP 87/100", "rect": [30, 650, 120, 18], "region": 3, "confidence": 0.9 }],
  "ui_regions":[{ "id": 3, "rect": [24, 660, 220, 16], "kind": { "bar": { "fill": 0.87, "color": [210, 40, 40] } }, "stability": 0.96 }],
  "characters":[{ "object": 17, "role": "player_candidate", "confidence": 0.5 }],
  "events":    [{ "kind": "scene_changed", "from": "menu", "to": "gameplay" }],
  "relationships": [{ "from": "text:0", "rel": "labels", "to": "region:3" }],
  "uncertainties": [{ "about": "ocr", "reason": "no OCR engine on this machine" }]
}
```

`GameStateEngine` then maps elements to *concepts*, each with evidence and
a confidence:

```json
{ "concepts": { "health": { "value": 0.87, "source": "region:3", "confidence": 0.93, "trend": "falling",
                             "evidence": ["red bar", "labelled 'HP'", "fell during combat", "empty before the death screen"] } } }
```

No concept is assumed. A game without health has no `health`; a game whose
only number is a score gets a `score`. A plugin can add concepts the generic
engine cannot find, and can confirm or rename them.

## How concepts are discovered

1. **What stays still is interface.** A stability map over a coarse grid
   records, for every cell, how often it stayed the same while the rest of
   the frame changed. Stable, detailed cells form the HUD layer; their
   connected groups are UI regions.
2. **What each element looks like.** A long thin run of one saturated colour
   inside a stable container is a bar, and its fill is measured. A roughly
   square stable region near a corner whose inside keeps changing a little
   is a minimap. Rows of small aligned shapes are text; OCR reads them when
   they change. Small square patterns that recur become the game's visual
   vocabulary.
3. **How it behaves.** A red bar that falls while the screen is busy and is
   empty when a death screen appears is health. A number that goes up by one
   each second is a timer. A number that jumps after things are defeated is
   score or currency. Labels next to elements ("HP", "Mana", "Gold") are
   strong evidence.
4. **What the player confirms.** A correction ("that's stamina, not health")
   is stored in the profile and outranks every inference.

## Game identity and profiles

`GameRecognizer` combines independent signals with a noisy-OR: the window
title and executable against a catalog and against learned profiles, text
read on screen (title screens and menus say the game's name), the HUD
signature (appearance hashes of the stable UI elements at their normalised
positions) against every stored profile, the Steam folder name when the
executable lives in one, and the player's confirmation. It returns the best
candidate with its confidence and every piece of evidence. Below the
threshold the answer is "not sure yet": Syrup keeps watching and says so,
rather than naming a game it is guessing. An unrecognised game still gets a
profile, keyed by its executable, and can be named later.

A `GameProfile` is what Syrup has learned about one game: identity, version,
genres, known UI elements (normalised position, kind, concept, appearance
hash, confidence, whether the player corrected it), visual vocabulary,
mechanics, entities, resources, objectives, player actions, common states,
progression systems, knowledge sources, terminology (words read on screen and
how often), strategy knowledge, the HUD signature, and its visual identity
(Syrup's hat). Profiles are updated as Syrup watches and written to disk
periodically, so the next session starts where the last one stopped. A
plugin only seeds or sharpens a profile; no game needs one.

## Knowledge

Every fact has provenance: the claim, its source (URL and kind), when it
was retrieved, the game version it applies to, a confidence, and a kind:
**fact**, **current patch**, **community consensus**, **speculation**, or
**Syrup's own inference**. Facts also carry a spoiler level. The knowledge
graph links nodes (item, character, enemy, boss, quest, skill, map, resource,
weapon, mechanic, strategy, build, faction, location, objective) with
relations (`effective_against`, `required_for`, `synergizes_with`,
`located_in`, `weak_against`, ...), each edge with its provenance.

The `ResearchAgent` runs when Syrup notices it does not know something (a
new game, an unknown word that keeps appearing, a place where the player
keeps failing, the player pressing *Research*): need research? → search →
collect sources → rank (official > wiki > community > forum; recency;
relevance) → extract facts → validate against the game's current version
(advice from an older patch is flagged) → update the graph. Sources are
Wikipedia, the game's Fandom wiki, Steam (store data and patch news), and any
sources a profile or plugin names. Fetching goes through one `Fetcher`
interface: `curl` in production, recorded responses in tests.

## Memory

| layer | holds | lives in |
|---|---|---|
| session | what happened this session: transitions, advice, feedback | `sessions/<id>/timeline.jsonl` |
| episodic | moments worth remembering: a death, six failures then a success, a first win | `games/<game>/episodes.json` |
| game | the profile and the knowledge graph | `games/<game>/profile.json`, `knowledge.json` |
| player | the player model, per game and overall | `players/<player>/model.json` |
| recordings | frames, only when recording is switched on | `recordings/` |

Screenshots are never mixed with learned metadata, nothing leaves the
computer unless research is on (and then only the search terms), and
`syrup forget` deletes a game's memory, the player's, or everything.

## The player model

Skills are estimates with evidence counts, not scores: each dimension
(survival, resource management, recovery, persistence, exploration,
consistency, and any a plugin adds) is a Beta distribution updated by what
happens (a low-health moment survived is a success for resource management;
a death at full health in two seconds is a failure for survival). The model
reports the mean *and* how sure it is, and says "not enough seen yet" rather
than a number when the evidence is thin. It also keeps habits, repeated
mistakes and the causes of failures.

## Coaching

Most observations produce no advice. The `CoachEngine` turns state,
transitions, knowledge, the player model and plugin rules into candidates
with a kind (warning, tactical, strategic, build, economy, route, objective
reminder, mechanical correction, learning suggestion, post-game insight), an
urgency (critical, important, opportunistic, educational), a confidence, a
reason ("why?") and a spoiler level. The `InterruptionPolicy` decides what is
worth interrupting for: critical things go through, everything else must be
valuable enough given how busy the player is right now (combat intensity),
how recently Syrup spoke, how the player rated this kind of advice, and
whether the topic was muted. The `SpoilerPolicy` (none, hints only, normal,
full information) filters or softens knowledge-based advice.

Feedback on every piece of advice is one click: useful, wrong, explain,
stop suggesting this. It changes the weight of that kind of advice for this
player and game, mutes topics, corrects profiles, and is kept in memory.

Syrup talks in short sentences. "Wait. That attack repeats every 4 seconds."
Not "Based on my comprehensive analysis".

## Syrup

The companion is the Maplesyrup dog. `SyrupAvatar` composes it from layers:
the head (from Maplesyrup's art), the hat, the **S** medallion (always
there, whatever the hat), and an expression prop (thinking, excited,
warning, confused, proud, researching, surprised). The hat comes from the
game's profile (`visual_identity.hat`), else from the catalog, else from the
game's genres (fantasy RPG: wizard hat; racing: racing helmet; shooter:
tactical helmet; space: astronaut helmet; pirates: pirate hat; farming:
straw hat; mystery: detective cap; cards: dealer visor; sports: sports cap;
horror: lantern hat); MapleStory keeps the original syrup cap. Movement is
rare and small; a reduced-motion setting turns it off.

The overlay has five modes: hidden, minimal (Syrup's head and one short
line), normal (the card with the line and *Why? / Ignore / Research*, and the
feedback row), analysis (normal plus what perception sees), and post-game (the
session's summary). On Windows it is a layered window: see-through, clicks
pass to the game everywhere except on the card, it never takes the focus, and
it keeps itself out of screen captures so Syrup never reads its own card.

## Plugins

```rust
trait GamePlugin: Send {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn detect(&self, cues: &IdentityCues, identity: &GameIdentity) -> Option<Confidence>;
    fn parse_observation(&mut self, frame: &Frame, obs: &mut Observation);
    fn extract_state(&mut self, obs: &Observation, state: &mut GameState);
    fn known_regions(&self) -> Vec<ElementHint>;
    fn known_entities(&self) -> Vec<KnowledgeNode>;
    fn seed_facts(&self) -> Vec<Fact>;
    fn knowledge_sources(&self) -> Vec<KnowledgeSource>;
    fn visual_theme(&self) -> Option<VisualIdentity>;
    fn scene_words(&self) -> Vec<(SceneKind, String)>;
    fn genres(&self) -> Vec<String>;
    fn advice(&mut self, ctx: &PluginContext) -> Vec<Advice>;
}
```

Every hook but `id` and `name` has a default that does nothing. Most plugins
need no code at all: a `plugin.json` is a complete plugin (see
[writing a plugin](../plugins/writing-a-plugin.md)). The universal system
runs the same with no plugins at all.

## Performance

- The capture loop never waits for analysis: a cheap change probe (a 64×36
  luminance thumbnail) runs on every frame; full analysis runs at 2 Hz on a
  quiet screen and up to 10 Hz when the screen is busy, and backs off when
  the last analysis ran over its budget.
- OCR reads a region only when its pixels changed, and at most a few regions
  a second; large reads run off the loop.
- Detectors work on a downscaled frame where resolution does not matter
  (motion, stability, scene change) and at full resolution only inside the
  regions that need it (bars, text).
- Research and reasoning are asynchronous.

## Observability

`syrup live --devtools` (and `replay`, `simulate`) serves a page on
`http://127.0.0.1:7777` ([its API](../protocols/devtools-api.md)): the current game and why Syrup thinks so, the frame
with every detected region, object and text box drawn on it, concepts with
their evidence, active hypotheses, the player model, knowledge queries, recent
events, the advice queue with what was suppressed and why, stage latencies and
FPS. `syrup replay --explain` prints the same for recorded frames.

## Languages

Rust for everything on the realtime path, Windows integration included,
because capture, perception, the overlay and the voice have to share one
process and one frame budget. TypeScript for the devtools page. Python only
for offline tools (generating the hat artwork). One language on the hot path
means no serialisation between capture and decisions and one build.

## How it is tested

- **Synthetic games with exact ground truth** (`syrup-testgames`): a
  first-person 3D dungeon (a raycaster with a health bar, ammo, gold and a
  minimap), a 2D side-scroller in the MMO style (HP/MP/EXP bars, level, a
  minimap panel, chat, mobs, a death dialog), and a card game (menu, table,
  score, turn timer, victory and defeat screens). Each renders deterministic
  sessions with the true regions, values, scenes and events, so perception,
  tracking, recognition, profiles and coaching are measured, not eyeballed.
- **Real footage, kept out of the repository**: recordings and screenshots
  of real games are evaluated locally and the numbers written down; the
  games' art is not committed.
- **Windows, live**: CI shows a synthetic game in a real window on a Windows
  runner while `syrup live` watches it.
