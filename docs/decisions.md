# Decisions

The choices that shaped Syrup Universal, and why. Each was made in the
order of priority the project set: **universality > modularity >
correctness > debuggability > realtime performance > visual polish**.

## 1. Nothing about any game in the core

The core types name no game concept: there is no `hp` field anywhere. A
game's interface becomes *concepts* (`health`, `mana`, `score`, `timer`…)
only when evidence says so, and they're stored in that game's profile.
MapleStory, the game Maplesyrup was built for, is a plugin like any other
could be. The test at every step: *would this still work tomorrow on a game
nobody has seen?*

## 2. Stillness is the interface

Whatever stays put while the game moves is interface. That one observation
finds HUDs in a first-person 3D game, an MMO and a card table alike, without
templates. Bars, text panels and minimaps are then told apart by their look;
what each one *is* is decided by how it behaves.

## 3. Concepts from evidence, with confidence, and the player has the last word

Each element collects evidence for each concept: a label read next to it, its
colour, its place, how it moves with the action, what it did at a death
screen, a plugin's hint, a player's correction. The concept with the most
evidence wins if there's enough; the evidence is kept and shown ("why?"). A
correction outranks everything. A concept is only spoken about once Syrup is
fairly sure of it.

## 4. One intermediate representation between seeing and understanding

Perception writes an `Observation`; everything after reads it. Detectors can
be replaced, plugins can add to it, and the devtools page and `--explain` show
exactly what the rest of the system was given.

## 5. Everything is an event

Modules publish events on one bus and don't call each other for
side effects. The session timeline, the devtools page and tests all read the
same events. The cost is some indirection; what it buys is that everything
Syrup did can be replayed and inspected.

## 6. Rust on the realtime path, one process

Capture, perception, state, coaching, the overlay and the voice share one
process and one frame budget. No serialisation between them, no second
runtime to install. TypeScript is used only for the devtools page (compiled
and committed, so a user needs no Node). Python is used only for offline
asset generation.

## 7. Nothing on the loop waits for the network or a slow engine

Research runs on a worker thread and comes back as events. Live, OCR runs off
the loop too, and the sampler backs off when analysis runs over budget. On
recordings, every sampled frame is analysed however long it takes, so results
don't depend on the computer's speed.

## 8. Text first, bar fill as corroboration

A number the game prints is the number. A bar's width is an estimate, marked
as such unless text corroborates it (from Maplesyrup, which measured OCR
misreading pixel fonts and learned to say how a value was obtained). Windows'
own OCR is preferred where it exists; Tesseract elsewhere; "no OCR" is a
supported mode, not a failure.

## 9. Recognition says "not sure yet"

Several independent clues (executable, window title, Steam folder, words on
screen, the look of the interface, the player saying so) are combined, and
below a confidence bar Syrup says it isn't sure. An uncatalogued game gets
an identity from its executable and a profile of its own. A game learned
before is recognised from its own profile. A profile made in the current
session doesn't count as "seen before".

## 10. Knowledge keeps its provenance, its version and its spoiler level

Every fact records where it came from, when, for which version of the game,
how confident, and what kind of claim it is (fact, current patch, consensus,
speculation, Syrup's inference). Advice says which of those it rests on.
Facts about older versions are flagged, and spoilers are held back by
default. Research only ever sends search terms out; fetched pages are cached
in the data folder.

## 11. Coaching is mostly silence

Candidates are cheap; saying them isn't. The interruption policy (urgency,
confidence, how busy the player is, how recently Syrup spoke, topic
cooldowns, a per-minute limit, what the player's feedback taught) decides.
Every held-back line is logged with its reason. Syrup's lines are short.

## 12. The overlay is painted in software

The same painter draws the overlay for the Windows window, for the devtools
page and for the pictures `--overlay-dir` saves. It looks the same
everywhere and it can be tested. On Windows the window is layered, doesn't
take clicks outside the card or the focus, and asks to be left out of screen
captures.

## 13. Syrup is Maplesyrup's dog, and the S always stays

The face comes from Maplesyrup's companion art (the derived frames are
committed; the originals are not). Hats are drawn as separate layers above a
face with the original cap cut away, and the S moved to a medallion so it
survives every hat. Hats come from the plugin, the catalog, or the genres
research finds.

## 14. Synthetic games with ground truth, real footage kept local

Three generated games (a raycast dungeon, an MMO-style side-scroller, a card
game) report the exact truth of every frame, so perception and state are
measured, not eyeballed, in every test run. Real games are evaluated on
local recordings. Their numbers go in `docs/evaluation.md` and their pixels
stay out of the public repository.

## 15. Live on Windows, in CI

A test game rendered into a real window, named `dungeon3d.exe`, is watched by
`syrup live` on a Windows runner with the overlay up, then again in a second
session that must remember it. Capture, the overlay and recognition run
against real Windows APIs on every push, not only on a developer's machine.

## 16. A pinned toolchain, and a look at the next one

CI builds, lints and tests with the Rust the code is written against (1.95),
so the build doesn't break when a new stable ships. A separate,
non-blocking job lints with the newest Rust so new lints are seen early.

## 17. Screen recordings lose their taskbar

A recording of the whole screen shows the operating system's taskbar under
the game, and positions learned from it would be wrong in a live session.
The taskbar is recognised by what it always shows (a date, a search box)
and cut off at its edge; `--crop` and `--no-crop` override.

## Deliberately not done (yet)

- **Input of any kind.** No automation: not in the MVP, and not planned.
- **Audio.** The architecture has room for listening to the game; the MVP
  only watches.
- **A language model in the loop.** Advice comes from rules, knowledge and
  plugins, which are fast, explainable and free. A model could phrase lines
  or answer questions later, behind the same interfaces.
- **Game-specific readers for MapleStory's bitmap fonts.** Maplesyrup has
  them; they're the next thing to move behind the plugin's
  `parse_observation` hook.
