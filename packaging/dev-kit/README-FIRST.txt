ASHEN MARINE - private test kit 9 ("the names inside the game, and the first real models")
=======================================================================================

What this is
  Your Dark Souls III character with Space Marine 2's chainsword and bolt pistol sounds, and - this kit - the first
  attempt at the NAMES and the LOOKS of the two test weapons:
    - the names are written into the running game itself (it needs no archive for that), so the Shortsword should now
      be called Chainsword, the Avelyn Bolter, and the Standard Bolts Bolt Rounds;
    - the program builds the Space Marine 2 chainsword and bolt pistol as Dark Souls III weapon models, checks them in
      every way it can, draws a picture of the result and - if every check passes and you say yes - puts them into the
      game, so the two weapons may LOOK like the Space Marine 2 ones.
  It is a private test kit, so it also writes logs that tell me how to make it better.

  Everything runs on your PC. Nothing is uploaded: you send me the files yourself at the end. Dark Souls III runs as an
  OFFLINE COPY of your save (your real save is backed up first and checked afterwards). Space Marine 2 and the
  Dark Souls III game files are only READ, never changed. The only things this kit puts into Dark Souls III's world are
  files in ashenmarine\mod (the new weapon models, if you say yes) and the changed names in the game's memory while it runs.

What went wrong in kit 8, and what is different now
  Your kit 8 logs showed two mistakes of mine:
    1. My program read some of the game's files as EMPTY (it misread a size field), so it could not open the weapon files
       at all. Fixed - and a test now covers it.
    2. I looked for the English text in a folder called ENGLISH; the game calls it engUS. Fixed.
  Three of Dark Souls III's archives (Data0, DLC1, DLC2) still do not open, and the item text is probably in one of
  them. Instead of waiting for that, this kit also changes the three names inside the running game. And it writes much
  more about those three archives into its reports, so that I can find out how to open them.

How to do it (about 15-20 minutes)
  1. Unzip this whole folder into the SAME place as the older kit folders (e.g. all in Downloads), as a new folder
     next to them. Do not delete the older ones yet (this kit takes over the keys they collected). Steam must be running
     and signed in; Dark Souls III must NOT be running.
  2. Double-click  Prepare-AshenMarine.bat  and wait for "Done". It does five things, one after the other:
       - makes the sound files from your Space Marine 2 (a minute or two),
       - tries to make the item-name FILE from your Dark Souls III. It may well say it could not (the text is probably in
         an archive that does not open yet). THAT IS OK NOW: the names come from the game's memory instead,
       - writes a report about where Dark Souls III keeps its files (no copies of them),
       - writes the Space Marine 2 model report (a minute),
       - STEP 5, THE MODELS (a minute or two): the new weapon models are built in memory and checked. LOOK AT THE LAST
         LINES: "N model file(s) made, M not made". If something was made it asks:
             Put the new models into the game now (Y = yes, N = no)
         Press Y (it also chooses Y by itself after 30 seconds). If the game ever crashes or a weapon looks wrong,
         double-click  Remove-Models.bat  : that takes them out again and the game shows its own weapons.
  3. Open the pictures in  ashenmarine\ds3-models  with a double-click:  wp_a_0200-overlay.png  (the chainsword) and
     wp_a_1409-overlay.png  (the bolt pistol); the names ending in _l are the left-hand versions. Each shows three views
     side by side (front, side, top). ORANGE is the outline of the Dark Souls III weapon that is being replaced; GREY is
     the Space Marine 2 weapon, turned and scaled to fit it.
  4. Double-click  Play-AshenMarine.bat  . Dark Souls III starts; the title bar reads
     "DARK SOULS III - Ashen Marine (offline copy)".  (A question about connecting online is expected: it is blocked
     on purpose.)  Load your normal character, then:
       a. Wait about 20 seconds, stand still (not in a menu) and press  F8  ONCE. Three test items go into your
          inventory (this kit folder has its own private copy of your save; this is the offline copy only). Open the
          inventory - Weapons and Ammunition. WHAT ARE THEY CALLED? (If they still have the old names, wait half a
          minute and look again: the program looks for the names every few seconds.)
       b. Equip them as before (the first in your right hand, the second in your left hand, the bolts in a bolt slot).
          LOOK AT THE WEAPONS IN YOUR CHARACTER'S HANDS: do they look like the Space Marine 2 chainsword and bolt pistol,
          like the old Shortsword and Avelyn, or something strange (invisible, huge, tiny, stretched, one flat colour)?
          If you can, take a screenshot of each (Windows key + Shift + S), with the weapon in hand.
       c. Swing and fire for a minute: the sounds should work as before. F9 switches between sound sets A and B.
       d. STAY IN THE GAME FOR AT LEAST 4 MINUTES after your character has loaded (walk around a little). The helper
          that collects the archive keys needs time, and so does the name program.
       e. Quit to the desktop from the in-game menu as usual. Right after that, the program looks again at the game's
          files (a minute or two; the window says so).
  5. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop (the reports, the logs and the
     pictures of the trial run). Attach it to the chat, with your screenshots, and tell me:
       - what are the three items called in the inventory?
       - how do the two weapons look in your hands?
       - what did step 5 of Prepare say, and did the game crash?
  6. OPTIONAL: double-click  Send-Model-Files.bat . It copies into ONE zip on your Desktop (AshenMarine-model-files.zip):
       - the chainsword and bolt pistol model files of your Space Marine 2 (with the pictures they use),
       - five weapon model files of your Dark Souls III,
       - the small INDEX files (.bhd, 2 KB to 250 KB; only an index of an archive, no content) of the Dark Souls III
         archives that do not open yet, and the PUBLIC keys the program saw in the running game (they are not secrets: they
         sit in the game's own memory).
     You do not have to: nothing is sent unless YOU attach the zip to the chat. If you do, I can try things on exactly your
     files here. The finished mashup never contains any Space Marine 2 or Dark Souls III file.

What it records (you can read every file yourself)
  ashenmarine\ds3-prepare\ds3-report.txt       what was read from Dark Souls III's files for the names, and what was found; for every
                                               archive that does not open: its first and last bytes and which keys were tried
  ashenmarine\ds3-probe\ds3-report.txt         where Dark Souls III keeps the files the mod needs (file names and sizes only), and how the
                                               model and texture files of two weapons are built (structure only, no copies)
  ashenmarine\ds3-models\ds3-models-report.txt the model trial run: every check, and every number of the new shapes
  ashenmarine\ds3-models\*-overlay.png         the pictures of the trial run (drawn by the program; no game file is in them)
  ashenmarine\logs\rename.txt                  what the name program found in the game's memory and what it wrote (short strings
                                               and the bytes around them; nothing private)
  ashenmarine\logs\harvest.txt                 what the helper looked for in the game's memory and what it found
                                               (key fingerprints and places only - never the keys themselves)
  ashenmarine\logs\me2-hook.txt                a read-only look at what ModEngine2 left in the game's program file (a few bytes at
                                               two places, and counts of a few byte patterns): it tells me whether the new weapon
                                               models can be loaded from the mod folder in this build of the game
  ashenmarine\prepare-sm2\prepare-report.txt   what was read from Space Marine 2 and how it became sound files
  ashenmarine\probe-sm2-mesh\mesh-report.txt   how Space Marine 2 stores the chainsword / bolt pistol models (and which pictures they use)
  ashenmarine\logs\sfx.txt, sfx-trace.csv      every sound that played, what was in your hands, stamina / buttons /
                                               ammunition (this is how I tune what counts as a swing or a shot)
  ashenmarine\logs\probe-ds3.txt               game build, parameter tables, the beeps
  ashenmarine\logs\hook.log, launcher.log      what the safety helper did (hook.log also lists the files the game asks for
                                               in the mod folder: "MOD FILE asked for by the game" - that tells me whether the
                                               game loads the new weapon models from there)
  ashenmarine\mod\ashenmarine-models.json      which model files were put into the game (names, sizes, hashes)
  Send-Logs.bat also lists the NAMES and SIZES of the files in ashenmarine\cache (not their contents).
  Your Windows user name and Steam id are masked in the copies Send-Logs makes. Your character's NAME is not logged.
  The sound files, the models and the cache stay on your PC (they are made from your copies of the games).

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - "Dark Souls III ran, but Ashen Marine's protection never started": Steam was probably not running. Start Steam,
    wait until it is ready, close Dark Souls III, press Play again.
  - Dark Souls III crashes: that is useful information - run Send-Logs.bat anyway and tell me roughly what you were
    doing (equipping a weapon? the first minute?). If it crashed when you equipped a new weapon, double-click
    Remove-Models.bat before you start the game again. Your real save was not changed (it is backed up and checked each time).
  - A weapon looks wrong: that is a result, not a failure. Send a screenshot and the logs. Remove-Models.bat takes the new
    models out again.
  - No sound at all: check that Prepare-AshenMarine.bat said it prepared the sounds, then look in
    ashenmarine\logs\sfx.txt (it says why sounds are off). Tell me what it says.
  - The weapons still have their old names after a minute in the world: run Send-Logs.bat and tell me.
    ashenmarine\logs\rename.txt says exactly what the name program found.
  - Prepare says "NOT READY YET" in step 2, 3 or 5: you have no key collection yet (no older kit folder was found). Start the
    game once with Play-AshenMarine.bat, quit it, and run Prepare-AshenMarine.bat again.
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Backups of your real save are in ashenmarine\backups.

Where things are
  ashenmarine\assets\sounds        the sound files made from your Space Marine 2, set A (private; safe to delete)
  ashenmarine\assets\sounds-exact  set B (the game's own volumes and delays), if it could be made
  ashenmarine\cache                what the helper saved from the running game (keys for the archives); private, safe to delete
  ashenmarine\mod                  what the game loads instead of its own files: the new weapon models in  mod\parts  (only if
                                   you said yes), and the item-name file in  mod\msg  (only if it could be made)
  ashenmarine\ds3-models           the model trial run: its report, the pictures and the candidate files (made from your copies
                                   of the games; private, safe to delete; the game does not load these)
  ashenmarine\model-files          only if you ran Send-Model-Files.bat: the copies that went into the zip (safe to delete)
  ashenmarine\save                 the private copy of your save (what the test plays; F8's items live only here)
  ashenmarine\backups              backups of your REAL save (original = first ever, session-* = newest three)
  ashenmarine\logs                 launcher.log, hook.log, harvest.txt, rename.txt, sfx files and probe files
  modengine2\                      ModEngine2 2.1.0 (MIT license) - the loader Melty will install for you in the real release

Good to know
  - If it cannot find Dark Souls III by itself, put the game's folder (the one that contains Game\DarkSoulsIII.exe)
    on the first line of a new text file  ashenmarine\game-folder.txt  and run it again.
  - If it cannot find Space Marine 2, put its folder on the first line of  ashenmarine\sm2-folder.txt .
  - The private save copy is a snapshot of your character taken the first time you press Play. To start a fresh
    copy of your current real character later, close the game and delete the  ashenmarine\save  folder.
  - Keyboard and mouse: the mouse buttons only count while the game window is in front. The sounds assume the game's
    default layout (left button = right-hand attack, Shift + left button = strong attack, right button = left-hand
    weapon). If yours is different, tell me which buttons - that is easy to change.
  - The bolt pistol is called "Bolter" for now: the game's own text for the Avelyn is six letters long, and the program only
    writes names that are not longer than the old ones (a longer one could run into whatever comes after it). "Bolt Pistol"
    needs a safer way, which I will work on once I can see what your game's memory looks like around those names.
  - This kit never goes online and never modifies any game file. What it adds to the game: files in ashenmarine\mod (loaded
    through ModEngine2) and three names in the game's memory while it runs.
  - It is an offline sandbox. Do not use it to play online.
