# Events

Everything that happens in Syrup is an event on one bus
(`syrup_core::EventBus`). Modules publish; the session timeline, the devtools
page and anything else read. Nothing downstream needs to know which module
produced an event.

The bus numbers every event (`seq`, from 1) and keeps a ring of the last
4096. Readers ask for "everything after #N" (`EventBus::since`), take the
last few (`last`), or subscribe for live copies (`subscribe`). The session
timeline (`sessions/<id>/timeline.jsonl`) is every event except the
once-a-second `frame_captured` heartbeat, one JSON object per line:

```json
{"seq": 13, "event": {"type": "game_identified", "ts_ms": 1000, "identity": {…}}}
```

Every `ts_ms` is milliseconds since the source started: the frame's own
clock, never the wall clock, so replays and tests are deterministic.

## The events

| `type` | when | fields |
|---|---|---|
| `frame_captured` | once a second | `frame`, `width`, `height`, `captured_per_s`, `analysed_per_s` |
| `scene_changed` | the screen became another kind of screen | `from`, `to`: `unknown`, `gameplay`, `menu`, `loading`, `dialogue`, `cutscene`, `defeat`, `victory` |
| `game_identified` | Syrup is sure which game this is | `identity` (below) |
| `game_uncertain` | not sure (at most every 15 s) | `best`: the best guess, if any |
| `ui_element_discovered` | a new piece of interface | `region` (id), `kind` (`bar`, `minimap`, `text panel`, `icon`, `panel`, `element`), `place` (`top left`…) |
| `concept_learned` | an element was recognised as a concept | `concept`, `source` (`region:3`, `screen/hp`), `confidence`, `evidence` |
| `state_changed` | a transition in the game state | `transition`: `kind`, `subject`, `from`, `to`, `detail`, `confidence` |
| `player_died` | a defeat screen (a death, a lost match) | `context` (`during a boss fight`) |
| `objective_changed` | the objective text changed | `text` |
| `research_requested` | a question went to the research worker | `game_id`, `question`, `reason` |
| `knowledge_updated` | research came back with facts | `game_id`, `question`, `facts` (new ones), `sources` |
| `research_failed` | research came back empty | `game_id`, `question`, `reason` |
| `advice_generated` | a rule or plugin proposed a line (logged at most every 10 s per topic) | `advice` (below) |
| `advice_shown` | Syrup said it | `advice_id`, `text` |
| `advice_suppressed` | the policy held it back (at most every 10 s per topic) | `advice_id`, `topic`, `reason` |
| `feedback_received` | the player clicked 👍 👎 ❓ 🔇 … | `feedback`: `advice_id`, `topic`, `kind`, `at_ms` |
| `profile_updated` | the game's profile changed | `game_id`, `reason` |
| `plugin_activated` | a plugin took over for this game | `plugin`, `game_id` |
| `note` | anything else worth a line | `message` |

Transition kinds: `resource_fell`, `resource_rose`, `resource_low`,
`resource_empty`, `resource_recovered`, `counter_increased`,
`counter_decreased`, `text_changed`, `element_appeared`,
`element_disappeared`, `scene_changed`, `player_died`, `victory`,
`round_started`, `level_up`, `area_revisited`, `repeated_failure`, `idle`,
`objective_changed`.

## A game identity

```json
{
  "game_id": "unknown-e77d1d5f",
  "title": "Sky Meadow Online",
  "confidence": 0.9775,
  "version": null,
  "platform": null,
  "evidence": [
    {"signal": "executable", "detail": "skymeadow.exe", "weight": 0.85},
    {"signal": "window_title", "detail": "\"sky meadow online\" (seen before)", "weight": 0.85}
  ],
  "confirmed": false
}
```

`game_id` is the catalog's id (`maplestory`, `elden-ring`), or
`unknown-<hash of the executable>` for a game nobody catalogued. Evidence
signals: `executable`, `window_title`, `steam_folder`, `screen_text`,
`hud` (the look of the interface against a learned profile), `confirmation`
(the player said so).

## A piece of advice

```json
{
  "id": 1,
  "topic": "game",
  "kind": "status",
  "urgency": "educational",
  "text": "Don't know this one yet. Learning it with you.",
  "why": ["executable: skymeadow.exe (not in the catalog)", "window title: \"Sky Meadow Online\""],
  "confidence": 0.6,
  "spoiler": "none",
  "rests_on": [],
  "expression": "thinking",
  "origin": "syrup",
  "created_ms": 0,
  "expires_ms": 30000
}
```

- `kind`: `immediate_warning`, `tactical`, `strategic`, `build`, `economy`,
  `route`, `objective_reminder`, `mechanical_correction`, `learning`,
  `post_game`, `status`.
- `urgency`: `critical`, `important`, `opportunistic`, `educational`.
- `rests_on`: the kinds of knowledge behind it (`fact`, `current_patch`,
  `community_consensus`, `speculation`, `inference`); empty for what Syrup saw.
- `origin`: `syrup` (the universal rules), `knowledge`, `explain`, or
  `plugin:<id>`.
- `topic` is what cooldowns, muting and feedback key on: `low:health`,
  `timer`, `repeat-death`, `boss:<name>`, `learned:<concept>`, `game`…
