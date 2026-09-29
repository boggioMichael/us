# Writing a plugin

Syrup needs no plugin to play along with a game: everything it knows about a
game it can learn from watching and from research. A plugin is a head start
or a sharper eye for one game: where its interface is, what its screens say,
what is worth knowing, which hat Syrup wears, and advice the universal rules
cannot give. The universal system runs the same with no plugins at all.

There are two kinds: a **data plugin** (a `plugin.json`, no code) and a
**Rust plugin** (the `GamePlugin` trait). MapleStory is both: its data is
`plugins/maplestory/plugin.json`, and `syrup-plugins/src/maplestory.rs` adds
what data cannot do (reading its death counter and boss timer).

## A data plugin

Put a folder with a `plugin.json` under a plugins folder and pass it:
`syrup live --plugins path/to/plugins`. Every field but `id` and `name` is
optional.

```json
{
  "id": "sky-meadow",
  "name": "Sky Meadow Online",
  "about": "What the plugin knows, and where it comes from.",
  "games": ["unknown-e77d1d5f", "sky-meadow-online"],
  "hat": "wizard_hat",
  "genres": ["mmorpg", "side_scroller"],
  "sources": [
    { "kind": "wiki", "provider": "mediawiki", "locator": "https://skymeadowonline.fandom.com/api.php" }
  ],
  "elements": [
    { "concept": "health", "kind": "bar", "norm": [0.19, 0.91, 0.25, 0.035], "confidence": 0.6 },
    { "concept": "boss_health", "kind": "bar", "norm": [0.29, 0.10, 0.42, 0.035] }
  ],
  "scene_words": {
    "defeat": ["return to the nearest town"],
    "loading": ["connecting to the meadow"]
  },
  "entities": [
    { "name": "Mossy King", "kind": "boss", "summary": "A moss-covered golem guarding Mossy Hills." }
  ],
  "facts": [
    { "subject": "Mossy King", "claim": "Stepping back when he raises both arms avoids the slam.", "kind": "community_consensus" }
  ],
  "tips": [
    { "when_text": ["mossy king"], "text": "Boss. Watch the arms.", "urgency": "important" }
  ]
}
```

| field | what Syrup does with it |
|---|---|
| `games` | the game ids the plugin is for (as `syrup profiles` shows them: a catalog id, or `unknown-…` for an uncatalogued game); the plugin's own `id` also counts |
| `hat` | Syrup's hat in this game: `syrup_cap`, `wizard_hat`, `knight_helmet`, `ranger_hood`, `racing_helmet`, `tactical_helmet`, `astronaut_helmet`, `pirate_hat`, `straw_hat`, `detective_cap`, `dealer_visor`, `sports_cap`, `lantern_hat` |
| `genres` | added to the game's profile |
| `sources` | where research looks: `provider` is `mediawiki` (a wiki's `api.php`), `wikipedia` (an article title) or `steam` (an app id) |
| `elements` | interface the game is known to have. `norm` is x, y, w, h in fractions of the game's picture (the window's inside, not the desktop); `kind` is `bar` or `text`. An element perception finds in that place (intersection over union above 0.4) starts out as that concept, with that confidence; evidence can still overturn it, and a player's correction always does. (`hue`, a fill's hue range in degrees, may be given for reference; the engine does not use it yet.) |
| `scene_words` | words that mean a screen in this game: keys `gameplay`, `menu`, `loading`, `dialogue`, `cutscene`, `defeat`, `victory` |
| `entities` | knowledge graph nodes: kinds as in [memory.md](../protocols/memory.md#knowledgejson-what-is-known) |
| `facts` | seed facts, attributed to the plugin; `kind` defaults to `fact`, `spoiler` to `none` |
| `tips` | said when any of `when_text` is on screen (whole words), at most once a minute each, through the same interruption policy as everything else |

Facts are the plugin author's own short summaries, with the plugin as their
source, never text copied from elsewhere.

## A Rust plugin

```rust
use syrup_plugins::{GamePlugin, PluginContext};

struct MyGame;

impl GamePlugin for MyGame {
    fn id(&self) -> &str { "my-game" }
    fn name(&self) -> &str { "My Game" }

    // Everything else is optional. For example, read something the universal
    // detectors cannot:
    fn extract_state(&mut self, obs: &syrup_core::Observation, state: &mut syrup_core::GameState) {
        // look at obs.text, obs.ui_regions… and insert or confirm concepts in state.concepts
    }
}
```

Register it with `PluginManager::add(Box::new(MyGame))` (the runtime's
built-ins are in `PluginManager::with_builtins`). The hooks, each with a
default that does nothing:

| hook | for |
|---|---|
| `detect(cues, identity)` | whether this is the plugin's game (default: the identity's id is the plugin's id) |
| `parse_observation(frame, obs)` | add to what perception saw: a game-specific reading on the frame |
| `extract_state(obs, state)` | add or confirm concepts (MapleStory adds `deaths_left` and `boss_timer`) |
| `known_regions()` | the elements above, as `ElementHint`s |
| `known_entities()`, `seed_facts()`, `knowledge_sources()` | knowledge to start with |
| `visual_theme()` | the hat (and an accent colour) |
| `scene_words()` | words for screens |
| `genres()` | genres for the profile |
| `advice(ctx)` | lines to propose; they go through the same interruption and spoiler policies, and the player's feedback applies to them like to any other |

A plugin sees what everything else sees (the observation, the state, the
profile). It gets no other access to the game, and must not want any: no
memory reading, no input, nothing Syrup's boundaries rule out.
