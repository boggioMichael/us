# What Syrup Universal takes from Maplesyrup

Maplesyrup (`github.com/boggioMichael/ms`) is a Rust, Windows-first companion
for MapleStory that reads the game only through captured pixels. It was
inspected before any of Syrup Universal was written: `master` at `19f3fb3`
(the merged MVP, about 12,000 lines of Rust), the `feature/ui` branch
(`b0f0099`, the animated voice companion in PySide6), and the design documents
under `docs/`. This page records what carries over, what is deliberately left
behind, and where each idea now lives.

## Carried over

| Maplesyrup | Why it is worth keeping | Where it lives now |
|---|---|---|
| **The non-invasive boundary**: no memory reads, injection, hooks or input automation | It is what makes a companion safe to run next to any game, including ones with anti-cheat | The whole architecture; see [overview.md](overview.md#boundaries) |
| **`Detection<T>`**: value, confidence, timestamp, source, reliability (`Corroborated`, `Heuristic`, `Predicted`, `Unreliable`), failure reason | Downstream code can tell "HP is 50%" from "found a red bar, might be HP" from "nothing found, and here is why" | `syrup-core::detection` (the source became a string, so any detector or plugin can name itself) |
| **`Confidence`** as a clamped newtype with `combine` (probabilistic OR) and `decay` | Evidence from independent detectors adds up without anyone inventing a formula each time | `syrup-core::confidence` |
| **Temporal building blocks**: `Ema`, `ConfidenceAccumulator`, the centroid `ObjectTracker` with velocity prediction and a grace period, `History<T>` | Single frames are noisy. These are small, tested and game-neutral | `syrup-state::temporal`, `syrup-perception::motion` |
| **Failure transparency**: a failed detection says why | "No OCR engine" and "no text" and "text too blurred to read" are different situations | `Uncertainty` entries in every `Observation` |
| **Reading values as text first, bar fill only as corroboration**, with per-region legibility | A number the game prints is the number; a bar's width is an estimate. Maplesyrup measured that rescaled or compressed captures destroy pixel-font digits and flags such reads instead of guessing | `syrup-perception::bars` + `syrup-perception::text` (a bar's value is marked `Heuristic` unless a label or number corroborates it) |
| **Skin-agnostic panel finding** by dominant quantised colour, and proportional search regions instead of fixed pixel offsets | Works at any resolution and UI scale | `syrup-perception::regions` (panels) and normalised rectangles (`NormRect`) everywhere a region is stored |
| **Dialog classification by text** (death, revive, level up, rune, generic) | The same modal shapes are reused for many purposes; the words tell them apart | Generalised into scene/dialogue cues in `syrup-state`; MapleStory's exact keyword families in `plugins/maplestory` |
| **Knowledge as original-authored data with stated provenance** (monster behaviour, rune and portal mechanics) | Advice must be traceable; nothing scraped verbatim | Seed facts of the MapleStory plugin, each with provenance "Maplesyrup knowledge module" |
| **The debugger**: one per-frame result rendered both as text and as boxes on the frame, with `--explain` giving the region, raw text, parse and confidence of every value | The only reliable way to see what the engine believes | `apps/devtools` (the live page draws the same boxes the engine reports) and `syrup replay --explain` |
| **Replay-based validation** with a real fixture and measured evidence instead of claims | Perception must be judged on recorded frames with known answers | `syrup-testgames` (synthetic games with exact ground truth) and local runs on real footage |
| **Window discovery with a picker and a static-image fallback**; `PrintWindow` with a screen-copy fallback and a blank-surface check | Real windows are messy: occluded, GPU-drawn, minimised | `syrup-capture` (Windows) and the `syrup` library's capture |
| **The Windows OCR engine** over Tesseract for game text | Maplesyrup measured Tesseract returning `6201-1` for a plainly visible `1291/1351`; the OS engine is trained on screen content and ships with Windows | `syrup-perception::ocr` (Windows OCR first, Tesseract where installed, and an explicit "no engine" otherwise) |
| **The mascot**: the cream fluffy dog, the syrup-dripping pancake cap with its orange **S**, brown `#57351F` strokes and captions, reduced-motion support, a caption bubble above the avatar that does not take input | The identity people already know | `assets/syrup` and `syrup-avatar`: the dog's head from Maplesyrup's idle animation, the S kept on a medallion so it survives every hat change, and the original cap as the MapleStory hat |

## Left behind, on purpose

- **MapleStory as the centre of the design.** Maplesyrup's pipeline names HP,
  MP, EXP, minimap, chat log, platforms and runes in its core types
  (`WorldState`, `GameState`). Syrup Universal's core types name none of
  them: they are *concepts* a game may turn out to have, discovered from what
  is seen (a bar that falls when the screen shakes and ends in a death
  screen is probably health) and stored in that game's profile. The
  MapleStory specifics moved into `plugins/maplestory`.
- **Fixed regions of interest.** Maplesyrup looks for the minimap in the top
  left and the chat in the bottom left. Syrup Universal first finds what
  stays still while the game moves (the HUD), then asks what each stable
  thing behaves like.
- **OCR on every frame.** Text is re-read only when its region changed, under
  a budget.
- **The PySide6 companion window and the 120-frame, 80 MB idle animation.**
  One process and one language for the realtime path matter more here; the
  avatar is composed from a few small layers (head, hat, medallion,
  expression) in Rust. The Python voice stack (Kokoro TTS, local STT,
  Ollama/OpenAI agent) stays an option to plug in later behind the voice and
  reasoning interfaces; the MVP speaks through the system voice.

## Where Maplesyrup goes next

`plugins/maplestory` is the compatibility layer: it recognises the game,
carries MapleStory's HUD layout priors, dialog keyword families, seed
knowledge and the original Syrup cap. Maplesyrup's specialised readers (the
bitmap-font HUD reader, bar geometry tuned on real frames) are the next thing
to move behind the plugin's `parse_observation` hook, where they can add
exact readings on top of what the universal detectors already found.
