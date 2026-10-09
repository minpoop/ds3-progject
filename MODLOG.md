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


## Kit 0.5: the Dark Souls III archive reader - what is verified and what is only assumed

Own implementation in `crates/ds3data` (no code from SoulsFormats, which is GPL-3.0; its sources were read for format facts only) plus `ashenmarine-setup ds3-probe` / `ds3-prepare`. Verified with synthetic installs (native and under Wine): every parser/writer
round-trips, `replace_file` with identical data gives identical bytes, garbage and truncated input never panics, nothing is ever written inside the game folder, a failed check writes nothing new and removes an override written earlier.
**Never run against a real Dark Souls III yet.** Assumptions the first real `ds3-report.txt` will confirm or refute (the report names each one in plain words when it fails):

1. The program file `DarkSoulsIII.exe` holds the archive keys as plain PEM text (else `cache/ds3-keys.pem`, written by the in-game probe from memory) and the right key for each `Data*.bhd` is among them (key chosen by decrypting the first block and finding `BHD5`).
2. Header = raw RSA public operation per 256-byte block, 255 output bytes each; BHD5 layout (0x1C header + salt, buckets, 40-byte DS3 file headers with u32 name hash, SHA/AES records, AES-128-ECB ranges); `.bdt` starts `BDF4`.
3. Path hash = trim, `\` to `/`, lower case, leading `/`, `h*37 + c` as u32; English item text is `/msg/ENGLISH/item.msgbnd.dcx` (the report also tries alternative spellings and lists what each hashes to).
4. `item.msgbnd.dcx` is DCX DFLT 10000_44_9 holding a BND4 (raw format 0x74, files in header order at one alignment, no per-file compression) whose FMG tables use version 2; the weapon names are exactly "Shortsword" (2000000), "Avelyn" (14090000), "Standard Bolt" (404000).
5. ModEngine2 prefers `<mod>/msg/ENGLISH/item.msgbnd.dcx` over the archived file and the game accepts the rewritten FMG layout (the report says whether the writer reproduces the game's own FMGs byte for byte; informational).

## Kit 0.5 results (owner's PC, 2026-10-09) - what they showed

- Owner: **"all sounds work, names did not change"** (the same logs: hand-aware swings, the Avelyn three-shot burst, equipment decoding and both sound sets behaved; set A or B was not chosen yet).
- Why the names did not change: `ds3-prepare` ran **before the game had ever been started with the kit** and the program file (`DarkSoulsIII.exe`, 84.8 MB, Steam build id 10167187, DS3 1.15.2.0 English) holds **no
  PEM key text at all** (0 `-----BEGIN` blocks), and `cache/ds3-keys.pem` did not exist yet - so "no archive key" was the right answer, not a bug. The Steam version of the exe is protected (its code and data are encrypted on disk).
  Archives seen: `Data0..Data5`, `DLC1`, `DLC2`; `.bhd` sizes 2.2 KB / 403 KB / 444 KB / 104 KB / 182 KB / 1.3 MB / 140 KB / 234 KB, `.bdt` 796 KB / 929 MB / 2.4 GB / 1.5 GB / 1.1 GB / 12.9 GB / 1.5 GB / 2.9 GB.
- The in-game scan for PEM text found **0 blocks at ~13 s** (title screen) and **1 block ~86 s after the start** (in the world, after the game tried to resolve `fdp-steam-ope-login.fromsoftware-game.net`, which the guard refuses). That one block was never
  tried against the archives (Prepare was not run again afterwards). It is most likely the key of the online login, not an archive key. Conclusion: the archive keys are not kept as PEM text, or only for a moment while the archives are opened at start.

## Kit 0.6 (this window) - the collector: getting the archive keys / tables of contents from the running game

- **Idea.** The game must read every archive header (RSA-"encrypted" tables of contents) itself, so something usable exists in its memory while it does. The hook now has a collector (`crates/hook/src/features/harvest.rs`) that starts at once,
  scans the process memory in passes (back to back for 30 s, then every 5 s until 3 min, then every 30 s, at most 15 min; low thread priority; stops as soon as every archive is covered or when the cache already covers them)
  and recognises, with `crates/ds3data/src/scan.rs`:
  1. RSA public keys as PEM (ASCII and UTF-16), bare base64, DER `RSAPublicKey` / `SubjectPublicKeyInfo`, Windows CNG / CryptoAPI key blobs;
  2. OpenSSL `BIGNUM` / mbed TLS `mpi` structures that point at a 2048-bit number, and 256-byte numbers lying next to the exponent 65537 (little-endian limbs or big-endian) - these candidates are **kept only if a real archive proves them**
     (the first encrypted block of a `.bhd` decrypts to `BHD5`), so nothing else the game holds is ever stored;
  3. a decrypted `BHD5` table of contents (contiguous or written in 256-byte slots): read whole, must parse completely, fit one `.bhd`'s block arithmetic and keep every file inside that archive's `.bdt`.
  Output: `cache/ds3-keys.pem`, `cache/bhd5/<archive>.bin`, `logs/harvest.txt` (fingerprints and places, never key text; per-pass counts; near misses with the first 32 bytes; places where the path hash of
  `msg/ENGLISH/item.msgbnd.dcx` / `menu.msgbnd.dcx` is followed by a plausible size and offset, with the neighbouring records - to learn a table layout if no whole table is found; which crypto libraries are loaded).
- `hook.log` now has a line **`ARCHIVE opened by the game [...]: Data1.bhd (+N ms after the hook loaded; ...)`** the first time the game opens each `Data*.bhd` / `.bdt` through one of the hooked file functions
  (`CreateFileW` & co.): it tells whether the game opens its archives before the hook is in place (then transient keys are gone and only things that stay in memory can be found; no such line at all = they were opened before the hook loaded or
  through a function that is not hooked). The collector also looks again at once when a new archive file is opened.
- Cost control: the hints about how the game holds its files (encrypted `.bhd` starts, path hashes with their neighbourhood) are looked for in the first 4 passes and then every tenth; a pass over 5 GB should take about 10 s.
- `ashenmarine-setup sm2-export-models` + `Send-Model-Files.bat` (**optional, asks Y/N**): copies the chainsword / bolt pistol template files (~3 MB) out of the player's own Space Marine 2 into a zip on the Desktop; nothing is uploaded; for building the
  mesh reader (the `.tpl` / `.tpl_data` formats cannot be learned from the numbers-only `mesh-report.txt`). Neither it nor the mesh probe ever writes inside the game folder (not even a report).
- `ashenmarine-setup ds3-prepare` / `ds3-probe` read the cache: keys from `cache/ds3-keys.pem`, tables from `cache/bhd5/*.bin` (`Archive::from_plain_header`, `PlainHeader`, `Ds3Install::open_with_sources`); a key wins over a table; the report says per archive
  which was used. The program file is now also searched for keys in the other shapes.
- `Play-AshenMarine.bat` runs the launcher **in the window** (it waits for the game to close) and then runs `ds3-prepare` by itself if no name file exists yet: names appear from the second Play.
- **Assumptions the first real `harvest.txt` will confirm or refute:** (a) keys or whole tables exist in memory at some moment within the first minutes; (b) a table in memory has the file layout (header at the start, `declared` size, offsets relative to it)
  or 256-byte slots; (c) a header's declared size lies between "fills the last block" and the `.bhd` size; (d) the 64-bit OpenSSL / mbed TLS structure layouts. If nothing is found, the log still says what was seen (counts per shape, near misses, path-hash neighbourhoods,
  encrypted `.bhd` starts in memory) - the next step would then be to hook the file reads of the `.bhd` files or to ask the owner whether the community-published public keys may be used.

## Kit 0.6 results (owner's PC, 2026-10-09) - what they showed

- Owner: **"names did not change"**, plus the optional model files (`wpn_chainsword_01.tpl`, `wpn_bolt_pistol_01.tpl`: 4 template files each) - see the SM2 section below.
- **The collector works.** In the first second after the hook loads, the game opens all 8 `.bhd` / `.bdt` pairs (`ARCHIVE opened by the game [fs_createfilew]` at +53 ms). Public RSA keys appear as **plain PEM text in the game's memory only for a moment**:
  `Data1`..`Data5` at about +1.3 s (all five caught in pass 4), `DLC2` at +25 s in one of the two sessions. Each key was proven by decrypting the first block of its `.bhd` to `BHD5`; 6 of 8 archives got a key. 63 key candidates were saved to
  `cache/ds3-keys.pem` (only proven ones are meant to be kept; unproven ones were kept because some blocks were unreadable). No whole decrypted table was found in memory (`table-like places 0`): the game parses the header and frees it.
- **`Data0.bhd` is not encrypted.** It is a plain `BHD5` file of 2212 bytes (not a multiple of 256, unlike the other seven), which is why no key ever matched ("no key matched (63 tried)"). `Data0.bdt` is 796 KB: it is probably the regulation (the parameter
  tables) plus a few small files. `DLC1.bhd` (139.8 KB) is the only archive that is still locked; its key was not seen (its encrypted start was in memory once, at the very beginning).
- Archive facts verified on the real game: `Data1` 2377 files / 347 buckets / salt 11, `Data2` 2344 / 337 / 7, `Data3` 721 / 103 / 9, `Data4` 963 / 137 / 7, `Data5` 7214 / 1031 / 7, `DLC2` 1264 / 181 / 8; `.bdt` of `Data5` starts `BDF4`, the others "UNEXPECTED" because
  they start with data (the check was only a hint; offsets in the `.bhd` are absolute).
- **The English item text was not found by the hash of `/msg/ENGLISH/item.msgbnd.dcx` (0x50b424bf) in any of the six archives that were open.** The hash rule is the one of SoulsFormats' `SFUtil.FromPathHash` (`reference read`: trim, `\`->`/`, lower case,
  leading `/`, `h*37 + c` as u32). *Correction to an earlier note:* the "path hash is in memory" lines of `harvest.txt` are the hook's **own** needle table (the 40 bytes after each hit are a Rust `(u32, &str)` array with a pointer into the hook DLL), not
  evidence about how the game keeps its files.
- Candidates for why: (a) the text is in `Data0` or `DLC1`; (b) it has another name/hash than assumed. Kit 0.7 answers it from the data instead of guessing (see below).

## Kit 0.7 - plain Data0, finding the text by what it contains

- `Archive::open` accepts a `.bhd` that already starts with `BHD5` (`HeaderSource::Plain`): no key needed. `Data0` should now open; the archive line says "plain header". Per-archive report lines also print the bucket check (`hash % buckets` equals the bucket the
  file is listed in, for all files if the table was read right).
- **Content search** (`crates/ds3data/src/discover.rs`): when the path hash finds nothing, `ds3-prepare` reads the first 16 KB of every file (4 KB - 24 MB stored size) of every opened archive in disk order (AES ranges decrypted), checks `DCX` + DFLT, inflates the first
  64 KB (streaming, `dcx::peek`), reads the BND4 header and the names of its files (`bnd4::peek`), and keeps the containers with `.fmg` names. The English item text is then chosen **by its text** (the same `patch_item_msgbnd_detailed` check as the real run:
  "Shortsword" at id 2000000, "Avelyn" at 14090000, "Standard Bolt" at 404000), whatever its hash. Time budget 240 s. `ds3-probe` always runs the search and lists every text container with its hash (and the path it would be, if it matches a known
  `msg/<language>/<file>.msgbnd.dcx`), per-archive counts, and what the files start with (`DCX>BND4 1200, TPF. 30, ...`) - this teaches the archive layout.
- If the found container's hash is not that of `/msg/ENGLISH/item.msgbnd.dcx`, the override is still written to `msg/ENGLISH/item.msgbnd.dcx` (the path every DS3 text mod uses) and the report says so loudly. If the game then still shows the old names, the hash rule or
  the name is what to fix next.
- If the text is in no opened archive, the message names the archives that could not be opened ("probably in DLC1").
- `hook.log` gets a line **`MOD FILE opened by the game [...]: <path>`** the first time the game opens a `.dcx` below a folder called `mod` (what ModEngine2 serves instead of an archived file) or any `*.msgbnd.dcx` from disk: if the new item text is
  there after Play, ModEngine2 does serve it; if the names still do not change, the game reads the text from somewhere else or replaces it later.
- The collector pauses less in the middle phase (1.5 s between passes from 30 s to 2 min, 10 s up to 5 min) to improve the chance of catching `DLC1`'s key.
- **Still assumed (first real data will tell):** the DCX header constants of the item text (`DCX_DFLT_10000_44_9`), BND4 raw format 0x74 with files in order at one alignment, FMG version 2, `ModEngine2` picking up `mod/msg/ENGLISH/item.msgbnd.dcx`, and that the `.fmg` names the container's files carry (the search keys on the extension).

## Space Marine 2 model files (`.tpl` + `.tpl_data`): what is verified on the owner's chainsword and bolt pistol

Own reader in `crates/sm2/src/tpl.rs` (template) and `crates/sm2/src/mesh.rs` (vertices and triangles); the layout facts were cross-checked against the published community notes (Saber "1SER" lists) and, above all, **against the real files**:
both templates read to the last byte (every chunk that names its end offset ends exactly there), and the decoded sub meshes render as the recognisable chainsword / bolt pistol (`examples/mesh_preview` writes an OBJ and a three-view PNG; `examples/tpl_dump` prints everything read).

- `.tpl`: 0x40-byte header (`1SER`, `tpl\0`, counters, a 16-character id `S3DRESOURCE`, header strings = none), then `TPL1`, a flag set (`i32` bit count + bytes; chainsword/pistol: bits 0 name, 2 state, 5 skin, 6 track animation, 8 bounding box, 9 LOD
  definitions, 10 texture list (empty), 11 geometry graph). Skin: bone count (78 / 30) and the number of bones each LOD keeps (78, 63, 48, 33, 18 / 30, 24, 18, 12, 6); no inverse bind matrices in weapons. Track animation: sequence names (`anim1`, `chain_anim`, `finisher1`, `IDLE`,
  `RELOAD_FULL`, `SHOOT_1`, ...) plus object animations and splines (read by their end offsets; not used yet).
- Geometry graph `OGM1`: header word (low 16 bits = number of properties; above 8 the flag word is 16 bits), then property lists (objects with names, parents, children, matrices; named objects; matrices; split info; object properties = 16 bytes each) and data sentinels
  (`u16` id + `u32` absolute end offset): 0 header (root node, node / buffer / mesh / sub mesh counts), 5 references (skipped), 2 buffers, 3 meshes, 4 sub meshes, 0xFFFF end. Buffers: flags (16-bit bit count + bytes), strides, lengths; **the `.tpl_data` file is exactly the buffers
  one after the other in this order** (sizes add up to the file size). Per LOD four buffers: positions (stride 8: 3 x i16 + packed normal i16), faces (stride 6: 3 x u16), bone numbers (stride 4: 4 x u8; teeth = bones 63..77 in the chainsword), interleaved (stride 8: compressed tangent 4 x i8 and
  uv 2 x i16, or 12 with a colour). A mesh lists (buffer, byte offset inside it); a sub mesh = (first vertex, vertex span, first face, face count) where **face indices are absolute inside the mesh's vertex range** (the 14 chain teeth interleave their vertices: tooth i uses 7580 + i + 14 k).
  Positions = `i16 / 32767 * scale + position` per sub mesh (the `i16` transform values are in metres: scale (1, 1, 1) gives the chainsword's 1.7 m length; positions are relative to the object, centred), uv = `i16 / 32767 * uv scale`, v flipped. Sub mesh 90 (chainsword, 11388 vertices, 14834 triangles) and 38 (bolt pistol,
  7721 / 9640) are the full-detail static meshes; the next ones halve the triangles (LOD 1..5). Materials are typed property lists (`shadingMtl_Tex`, `layer0 = {texName, tint, tiling, blending, ...}`) - the textures `wpn_chainsword_01`, `chainsword_blade_01`, `shp_sc_grey_98` are separate `.pct` files (not in the sent model files).
- Not done yet: textures, the animated teeth (own objects with matrices), tangent / bone weights use, the DS3 side (FLVER2 + TPF writer).

## Kit 0.8 - the weapon model swap: what is built, what is assumed

**Built (all tested with made-up data; Wine suites cover the shipped programs):**

- `ds3data::flver` / `tpf` - FLVER2 (0x20013/0x20014) and PC TPF readers/writers that give back the very same bytes; `ds3-probe` step 7 reports the structure of `wp_a_0200` (Shortsword) and `wp_a_1409` (Avelyn): bones, dummies, materials with texture slots,
  layouts, per-member vertex statistics (unit-length tests of the 4-byte members, ranges of floats and shorts, raw first vertices), and "written again it is byte-identical" for the model and the textures. `ds3-export-models` (optional, `Send-Model-Files.bat`)
  copies the five containers (`wp_a_0200`, `_0200_l`, `_1404`, `_1409`, `_1419`) as stored; `sm2-export-models` now also copies the textures the full-detail material names.
- `ds3data::vertex` (one vertex of a layout: positions, normals, tangents, bitangents, uvs in every storage type, bone numbers/weights, colours; writing starts from a template vertex so padding bytes stay the game's), `modelswap` (new shape into the biggest mesh; keeps
  bones, dummy points, materials, layout; **first checks that the community's vertex notes fit the game's own vertices and that re-encoding them gives the same bytes**; reads the result back), `dds` (RGBA, area-average resize, mip chains, BC1/BC3/BC4/BC5 encoders, plain-colour
  textures in every format incl. BC7 as a constant block, mean-colour reader), `weaponswap` (the whole container: model, colour map from a picture, the other maps as the average colour of the map they replace, all other files carried over; refuses unless the writers
  reproduce the game's own files exactly). `setup::weaponmodel` + `ds3-models` command (trial run by default; `--install` writes `{mod}/parts/*.partsbnd.dcx` and `ashenmarine-models.json`, removing an earlier run's files first).
- SM2 side facts (real files): the templates keep the weapon twice - as separate parts (base + 14 chain teeth / base + magazine + bolt + cartridge, each with LODs) and as **one merged static mesh per level whose object is the one `LodDef` index 0 names** (object 120 -> sub mesh 90 for the
  chainsword, 14834 triangles = base 10326 + 14 x 322 teeth; object 52 -> sub mesh 38 for the pistol, 9640 triangles). That merged mesh is "the full-detail model" (`Template::full_detail_sub_meshes`). It sits in the space of its parts, displaced from the model space (hand at the origin) by the
  centre of the `rb_wpn` rigid body box minus the mesh centre (chainsword (-0.029, -0.019, +0.62), checked against the matrices of `chainsword_base_geo`), which is how the grip is found. The chainsword is 1.71 m long (z), the pistol 0.67 m.

**Assumed until real DS3 data is seen (kit 0.8 asks for it; every assumption is a check that fails closed):** member type meanings and the uv factor 2048 (`vertex.rs`), which byte of a 4-byte vector is padding (kept from a template vertex, never invented), the texture formats of the weapon maps and
what the engine does with a different mip count or a different DDS flavour, whether `bnd4::replace_file` accepts the real containers' layout, that ModEngine2 serves `mod/parts/*.partsbnd.dcx`, the axis/side/scale fit (longest axis to longest axis, bulk side from the centroid against the grip, same length as the weapon replaced,
proper turn only), the winding fix, and that the picture's v must be flipped for DS3 (`FLIP_V`). The first in-game test will show orientation, scale and texture side; each is one constant.

## Kit 0.8 results (owner's PC, 2026-10-09) - what they showed

- **A real bug of mine, found only by real data:** the entry of a BHD5 archive has a field "unpadded size" that is `0` (or `-1`) when the size is "not given" - every entry of the real `Data1`..`Data5` has `0`. The reader took `0` as "the file is empty", so every file of every opened archive read as under 4 bytes ("what the files start with: (under 4 bytes) 5925, ENFL 703 ..."), no weapon container could be read ("not a DCX file") and the content search found nothing. `Entry::unpadded()` now trusts the field only inside `1..=padded size`; regression test with entries of 0, -1 and a real number. (The fake archives of the tests always carried a real number, which is why nothing caught it.)
- **The language folder is `engUS`, not `ENGLISH`** (path hash of `/msg/engUS/item.msgbnd.dcx` = 624f014f; the hash ignores case). Evidence: the hook log of kit 0.8 shows ModEngine2 asking the file system for `<mod>\msg\engus\ngword.msgbnd.dcx`, `item_dlc2`, `menu_dlc2` and `<mod>\msg\na\sellregion.msgbnd.dcx`. Neither the `engUS` nor the `ENGLISH` item text is in `Data1`..`Data5` (5 keys: a801eed5, b23f5bb3, e04b5de5, 3649ce9d, a1336a2d, all found as PEM text within 1.3 s), so it is in `Data0`, `DLC1` or `DLC2` - or the game does not read it from an archive path at all.
- **Keys of the other archives:** `DLC2`'s key (36555232) was seen in kit 0.6 about 25 s into a longer session (it opened `DLC2`); `DLC1`'s never; a sixth PEM key (5a41549b) appears ~29 s after the start in kits 0.6 and 0.8 and opens none of `Data0`/`DLC1`/`DLC2` by the "decrypts to BHD5" test. The kit 0.8 session lasted 70 s, so nothing late could be seen. `Data0.bhd` is 2212 bytes, starts `49 4f 74 8f`: not `BHD5`, not a whole number of 256-byte blocks - unexplained.
- **How ModEngine2 finds loose files** (read from its public source, `mod_loader/archive_file_overrides.cpp`): it hooks the game's `virtual_to_archive_path` (a fixed RVA, 0x7d660, of the DS3 exe) and, for results that look like `dataN:/...`, asks `fs::exists(<mod>/<path>)` once per distinct path (cached), rewrites the path to `.///////<path>` so that the game opens it from the disk, and hooks `kernel32!CreateFileW` to redirect that open into the mod folder. The hook log of kit 0.8 shows such an existence check for exactly four files (the `msg` ones above) and for no `parts`, `chr` or `map` file during 70 s: so either most files do not take this path in this build or the hook point does not match the owner's exe. **Unknown; kit 0.9 logs every path below the mod folder (any ending, 300 distinct) and installs the model files, which settles it.**
- **How the game keeps its text in memory** (probe logs of kits 0.4/0.5): every name is its own zero-ended UTF-16 string in a heap block, at an address divisible by 8, with an 8-byte block header in front (not a length; in one scan a pointer to itself, in another other bytes), many pointers to it; no FMG table is in memory ("found 0 text tables that parse as FMG"); the strings are built again - at other addresses - when a world loads ("Shortsword" 0x1CD775BBC10 on the title screen, 0x1CD76DC4648 in the world).
- The Space Marine side is complete: templates read to the last byte, textures decode, the sound sets work, the F8 items and the burst work.

## Kit 0.9 - the names inside the game, the first real models, more about the archives that do not open

- **`ds3data`/`setup`:** archives `engUS` paths (`/msg/engUS/item.msgbnd.dcx`, override file `mod/msg/engus/item.msgbnd.dcx`, 16 PC language folders, the files the game was seen asking for in the path table); `look_at_bhd` + `describe_unopened`: for every `.bhd` that no key opens, the report lists its first and last 64 bytes (the whole file when it is under 4 KB) and tries EVERY key that is known (the program file's, `cache/ds3-keys.pem`, the new `cache/keys-seen.pem`) on the first 256-byte block: a key that is right for an archive turns every block into a plain block (a wrong one does so for one block in about 128) whatever the header looks like, so "the key is right but the header is laid out differently" shows up. `ds3-export-models` (optional zip) also copies those small `.bhd` files and the public keys.
- **`hook` collector:** saves every key the game keeps as PEM text (not only the ones that open an archive) and writes every distinct key seen to `cache/keys-seen.pem`; the log of mod-folder paths covers every file ending, 300 distinct paths.
- **`hook` rename (new, `--rename`, `logs/rename.txt`):** a thread searches the process's own private writable memory for the old names as WHOLE strings (exact UTF-16 text, a zero right after, no printable ASCII character right before, address divisible by 8; its own copies skipped) and overwrites each in place - `WriteProcessMemory`, never longer than the old text, shorter names padded with spaces (`Shortsword` -> `Chainsword`, `Avelyn` -> `Bolter`, `Standard Bolt` -> `Bolt Rounds  `; the list comes from the weapons sheet: `live_name`, `ammo_*`). Repeats every 3 s until something is written, then every 5 s for 3 minutes, then every 20 s (all of memory every fourth pass, the places that had a find in between), because the strings move when a world loads. Every find is logged with the 32 bytes before and the 64 after, so the size of the block and the header can be studied: **"Bolt Pistol" (11 characters) needs room that is not known to exist** - if the bytes after the zero turn out to be padding of the same block, a longer name becomes possible.
- **`hook` me2check (new, with `--probe`, `logs/me2-hook.txt`):** twice (5 s and 45 s after the start) a read-only look at ModEngine2's footprint in the program file: the bytes at the two offsets its source names (0x7D660 `virtual_to_archive_path` hook; 0xEA1B83 five NOPs), where a jump there leads (by module name: a hook from `modengine2.dll` or not), and the counts of its loose-parameter signatures in original and patched form. Settles whether ModEngine2's loose-file hook is where it expects it in the owner's build, which the missing `mod\parts` lookups in the kit 0.8 log put in doubt.
- **`ds3-models --install` is offered in Prepare** (Y by default after 30 s) when the trial passed every check, `ds3-models --remove` / `Remove-Models.bat` takes the installed files out again (only the ones the manifest lists).
- Versions: 0.9.0. Sheets: `weapons` (live names), `systems` (`item_names_live`), `files` (`rename_log`, `ds3_header_copies`, `ds3_keys_seen`).

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

1. Kit 0.6: did the collector find keys or tables of contents (`harvest.txt`: "it OPENS ...", "TABLE OF CONTENTS of ... found")? Do the weapons show the names Chainsword / Bolt Pistol / Bolt Rounds after the second Play? Which `msg` folders and FMG ids exist (`ds3-report.txt`)?
2. Kit 0.5: does the Avelyn burst play three shots; is the crossbow silent as a sword; did equipment reading work (log lines "equipment: ..." with the raw numbers)?
3. Kit 0.5: which sound set is better (F9): the classic mix or the exact reading? Does the exact reading cover the bank now (report line "reading the bank exactly: N of M")?
4. Kit 0.5: `mesh-report.txt` - how are the SM2 `.tpl` / `.tpl_data` files laid out; do the vertex/index finders hit; is `tpl_data` compressed?
5. Which weapon rows to repurpose permanently and how to name them for good (currently Shortsword -> Chainsword, Avelyn -> Bolt Pistol, Standard Bolt -> Bolt Rounds).

## Next

- Kit 0.6 -> owner -> logs (`harvest.txt`, `ds3-report.txt`, names after the second Play). Then: names verified -> make weapon grant automatic (no hotkey); models: read `mesh-report.txt`, write the SM2 mesh reader, FLVER2/TPF writer for the DS3 weapon model files (`parts/wp_a_*.partsbnd.dcx`), convert textures (BC7/BC5 -> DS3 TPF); equip-aware idle loop,
  equip/unequip sounds, hit sounds.
- M4: Melty listing, release, one-click check, real screenshot, publish only with the owner's OK.
