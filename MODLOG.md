# MODLOG - Ashen Marine engineering journal

Anything not written here is lost at the next context compaction. Newest entries at the bottom of each section.

## Decisions (agreed with the project owner)

- **Idea:** play your own Dark Souls III character as a Space Marine ("Ashen Marine").
- **Host / guest:** Dark Souls III is the host (`primary`, ModEngine2 installed by Melty). Space Marine 2 is **not in
  Melty's catalog**, so it cannot be host or companion; it is `secondary`: the mod finds the player's SM2 install itself and
  only **reads** it. Playing *inside* SM2 is out of scope (cannot be published on Melty today).
- **First build ("Swing it first"):** a chainsword and a bolt-pistol-style gun in the player's inventory, using SM2's real
  sounds (and looks / numbers where readable). DS3's moveset and damage rules.
- **Save safety (owner's choice):** a **separate offline copy** of the DS3 save; the real save and online account are never
  touched; the game's network is blocked. Single player only.
- **No look-alikes** of SM2 content. If a model cannot be read, the first build says so (labelled partial); nothing is
  faked.
- **Publishing:** only after the owner has played it in Melty and authorises. Test builds are private dev kits, never
  `publish`ed (Melty goes live as soon as the owner presses Play on a published draft).

## Environment facts

- Work happens in a cloud container (Linux). **Neither game is installed here**; no computer-use or linked device. Real
  launches, screenshots and the final Melty "Play to publish" run must happen on the owner's PC.
- Toolchain here: Rust 1.97 + `x86_64-pc-windows-gnu` target, mingw-w64, Wine 9 (headless via xvfb), PowerShell 7 (to test
  the scan script). Python 3 for `tools/`.
- GitHub: `git push` is refused (403, Claude GitHub App has no write access to minpoop/ds3-progject) and so is the GitHub
  connector (403 "Resource not accessible by integration"). Work is committed locally only until the owner reconnects /
  installs the app. Source snapshots are sent to the owner as files meanwhile.

## What is verified (and how)

| Claim | Evidence |
|-------|----------|
| Path redirect, network rules, save backup/verify/restore, config, Steam parsing | 28 native unit tests |
| Hook DLL: every file API under the real save path lands in the private copy; non-loopback connect/sendto/DNS/ConnectEx/WinHTTP/WinINet refused; loopback works | `tools/wine-sandbox-test.sh` scenario A (26 probes + external checks) under Wine |
| The guard is what refuses connections | scenario B (control run with the guard off) |
| Fail closed (missing private save / missing config) | scenarios C, D |
| Launcher: backup, private copy, launch, verify; restore if the real save changed; refuse if game already running / second launcher | `tools/wine-launcher-test.sh` E, H, F, G, I, J |
| Real ModEngine2 2.1.0 accepts our config + CLI and loads our hook DLL through `external_dlls` | scenario K and an earlier manual run (ME2's own log: "Loaded external DLL ...") |
| SM2 readers (`crates/sm2`): zip paks incl. a stored zip inside a pak, both texture mip layouts, BC1-BC7 decode to PNG/DDS, Wwise bank chunks + event->action->container->sound chains, wem headers, PCM->WAV | 26 unit tests on synthetic data (no game file) |
| `ashenmarine-setup probe`: finds SM2, reports weapon files / textures / .cls / sounds, writes previews; **install stays byte-identical** | integration test on a synthetic install + `tools/wine-setup-test.sh` (the shipped .exe under Wine) |
| Hook `features` (read-only probe): strict FMG text-table finder in own memory (no false hits in random data), version gate, diagnostics, two-beep audio path, mixer | Wine unit tests (5) + native tests (40 in common incl. fmg + mixer); launcher scenarios L/K (probe thread starts, copes with an unknown game, sandbox unaffected) |
| **On the owner's real PC (kit 0.2, 2026-10-07):** DS3 1.15.2.0 English starts through ModEngine2 with our hook; 30 hooks, both in-process self-tests passed; ~240 save-file operations redirected to the private copy; the game's login host and `api.github.com` lookups refused; real save byte-identical afterwards (VERIFIED); per-frame game task registered and ran (~58 frames/s) | `AshenMarine-logs.zip` of kit 0.2 |
| SM2 readers on the real install (114 client paks, 80 GB, 475k names, 0 duplicates, 11 s): texture descriptors + BC7/BC5 decode (owner confirmed the previews look like textures), `.cls` text, 30 Wwise banks, 34 sound zips | kit 0.2 probe report |
| **On the owner's real PC (kit 0.3, 2026-10-07):** both beeps heard (title screen and in the world), so our own waveOut mixer plays next to DS3's audio; the per-frame task, `PlayerIns`, `GameDataMan`, `MapItemMan`, the inventory (read, and a change detected: Estus use) and the equipment-slot array all work in a real world; 98 parameter tables are registered | `AshenMarine-logs.zip` of kit 0.3 |
| Wwise Vorbis (SM2's sound format) -> PCM: `crates/sm2/src/wwvorbis.rs` (ww2ogg algorithm, aoTuV 603 codebooks, lewton) | 24 real clips from the owner's PC: setup header and every audio packet **bit-identical** to the reference ww2ogg, PCM within +-1 LSB of ffmpeg's decode of the reference Ogg, sample counts exact (`crates/sm2/tests/wem_samples.rs`, gated on `ASHEN_SM2_WEM_DIR`) |
| Trigger logic (swing = attack button + sudden stamina drop, combo steps, shots = ammunition count falling, F7 loop) end to end into the mixer messages | unit tests with made-up frames (`ashen_common::triggers`, `hook::features::sfx` under Wine) |
| **Not verified yet:** that the triggers fire at the right moments in a real fight (stamina scale, button bindings), that `give_item_directly` + in-memory renaming work, how item names are stored, the weapon table dump (needs the `name_offset` fix), and Space Marine 2 event -> sound selection on real banks | kit 0.4 |

## Findings / gotchas

- ModEngine2 2.1.0 launcher CLI: `-t ds3 -p <game exe> -c <toml> --modengine-dll <dll>` (also `-s` suspend). DS3 config keys:
  `[modengine] external_dlls`, `[extension.mod_loader] mods = [{enabled,name,path}]`, `[extension.scylla_hide]`. No save
  management and **no network switch** for DS3: both are ours to do. ME2 warns "not a modengine extension" for a plain DLL
  (harmless). ME2 logs to `modengine2/logs/modengine_<date>.log`.
- Windows and Wine export some file APIs from `kernel32` and others from `kernelbase` (`MoveFileW` is not in Wine's
  kernelbase): every file hook has a `fallback_module`. Found by the Wine tests; would have wrongly closed the game.
- `Clean` is a reserved block keyword in PowerShell 7.3+: the scan script's helper is called `Mask`.
- The hook must never show a dialog while the game keeps running: it writes `FATAL.txt` and terminates; the launcher shows
  the reason after the process is gone.
- ModEngine2's launcher stays alive while the game runs (real behaviour): the launcher watches the game process, not ME2.
- `darksouls3` 0.14 (read from source, not run): knows only DS3 **1.15.2.0 (English)** and **1.15.2.1 (Japanese)** and *panics* on any other
  build, so the hook checks the exe's version resource first. It gives: `GameDataMan::give_item_directly(ItemId, qty)` and
  `MapItemMan::grant_item` (with pop-up); `CSRegulationManager::get_param::<EQUIP_PARAM_WEAPON_ST>()` rows editable in memory (fields incl.
  `atk_base_physics`, `equip_model_id`, `icon_id`, `wep_se_id_offset`, `arrow_bolt_equip_id`, `max_arrow_quantity`, `durability`, `weight`);
  `PlayerGameData` (stats, inventory with quantities, equipped slots); `ChrDataModule` hp/fp/stamina; a per-frame task API (`SprjTaskImp::run_recurring`).
  It does **not** give: animation/TAE state, message (FMG) lookup, sound events, input.
- DS3 PC has Arxan (guardIT) code restoration: do not patch game code; our inline hooks are only on system DLL APIs, the crate's task API
  runs on the game's own thread instead.
## Kit 0.2 results (owner's PC) - what they showed

- DS3: `I:\SteamLibrary\steamapps\common\DARK SOULS III\Game\DarkSoulsIII.exe`, product version **1.15.2.0**, language 0x09 -> supported by the
  `darksouls3` crate. Real save: `%APPDATA%\DarkSoulsIII\<steamid64 as 16 hex digits>\DS30000.sl2` (+ GraphicsConfig.xml).
- The game asks for `fdp-steam-ope-login.fromsoftware-game.net` (its login server, retried ~6 times) and ModEngine2 for `api.github.com`: both refused, game
  carries on offline. ModEngine2 prints "not a modengine extension" for our DLL (harmless) and ScyllaHide injects fine.
- **Probe bug (fixed in kit 0.3):** `CSRegulationManager` is non-null before its parameter vector is filled; `get_param` indexes `params[26]` and
  panicked ("len is 0 but the index is 26"). My first version let one failed step switch the whole probe off, so no world data and no beep. Now: every step is
  tried on its own, retried, given up only after 5 failures; every pointer hop is checked with ReadProcessMemory first; the beep also plays at the title screen.
- Text: at the title screen "Straight Sword" occurred exactly once in memory (4.9 GB scanned), as an isolated hashed-string object, not in an FMG table, so item
  names are probably loaded later (or stored differently). Kit 0.3 rescans in the world, tolerates relocated (absolute) string pointers and logs near-misses.
- SM2 (build 25098992): paks are `client_pc/root/paks/client/**` (114 files, the earlier 184 included server paks). Weapon templates are folders named
  `tpl/<name>.tpl/` holding `.tpl .lods_base .cdt .tpl_data .geom_dbg .tpl_markup` (+ `.tpl.resource`, `.tpl_markup.resource` in resources.pak): chainsword 8 files
  (tpl 453 KB, tpl_data 1 MB), bolt pistol 7 (tpl 126 KB, tpl_data 684 KB); 14 chainsword and 34 bolt-pistol variants.
- SM2 textures: `pct/<name>.pct.resource` is YAML `res_desc_pct` (format 51 = BC7 mostly, `_nm` = DXN/BC5, `_spec` = BC7), one data file per mip named
  `<name>_1.pct_mip` ... `_N` where `_1` is the TOP mip; 1024x1024 BC7 top mip = 1 MiB. My loader needs no change.
- SM2 data: `.cls` is Saber's `key = { ... }` text with `__type = "..."`. Weapon stats are not yet seen (kit 0.2 only printed unrelated sfx classes); candidates:
  `ssl/weapons/melee/weapon_actors/wpn_melee_chainsword.cls`, `ssl/weapons/common/firearm/firearm_versions/hgun_bolt_pistol/*_authority.cls`,
  `ssl/characters/player/marine/pc_marine_pve_authority.cls`. `sounds/stats/editor.*.events.csv` lists real event names (`scene,bank,event,count,...,seconds`,
  e.g. `wpn_firearm_shoot_2d_bolt_pistol_heavy`).
- SM2 sound: 30 banks, Wwise **version 150**; `wpn.bnk` has only BKHD+HIRC (no DIDX/DATA): 7186 sounds, 838 events, all media lives in `sounds/desktop/wpn.zip`
  (3642 `<mediaid>.wem`, stored zip). The 22 media my census could follow are **Wwise Vorbis (0xFFFF)**: `fmt ` chunk 66 bytes with the `vorb` block inside
  (sample count at +8 of the extra bytes, setup packet offset ~888, first audio packet ~1091 -> a ~200 byte setup packet = codebooks referenced by id from
  an external library), plus a 16-byte `hash` chunk. So decoding needs a ww2ogg-style rebuild + the packed codebook library (BSD) + a Vorbis decoder.
- Triggers for the weapon sounds will be polled (a stamina drop = a swing, a falling ammo count = a shot); item names will need the text tables
  (FMG) found in memory, or a loose msg override. The probe in kit 0.2 collects exactly that evidence.
- Panics now unwind (workspace profile): optional features catch them at their own thread entry points and log; every `extern "system"`
  boundary still aborts on unwind, as before.

## Scan part 1 (owner's PC, 2026-10-07) - what it showed

- SM2: `I:\SteamLibrary\steamapps\common\Space Marine 2`, Steam build 25098992. Layout: `client_pc\root\{bin,loadconfig,local,mods,paks,prebuild,sandbox}`,
  `EasyAntiCheat\`, `server_pc\`, `start_protected_game.exe`, `Warhammer 40000 Space Marine 2.exe`. An official `client_pc\root\mods` folder exists.
- 184 `.pak` files (client: `resources.pak`, `default\default*.pak`, `default\scenes\*.pak`; plus `server_pc`). **Every pak opens as a standard zip** (stored + deflate mix),
  up to ~118k entries each. Content types by count: `.pct_mip` 173k (64 GB), `.resource` 170k (resources.pak), `.sani/.sani_data` (animation), `.sslbin`, `.geom_dbg`,
  `.tpl` 29k, `.lods_base`, `.cdt`, `.tpl_data` 26k (19 GB), `.td`, `.sso`, `.tpl_markup`, `.cls` 3.3k (TEXT), `.asset` (TEXT, `__type: res_desc_texture`), `.sfx`, `.bik`, `.bnk`
  (Wwise, `BKHD`, bank version 0x96 = 150), nested `.zip` (34, 6.7 GB) with `.header` sidecars in the `default_sound_*.pak` archives.
- Weapon templates live in `default_tpl_*.pak` as `tpl/wpn_<name>.tpl/wpn_<name>.{tpl,lods_base,cdt,tpl_markup,tpl_data,geom_dbg}`; e.g. `wpn_chainsword_challenge_mode_01/02`,
  `wpn_chainsword_space_wolves_01`, `wpn_bolt_rifle_01_relic_01/02`, `wpn_bolt_carbine_01_*`, `wpn_thunder_hammer_librarium_01`. Textures: `pct/wpn_*_<map>_<n>.pct_mip`
  (4 MiB samples). Weapon text data: `ssl/weapons/**/*.cls` (default_other.pak). Weapon sound bank: `sounds/desktop/wpn.bnk` (881 KB).
- **The report was cut at 470 lines by my own cap**, losing the whole DS3 section and the last file-type samples. Fixed with scan part 2 (DS3 first, capped sections).

## Kit 0.3 results (owner's PC, 2026-10-07) - what they showed

- Owner heard **both beeps** (title screen and in the world). The probe now survives the early title-screen state (per-step resilience works).
- The character is a fresh Knight: inventory = Fists x4 (weapon row 110000), **Longsword (2010000)**, **Knight Shield (21040000)**, 4 armour pieces, Estus Flask (goods
  151), Ashen Estus... (goods ids 94/103/117/119/151/191/1000). Stats: vigor 12 ... luck 7 (level ~8). Equipment slot array: [128..133, -1 x6, 145,135,136,137, -1 ...].
- 98 parameter tables registered; only 39 were "readable" because **my own check rejected `name_offset > 0x1000`** (the table's name sits behind all rows, so big tables - weapons
  among them - were refused). Fixed in kit 0.4. `ParamRowInfo` has a third word (`_unk10`) that is very likely the row-name offset; kit 0.4 reads it.
- `probe-ds3-samples.csv` was lost (flushed every 100 rows, game closed earlier). Kit 0.4 flushes every 3 s.
- Text: "Straight Sword", "Estus Flask" (x1, later x4 incl. "Ashen Estus Flask"), "Longsword" each occur **once** as separate UTF-16 strings, each preceded by an 8-byte block header
  `[u32 hash?][00][u16 counter][u8 flags 0x80/0x88/0x90]` (counters of neighbouring blocks differ by one: consecutive heap allocations), then the characters, then NUL; **no
  `.fmg`-shaped table exists in memory** (0 of 6 scans). "Straight Sword" is not an item name in DS3 (weapon category label); item names are "Longsword", "Shortsword" (2000000), "Light Crossbow"
  (14040000), "Standard Bolt" (404000), "Knight Shield", "Fists" (Paramdex, MIT-style name lists; the game's own table is checked by the kit 0.4 probe). Plan: find the pointer(s) to these
  strings (kit 0.4 text diagnosis: neighbourhood + pointer scan) and, as a first experiment, overwrite name strings in place (same length, space padded).
- The first launcher run lasted ~2 s before the second one: Steam was probably not running, so the game restarted itself through Steam (without our protection). The launcher now says so.

## Kit 0.4 results (owner's PC, 2026-10-08) - what they showed

- Owner: **"all sounds work as expected; the names of the weapons were default and the models are default; the crossbow only held 1 shot"**.
- `prepare` produced 13 slots / 27 files from `wpn.bnk` (Steam build 25098992, bank v150, 12509 objects) but **the exact bank reading failed** (482 of 9544 sound/container objects read exactly; sounds
  "parsed 44 of 45"), so the approximate walk ran (every sound at full volume). Cause found afterwards from the public Wwise format description (wwiser): in v150 a node has no "override attachment"
  byte, **does** have a metadata-plug-in block, property ids differ (Volume 0x00, Pitch 0x01, MakeUpGain 0x05, InitialDelay 0x22 in seconds), ranged properties are ids-first, 3D bytes exist only when a node
  overrides positioning, state chunks carry property bundles and use variable-size ints. A minimal sound is exactly 45 bytes in that layout. Kit 0.5 implements it (unit tests built byte by byte from the description).
- Triggers: stamina drops of 17 per light attack, 26 per crossbow shot (stamina max 95); the mouse buttons were read as LMB / RMB only (no pad). **The crossbow was in the LEFT hand**: RMB fired it and, because RMB was
  mapped to "strong attack", also played `chainsword_strong`. The game's default keyboard layout is LMB = right-hand attack, **Shift + LMB = strong attack**, RMB = left-hand weapon (public control lists).
- Equipment slots (from the owner's log): `equipment_indexes` = [LH1, RH1, LH2, RH2, LH3, RH3, arrow1, bolt1, arrow2, bolt2, ...]; after F8 the bolts auto-equipped to slot 7, equipping the new sword changed entry 1
  (right hand 1), the crossbow entry 0 (left hand 1). Empty hands point at inventory entries for "Fists" (row 110000). So `equipment_indexes[slot]` -> inventory entry -> item id works; the seven numbers in front of the table
  (`EquipGameData + 0x08`: arm style, left/right weapon slot in use, arrow/bolt slots) are read as the ChrAsm record of other FromSoftware titles and are only trusted when they are in range (logged either way).
- Names: the memory rename only reached transient copies of the strings ("Light Crossbow" 1 hit, "Standard Bolt" 1 hit, "Shortsword" 0 hits) and changed nothing on screen. The real source of item names is `msg/<language>/item.msgbnd.dcx`
  inside the game's encrypted archives. Kit 0.5 reads those archives (own implementation of BHD5 / DCX / BND4 / FMG, read-only) and writes a patched copy as a **loose file for ModEngine2** (`mod/msg/ENGLISH/item.msgbnd.dcx`).
- Single shot: the Light Crossbow fires one bolt and reloads (design). Weapon table facts: Light Crossbow 14040000, Avelyn 14090000 (same weapon category 11 / motion 46), Repeating Crossbow 14190000. **Avelyn sends three bolts per trigger
  pull** (base game; 16 STR / 14 DEX; weight 7.5): the bolt pistol now repurposes it, each bolt of a burst plays a shot sound at least 110 ms apart.
- Weapon table categories (this build): 1 straight sword ... 8 staff/flame/chime, 9 fists and claws, 10 bow, 11 crossbow, 12 shield, 13 arrow, 14 bolt; bolt rows 404000..404600.

## Kit 0.5 (this window) - what changed and why

- Hand-aware attacks: attack inputs map to hands (RB / LMB = right, Shift+LMB or RT = strong right, LB / RMB = left); the weapon class in that hand decides whether a swing sound plays (melee yes; crossbow, bow, shield, catalyst, unarmed no);
  unreadable equipment = everything sounds (as in kit 4). Shots only count while a crossbow is in a hand and only for bolt stacks (category 14).
- Bolt pistol = Avelyn; bursts are queued (`ShotQueue`): the first shot at once, further ones >= 110 ms apart. F8 is driven by `design/sheets/weapons.json` and does not give a weapon twice.
- Two sound sets: `sounds/` (kit-4 style, the default) and `sounds-exact/` (the bank's own volumes and delays); F9 switches and plays a preview swing. The owner decides which is better; then the default flips.
- Names via the archives (see above); public RSA keys for the archive headers are taken from the player's `DarkSoulsIII.exe` (PEM text) or, if the program file does not hold them as text, from `cache/ds3-keys.pem` which the in-game probe writes from memory.
- Model groundwork: `ashenmarine-setup sm2-mesh-probe` writes a structural report on the chainsword / bolt pistol template files (hex heads, names, entropy, vertex/index buffer finder); `ds3-probe` also lists the BND4 contents of
  the DS3 weapon model files. Models stay "DS3 default" in this kit.

## Space Marine 2 sound events found (kit 0.3 report)

- `wpn.bnk` v150: 7186 sounds, 838 events; media in `wpn.zip` (3642 `.wem`). Names recovered by hashing words: chainsword (`chswd`) events - `wpn_melee_chswd_light_1hit..4hit` (12-16 sounds each),
  `slash_1hit..5hit` (+`_mirror`), `idle_start` / `idle_loop`, `charge_loop_start/stop`, `charge_step_01/02`, `dodge_attack_01`, `parry`, `riposte`, `finish_war_*`; `wpn_melee_chswd_3d_*` are the
  3D-positioned (other players) versions. Bolt pistol: `wpn_firearm_shoot_2d_bolt_pistol[_heavy|_deathwatch|_suppressed|_burst|_empty_shoot]` (~280 sounds = layers x random variants x tails),
  `wpn_firearm_shoot_3d_*`, `wpn_firearm_foley_bolt_pistol_zoom_in/out`.
- Containers are random/switch/layer. Kit 0.4 `prepare` reads the bank **exactly** where it can (`crates/sm2/src/hirc.rs`, layout after rewwise: node base params, volume/pitch/delay properties inherited down the tree,
  random playlists with weights, switch containers via their default switch, layer containers = all children; a parsed object must use its bytes exactly) and counts how many objects it understood in the
  report ("reading the bank exactly: N of M ... read exactly"); if fewer than ~98 % are understood it falls back to the approximate walker (`Bank::resolve_take`: random/switch = one child, layer/mixer = all, equal
  levels). Each play of an event is rendered 1..3 times (seeded, reproducible), mixed, and all sounds are brought up together so the loudest is at 90 % of full scale. Everything about real banks is
  **unverified until the owner's report comes back**: v150 layout of the node parameters, Play action = 0x0403, whether chainsword/bolt-pistol events live in `wpn.bnk` (the report lists missing ones and where they are).

## Research findings (public sources; no SM2/DS3 files involved)

| Topic | Finding | License / use |
|-------|---------|---------------|
| SM2 mods (official docs, prismray.io; egress-blocked here, read via search summaries) | Mod packs = **stored** zip paks with root folders `tpl`, `pct`, `ssl`, placed in `client_pc\root\mods`; mod paks outrank default paks; since SM2 7.0 mod paks are the only way to mod. | n/a |
| `.pct_mip` textures (vash2pid/texmipper) | `.pct_mip` is **raw block-compressed pixel data** (DDS header stripped) per mip; a text `.resource` descriptor (YAML: `header{sx,sy,format,nMipMap,mipLevel[]...}`, `mipMaps[]`) says size/format; format ids 0-64 (XT1..XT5 = DXT1-5, DXN = BC5, DXT5A = BC4, BC7 = 51/52 ...). | MIT: may be ported with credit |
| Models (Wildenhaus/LibSaber + IndexV2, ResHax thread) | Saber "1SER" property-flag serialization (class lists, `objGEOM_MNG`, `objGEOM_VBUFFER_INFO/MAPPING`, `objGEOM_STREAM_TO_VBUFFER`, `objSPLIT_RANGE`, FVF vertex buffers). Needed fixes after SM2 patches (Oct 2024, Apr 2025, Sep 2025): **format drifts with game updates**. | **No license file**: read for format facts only; write our own parser; never copy code |
| Wwise banks | Standard `BKHD/DIDX/DATA/HIRC` banks; `vswarte/rewwise` is a Rust parser/unpacker for ER/AC6 banks; `.wem` via vgmstream or ww2ogg+revorb. | rewwise: MIT OR Apache-2.0; vgmstream: ISC |
| DS3 formats | SoulsFormats and WitchyBND are **GPL-3.0**: cannot be bundled or ported; write own BHD5/DCX/BND4/FLVER/TPF code from format knowledge if needed. Soulstruct is GPL-3 too. | do not copy |
| DS3 runtime | `vswarte/fromsoftware-rs` crate **`darksouls3` 0.14.0** (MIT) has typed game structures + generated param structs (EQUIP_PARAM_WEAPON_ST ...) + RVA tables for DS3 **1.15.2.0 (WW)** and 1.15.2.1 (JP). **Builds for x86_64-pc-windows-gnu here** (21 s). | MIT: usable as a dependency |

Consequences: textures and sounds are tractable; weapon **numbers** partly (text `.cls`); **models** are the long pole on both sides (SM2 Saber parser that can break with patches, plus a DS3 FLVER/BND4/TPF writer
and an archive reader to modify an existing weapon file). The in-game side (granting items, editing params in memory) is much less blind thanks to `darksouls3`.

## Open questions (waiting on the owner's PC)

1. Kit 0.5: do the weapons show the names Chainsword / Bolt Pistol / Bolt Rounds (and the new descriptions)? Did `ds3-prepare` find the archive keys in the exe (fingerprints in `ds3-report.txt`)? Which `msg` folders and FMG ids exist (the report lists them)?
2. Kit 0.5: does the Avelyn burst play three shots; is the crossbow silent as a sword; did equipment reading work (log lines "equipment: ..." with the raw numbers)?
3. Kit 0.5: which sound set is better (F9): the classic mix or the exact reading? Does the exact reading cover the bank now (report line "reading the bank exactly: N of M")?
4. Kit 0.5: `mesh-report.txt` - how are the SM2 `.tpl` / `.tpl_data` files laid out; do the vertex/index finders hit; is `tpl_data` compressed?
5. Which weapon rows to repurpose permanently and how to name them for good (currently Shortsword -> Chainsword, Avelyn -> Bolt Pistol, Standard Bolt -> Bolt Rounds).

## Next

- Kit 0.5 -> owner -> logs. Then: names verified -> make weapon grant automatic (no hotkey); models: read `mesh-report.txt`, write the SM2 mesh reader, FLVER2/TPF writer for the DS3 weapon model files (`parts/wp_a_*.partsbnd.dcx`), convert textures (BC7/BC5 -> DS3 TPF); equip-aware idle loop,
  equip/unequip sounds, hit sounds.
- M4: Melty listing, release, one-click check, real screenshot, publish only with the owner's OK.
