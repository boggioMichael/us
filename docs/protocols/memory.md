# What Syrup keeps on disk

Everything Syrup learns is plain JSON in its data folder:
`%APPDATA%\SyrupUniversal` on Windows, `~/Library/Application Support/SyrupUniversal`
on macOS, `$XDG_DATA_HOME/syrup-universal` or `~/.local/share/syrup-universal`
elsewhere; `SYRUP_DATA_DIR` or `--data-dir` choose another.

```
games/<game>/profile.json     what Syrup learned about the game
games/<game>/knowledge.json   facts with their sources, and the knowledge graph
games/<game>/episodes.json    moments worth remembering
players/<player>/model.json   the player model
sessions/<session>/timeline.jsonl   every event of one session
sessions/<session>/summary.json     how the session went
recordings/<session>/<ms>ms.png     frames, only with --record
cache/research/                     fetched pages (so research is not repeated)
```

Files are written whole and atomically (a temporary file, then a rename), so
a crash never leaves half a profile. `syrup forget` deletes a game, the
player, the recordings, the sessions, or everything. Learned metadata and
pictures never mix: frames exist only under `recordings/`, and only when
asked for.

## `profile.json`: a game

```json
{
  "schema": 1,
  "game_id": "unknown-e77d1d5f",
  "title": "Sky Meadow Online",
  "aliases": [],
  "executables": ["skymeadow.exe"],
  "window_titles": ["Sky Meadow Online"],
  "current_version": "1.3.1",
  "genres": ["mmorpg", "rpg", "side_scroller"],
  "known_ui_elements": [
    { "id": 9, "norm": {"x": 0.19, "y": 0.919, "w": 0.246, "h": 0.026}, "kind": "bar", "concept": "health",
      "appearance": 11329200203873757665, "seen": 129, "confidence": 0.95, "corrected": false, "origin": "observed" }
  ],
  "visual_vocabulary": [],
  "mechanics": [], "entities": [], "resources": [], "objectives": [], "player_actions": [],
  "common_states": [], "progression_systems": [], "strategy_knowledge": [],
  "knowledge_sources": [
    { "kind": "wiki", "provider": "mediawiki", "locator": "https://skymeadowonline.fandom.com/api.php" }
  ],
  "terminology": { "quest": { "count": 12, "last_seen_ms": 74500 } },
  "visual_identity": { "hat": "wizard_hat", "accent": null },
  "hud_signature": [
    { "norm": {"x": 0.0, "y": 0.0, "w": 0.25, "h": 0.244}, "appearance": 9274179234379629696, "kind": "minimap" }
  ],
  "corrections": [],
  "stats": { "sessions": 1, "observed_ms": 74500, "analysed_frames": 150,
             "first_seen": "2026-09-29T02:17:11Z", "last_seen": "2026-09-29T02:18:10Z" }
}
```

- `known_ui_elements`: the interface as Syrup learned it: where (in fractions
  of the frame, so a new resolution does not matter), what kind, what it is
  believed to be, how often it was seen, how sure Syrup is, and whether the
  player named it (`corrected`, which outranks inference). `origin` is
  `observed`, `player` or `plugin:<id>`.
- `hud_signature`: the most stable elements' places and looks: how Syrup
  recognises the game from its interface alone next time.
- `terminology`: words read on screen and how often (the game's vocabulary).
- `knowledge_sources`: where to look the game up (found by research, or given
  by the catalog or a plugin).
- `visual_identity.hat`: Syrup's hat for this game.
- `corrections`: what the player said, and when.

## `knowledge.json`: what is known

```json
{
  "game_id": "unknown-e77d1d5f",
  "nodes": { "mossy-king": { "id": "mossy-king", "name": "Mossy King", "kind": "boss", "aliases": [],
                             "summary": "…", "facts": ["f12"], "provenance": "…" } },
  "edges": [ { "from": "mossy-king", "rel": "weak_against", "to": "fire", "confidence": 0.7,
               "fact": "f13", "provenance": "…" } ],
  "facts": [
    { "id": "f2", "claim": "The Mossy King now attacks every 4 seconds instead of 3.", "subject": "Mossy King",
      "kind": "current_patch",
      "source": { "kind": "patch_notes", "title": "Patch 1.3.1", "url": "https://store.steampowered.com/news/app/999001/view/3" },
      "retrieved_at": "2026-09-29T02:17:11Z", "game_version": "1.3.1", "confidence": 0.9,
      "spoiler": "none", "stale": false }
  ],
  "researched": [
    { "question": "Sky Meadow Online", "at": "2026-09-29T02:17:11Z", "facts_added": 10,
      "sources": ["https://en.wikipedia.org/wiki/Sky_Meadow_Online", "…"], "ok": true, "note": "" }
  ]
}
```

A fact's `kind` is `fact`, `current_patch`, `community_consensus`,
`speculation` or `inference` (Syrup's own conclusion); its source's kind is
`official`, `patch_notes`, `game_database`, `wiki`, `encyclopedia`, `guide`,
`community`, `forum`, `video`, `observation`, `plugin` or `player`. A fact
about an older version than the one being played is marked `stale` and
ranked below current ones; `spoiler` is `none`, `mild` or `major`. Node kinds:
`game`, `item`, `character`, `enemy`, `boss`, `quest`, `skill`, `map`,
`resource`, `weapon`, `mechanic`, `strategy`, `build`, `faction`, `location`,
`objective`, `concept`. Relations: `effective_against`, `weak_against`,
`required_for`, `synergizes_with`, `located_in`, `drops`, `part_of`,
`counters`, `unlocks`, `related_to`.

## `episodes.json`: moments

```json
[ { "at": "2026-09-29T02:18:24Z", "session": "20260929-021710", "kind": "death",
    "summary": "died during a boss fight", "details": {} } ]
```

Kinds: `death`, `victory`, `level_up`, `repeated_failure`.

## `model.json`: the player

```json
{
  "player_id": "default",
  "games": {
    "unknown-e77d1d5f": {
      "skills": { "survival": { "alpha": 1.0, "beta": 2.0 }, "resource_management": { "alpha": 2.0, "beta": 2.0 } },
      "habits": { "fights at low health": 3 },
      "mistakes": [],
      "deaths": 1, "wins": 0, "level_ups": 1, "sessions": 1, "play_ms": 74500,
      "advice_weights": { "kind:Tactical": 1.15, "topic:low:health": 1.2 },
      "muted_topics": ["timer"]
    }
  },
  "overall": { … }
}
```

A skill is a Beta distribution: its mean is the estimate, and the amount of
evidence (`alpha + beta - 2`) says how sure Syrup is. Below a few
observations the devtools page says "not enough seen yet" instead of a
number. `advice_weights` is what feedback taught: how much each kind of
advice, and each topic, is worth to this player in this game.

## A session

`timeline.jsonl` is every [event](events.md) of the session, one per line.
`summary.json`:

```json
{
  "session": "20260929-021710", "game_id": "unknown-e77d1d5f", "title": "Sky Meadow Online",
  "started": "2026-09-29T02:17:10Z", "ended": "2026-09-29T02:18:47Z", "duration_s": 74.75,
  "analysed_frames": 150, "deaths": 1, "victories": 0, "level_ups": 1,
  "advice_shown": 6, "advice_suppressed": 13, "feedback": {},
  "concepts": ["experience", "health", "mana", "level", "currency"], "highlights": []
}
```

`syrup live … --json` prints it at the end.
