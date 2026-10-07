# Ashen Marine

**Play your own Dark Souls III character as a Space Marine.** Dark Souls III is the game you play;
Space Marine 2 brings its real weapons into it, read from your own installed copy.

> Status: early development. Nothing here is released yet. See [Roadmap](#roadmap).

## What a player gets

- Press Play in [Melty](https://melty.gg): Dark Souls III starts **offline** with your character on a
  **separate copy of your save**. Your real save and online account are never touched by the mashup.
- A Space Marine 2 **chainsword** and **bolt pistol** in your inventory from the first load, using
  Space Marine 2's own sounds (and its looks and numbers, where they can be read).
- Moves and damage rules are Dark Souls III's. The look, sound and numbers are Space Marine 2's.
- Single player only (offline).

## Requirements

- Dark Souls III (Steam), version 1.15.2.
- Warhammer 40,000: Space Marine 2 (Steam). The mashup finds your install itself and only **reads** it.
  If it is missing, the mashup says so in-game and Dark Souls III still runs.
- [ModEngine2](https://github.com/soulsmods/ModEngine2) - Melty installs it for you.

## How it is really "both games"

| Game | Role | What it brings |
|------|------|----------------|
| Dark Souls III | host (`primary`) | the game you play, your character, the world; started through ModEngine2 |
| Space Marine 2 | `secondary` (not in Melty's catalog yet) | the chainsword and bolt pistol: sounds, textures, models, numbers, read from your own install on first Play |

No Dark Souls III or Space Marine 2 file is ever shipped. A one-time setup step on your PC reads your own
copies and writes converted files into the mashup's private folder.

## Safety

- **Dark Souls III online:** FromSoftware's online service can flag or ban accounts that connect with
  modified files. The mashup therefore (1) redirects the game's save folder to its own copy, (2) blocks the
  game's network access, and (3) backs up and verifies your real save around every launch. If the sandbox
  cannot be established, the game is closed instead of continuing.
- **Space Marine 2 (Easy Anti-Cheat):** never launched, patched or hooked. Its files are only read.

## Repository layout

- `design/` - the design sheets (JSON). They are the source of truth; code is generated from them.
- `tools/` - preflight checker, code generator, and the read-only install scan.
- `MODLOG.md` - engineering journal: decisions, what failed and why.

## Roadmap

1. **M0** design sheets, preflight, read-only scan of the real SM2 files.
2. **M1** safe sandbox: separate offline save, offline guard, ModEngine2 launch.
3. **M2** read SM2 weapon content from the player's install.
4. **M3** chainsword and bolt pistol in Dark Souls III.
5. **M4** Melty release, real screenshot, publish.

## Credits

- [ModEngine2](https://github.com/soulsmods/ModEngine2) by the soulsmods community (loader).
- Dark Souls III is (c) FromSoftware / Bandai Namco. Warhammer 40,000: Space Marine 2 is (c) Games Workshop /
  Saber Interactive / Focus Entertainment. This is an unofficial fan project and contains none of their files.
- Built with AI assistance (Claude).

License: to be decided with the project owner before release.
