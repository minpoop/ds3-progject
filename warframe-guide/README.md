# Tenno Field Manual

An unofficial, horror-themed Warframe guide website, built from the "Warframe Best Weapon Spreadsheet"
ChatGPT conversation.

Open `index.html` in a browser, or copy `tenno-field-manual.html` (everything in one file, about 11 MB)
to a phone and open it. Both work offline. Without a connection the fonts fall back to built-in ones
and the News chapter shows its built-in snapshot.

## What's in it

| Chapter | Contents |
|---|---|
| Overview | Frame of the day, latest official news, the six companions, quick links, a 10-step power checklist (saved in the browser) |
| News | Official Digital Extremes news: the in-game news feed (refreshed live when online) and the PC update notes from the official forums |
| Weapons | Two primary, secondary and melee picks for every Mastery Rank 0-30, with why each is good, how to get it, MR requirement, search |
| Warframes | All 67 base Warframes with what they're good at, trade-offs, Steel Path use, where to farm, build direction, role filter |
| Modding | Priority mods and where to farm them, build skeletons, damage types, common build traps |
| Systems | Helminth, Archon Shards, Operator/Focus/Amps, companions, Liches and Sisters, Railjack, Necramechs, Incarnons |
| Materials / Currencies | Best farm location, method, alternative and unlock for 34 resources and 22 currencies and standings |
| Platinum | Eight repeatable methods, safe trading, how to make a sale, what to spend on |
| Steel Path | 11-step unlock-to-endgame route, starter loadouts, low-damage checklist |
| Roadmap / Field Notes | Whole-game order of operations, sources to double-check, fine print |

**Companion radio** (bottom right, starts minimized as a "Tips" portrait): James Sunderland, Simon Henriksson, Dante, Vergil, Leon S. Kennedy and Torque
give tips for whichever chapter is open. **Another hint** rotates to the next character, **Tone** switches
between funny, serious and mixed lines, and tapping a portrait on the Overview picks a speaker. All lines are
original fan writing. Portraits are official character art from each game, taken from its fan wiki and cropped to busts.

**Atmosphere**: a painted haunted room behind the page (canvas), fog, film grain and flicker, optional
ambient sound generated in the browser, and rare jump-scare moments when cycling hints (can be turned off,
and "Test a scare" shows one on demand). Motion is reduced when the device asks for it.

## Files

- `index.html`: page shell and logo
- `assets/style.css`: all styling
- `assets/data.js`: the guide content (weapons, frames, mods, farming, platinum, Steel Path, roadmap)
- `assets/companions.js`: companion dialogue and portraits
- `assets/app.js`: rendering, search, filters, checklist, companion radio, sound and background
- `assets/art.js`: item pictures at full resolution, embedded as WebP (generated)
- `assets/news.js`: snapshot of official news and update notes, with images (generated)
- `assets/portraits.js`: companion portraits at 640×800, embedded as WebP (generated)
- `tenno-field-manual.html`: single-file build (generated)

## Rebuilding

```sh
cd warframe-guide
python3 tools/build_art.py        # re-download item pictures after editing names in data.js (needs Pillow + curl)
python3 tools/build_portraits.py  # re-download and crop the companion portraits
python3 tools/build_news.py       # refresh the news snapshot (do this before re-publishing)
python3 tools/bundle.py           # rebuild tenno-field-manual.html after any change
```

News comes from Digital Extremes' in-game news feed (served as JSON by [api.warframestat.us](https://api.warframestat.us/pc/news))
and the official [PC Update Notes](https://forums.warframe.com/forum/3-pc-update-notes/) forum feed. The page fetches the
in-game feed live when it is opened online from a file or a normal website; viewers that block outside requests show the snapshot.

Mastery Rank requirements and item pictures come from the community
[warframe-items](https://github.com/WFCD/warframe-items) data. Drop locations and vendors change with game updates,
so check the [official drop tables](https://www.warframe.com/droptables) or the
[wiki](https://wiki.warframe.com/) before a long farm.

Unofficial fan project, not affiliated with Digital Extremes or the publishers of the companion characters' games.
