# The devtools API

`syrup live --devtools [PORT]` (and `replay`, `simulate`) serves the devtools
page and this API on `http://127.0.0.1:7777` (only on this computer). The
page is `apps/devtools`, built into the program.

A request whose `Host` isn't this address is refused (`403`), so a web page
can't reach the server by DNS rebinding. Changes must be sent as
`application/json` (`415` otherwise), which another site can't do without
the browser asking first, and this server never allows it.

## Reading

| request | answer |
|---|---|
| `GET /api/snapshot` | everything the page shows (below) |
| `GET /api/events?since=N` | the events after #N, at most the last 400: `[{"seq": 12, "event": {...}}]` ([events](events.md)) |
| `GET /api/frame.png` | the last analysed frame, with every region, text box and moving thing perception found drawn on it (halved when wider than 1280) |
| `GET /api/overlay.png` | Syrup's overlay exactly as the player sees it |

The snapshot:

```json
{
  "session": "20260929-021710",
  "t_ms": 40000,
  "game": { "game_id": "…", "title": "…", "confidence": 0.97, "evidence": [ … ], … },
  "candidates": [ … ],
  "plugin": "maplestory",
  "ocr": "windows",
  "research": true,
  "scene": { "kind": "gameplay", "confidence": 0.8, "reason": "5 interface elements and motion" },
  "state": { "scene": "gameplay", "concepts": { "health": { "value": 0.87, "max": 1480, "unit": "fraction",
             "source": "region:9", "confidence": 0.95, "reliability": "corroborated", "trend": "falling",
             "evidence": ["labelled \"HP\"", "red bar"], … } }, "activity": { … }, "recent": [ … ], "hypotheses": [ … ] },
  "profile": { "game_id": "…", "title": "…", "genres": ["mmorpg"], "hat": "wizard_hat", "sessions": 2,
               "observed_min": 3.5, "elements": [ … ], "terms": [["quest", 12]], "version": "1.3.1" },
  "player": { "skills": { "survival": { "alpha": 1.0, "beta": 2.0 } }, "habits": { }, "deaths": 1, … },
  "facts": [ … ],
  "researched": [ … ],
  "advice": [ { "advice": { … }, "shown": false, "reason": "the player is busy", "at_ms": 39500 } ],
  "timings": { "perception": 21.4, "recognition": 0.1, "state": 0.6, "coach": 0.2, "total": 23.0 },
  "fps": { "captured": 8.0, "analysed": 3.9 },
  "regions": 7,
  "texts": ["HP 1193/1480", "Lv. 12"],
  "uncertainties": ["ocr: …"],
  "view": { "mode": "normal", "hat": "wizard_hat", "expression": "neutral", "line": { … }, "status": "Watching Sky Meadow Online." }
}
```

## Steering

| request | body | does |
|---|---|---|
| `POST /api/feedback` | `{"advice_id": 7, "topic": "low:health", "kind": "useful"}` | the player's reaction: `useful`, `wrong`, `explain`, `stop_suggesting`, `ignore`, `research` |
| `POST /api/mode` | `{"mode": "analysis"}` | the overlay mode: `hidden`, `minimal`, `normal`, `analysis` |
| `POST /api/confirm` | `{"title": "Hollow Knight"}` (optionally `game_id`) | "this game is …": a catalogued title picks its entry; any other title becomes a game of its own |
| `POST /api/correct` | `{"norm": {"x": 0.19, "y": 0.92, "w": 0.25, "h": 0.03}, "kind": "bar", "concept": "stamina"}` | "that element is …" (`"concept": ""`: it is nothing); stored in the profile, outranks inference |
| `POST /api/research` | `{"topic": "Mossy King"}` | look this up now |
| `POST /api/command` | a whole command, e.g. `{"command": "mode", "mode": "minimal"}` | any of the above |

Each answers `{"ok": true}`, or `400` with `{"error": "…"}`. Commands take
effect with the next captured frame.
