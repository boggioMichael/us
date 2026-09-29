# The observation IR

Perception never says "HP". For every analysed frame it says what it saw,
in one intermediate representation (`syrup_core::Observation`), and everything
downstream (recognition, the state engine, plugins, the devtools page) reads
that. This is a real one, from the side-scroller test game 19.5 s in (lists
shortened):

```json
{
  "frame_index": 78,
  "timestamp_ms": 19500,
  "frame_size": [960, 540],
  "scene": { "kind": "gameplay", "confidence": 0.8, "reason": "10 interface elements and motion" },
  "objects": [
    { "id": 120, "rect": {"x": 810, "y": 384, "w": 6, "h": 54}, "velocity": [-6.0, -12.0],
      "age_frames": 9, "predicted": false, "confidence": 0.75, "color": [100, 70, 40] }
  ],
  "text": [
    { "text": "Defeat slimes (4/10)", "rect": {"x": 710, "y": 43, "w": 147, "h": 13},
      "region": 4, "confidence": 0.96, "engine": "tesseract", "fresh": false }
  ],
  "ui_regions": [
    { "id": 3, "rect": {"x": 0, "y": 0, "w": 240, "h": 132}, "norm": {"x": 0.0, "y": 0.0, "w": 0.25, "h": 0.244},
      "kind": "minimap", "stability": 0.97, "confidence": 0.79, "appearance": 9274179234379629696 },
    { "id": 10, "rect": {"x": 2, "y": 530, "w": 956, "h": 8}, "norm": {"x": 0.002, "y": 0.981, "w": 0.996, "h": 0.015},
      "kind": { "bar": { "fill": 0.684, "color": [218, 183, 62], "vertical": false } },
      "stability": 1.0, "confidence": 0.95, "appearance": 117476633074428198 }
  ],
  "characters": [ { "object": 120, "role": "other", "confidence": 0.2, "reason": "moves on its own" } ],
  "events": [
    { "kind": "region_disappeared", "region": 19 },
    { "kind": "text_changed", "region": 35, "text": "…" }
  ],
  "relationships": [
    { "from": {"text": 0}, "rel": "inside", "to": {"region": 4} },
    { "from": {"text": 1}, "rel": "labels", "to": {"region": 10} }
  ],
  "uncertainties": [],
  "metrics": { "change": 0.09, "motion": 0.033, "brightness": 0.53, "saturation": 0.27, "red_tint": 0.021, "detail": 0.17 },
  "signature": { "hash": 1058806853557313538, "histogram": [ … ] },
  "analysis_ms": 352.6
}
```

| field | what it is |
|---|---|
| `scene` | which kind of screen: `unknown`, `gameplay`, `menu`, `loading`, `dialogue`, `cutscene`, `defeat`, `victory`; with the reason |
| `objects` | things that move against the scene (camera motion removed), tracked across frames with a velocity |
| `text` | what was read, where, from which region, by which engine; `fresh` when it was read this frame (text is re-read only when its region changes) |
| `ui_regions` | the interface: whatever stays still while the game moves. `kind` is `bar` (with its fill, colour and direction), `minimap`, `text_panel`, `icon`, `panel` or `unknown`. `norm` is the rectangle in fractions of the frame (what profiles store, so it survives a resolution change); `appearance` is a perceptual hash |
| `characters` | moving things that may be the player or others: `player_candidate`, `other` |
| `events` | `scene_changed`, `region_appeared`, `region_disappeared`, `text_changed`, `flash` (the screen jumped in brightness), `color_drain` (it went grey or red, and held still: deaths often look like that) |
| `relationships` | `labels` (this text names that bar), `inside`, `near` |
| `uncertainties` | what perception could not do, and why ("no OCR engine on this computer", "still learning what stays put") |
| `metrics` | whole-frame measures: change since the last analysis, motion, brightness, saturation, red tint, detail |
| `signature` | a hash and a colour histogram of the frame, for "have I been here before?" |

Everything with a confidence is between 0 and 1. Rectangles are in the
frame's pixels (top left, size) unless named `norm`.

`cargo run -p syrup-perception --example observation_json -- <dungeon|scroller|cards> <seconds>`
prints one; `syrup replay --explain` prints what they add up to as the
recording plays.
