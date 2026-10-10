ASHEN MARINE - private test kit 10 ("the names and the weapon models, for real this time")
==========================================================================================

What this is
  Your Dark Souls III character with Space Marine 2's chainsword and bolt pistol sounds, and - this kit - the NAMES and the
  LOOKS of the two test weapons:
    - the Shortsword should be called Chainsword, the Avelyn Bolt Pistol, and the Standard Bolts Bolt Rounds;
    - the program builds the Space Marine 2 chainsword and bolt pistol as Dark Souls III weapon models, checks them in every
      way it can, draws a picture of the result and - if every check passes and you say yes - puts them into the game, so
      the two weapons should LOOK like the Space Marine 2 ones.
  It is a private test kit, so it also writes logs that tell me how to make it better.

  Everything runs on your PC. Nothing is uploaded: you send me the files yourself at the end. Dark Souls III runs as an
  OFFLINE COPY of your save (your real save is backed up first and checked afterwards). Space Marine 2 and the
  Dark Souls III game files are only READ, never changed. The only things this kit puts into Dark Souls III's world are
  files in ashenmarine\mod (the new names and, if you say yes, the new weapon models).

What went wrong in kit 9, and what is different now
  Your kit 9 logs showed three mistakes of mine, all found and fixed:
    1. THE NAMES: the game asks ModEngine2 for a text file called item_dlc2.msgbnd.dcx (your logs show it asking for
       exactly that). Kit 9 wrote its changed text under another name, item.msgbnd.dcx, which the game never asks for - so it
       was never used. Now the changed text is written as item_dlc2.msgbnd.dcx (and as item_dlc1 too, which your game also
       has). The "write the names into the game's memory" helper of kit 9 is gone: it never worked and is not needed.
    2. THE MODELS, part 1: the Shortsword's file holds TWO models, the sword and its scabbard, and the program refused to
       touch a file with two models. Now it knows which one is the sword (the one named like the picture file) and shrinks
       the scabbard to a dot, because the Space Marine 2 chainsword has none.
    3. THE MODELS, part 2: the program wrote a wrong number into the header of the model files, so its own safety check
       ("does writing the file again give exactly the game's bytes?") said no for every model. Fixed, and I checked it on
       the real weapon files you sent me: all of them come out byte for byte the same now, and the whole swap runs here on
       your chainsword and bolt pistol files without a complaint.
  Also new: the game's log (logs\hook.log) now lists the files it asked for in the mod folder AND the ones it really opened
  from there ("MOD FILE OPENED"), and logs\launcher.log ends with one line saying how many files the game loaded from the mod
  folder. That settles at once whether the game used the new names and models.

How to do it (about 15 minutes)
  1. Unzip this whole folder into the SAME place as the older kit folders (e.g. all in Downloads), as a new folder
     next to them. Do not delete the older ones yet (this kit takes over the keys they collected). Steam must be running
     and signed in; Dark Souls III must NOT be running.
  2. Double-click  Prepare-AshenMarine.bat  and wait for "Done". It does five things, one after the other:
       - makes the sound files from your Space Marine 2 (a minute or two),
       - STEP 2, THE NAMES: reads your Dark Souls III item text and writes the changed copies. LOOK AT THE LAST LINES: it should
         say "Written: msg\engus\item_dlc2.msgbnd.dcx, msg\engus\item_dlc1.msgbnd.dcx and ashenmarine-msg.json",
       - writes a report about where Dark Souls III keeps its files (no copies of them),
       - writes the Space Marine 2 model report (a minute),
       - STEP 5, THE MODELS (a minute or two): the new weapon models are built in memory and checked. LOOK AT THE LAST
         LINES: "N model file(s) made, M not made" (I expect 4 made: the chainsword and the bolt pistol, each for the right
         and the left hand; a missing left-hand file is only a note). If something was made it asks:
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
          inventory - Weapons and Ammunition. WHAT ARE THEY CALLED?
       b. Equip them as before (the first in your right hand, the second in your left hand, the bolts in a bolt slot).
          LOOK AT THE WEAPONS IN YOUR CHARACTER'S HANDS: do they look like the Space Marine 2 chainsword and bolt pistol,
          like the old Shortsword and Avelyn, or something strange (invisible, huge, tiny, stretched, one flat colour, a
          sword hanging at your hip)? If you can, take a screenshot of each (Windows key + Shift + S), with the weapon in hand.
          Try the left hand, too: the left-hand versions are separate files.
       c. Swing and fire for a minute: the sounds should work as before. F9 switches between sound sets A and B.
       d. Walk around a little (a minute or two is enough this time).
       e. Quit to the desktop from the in-game menu as usual. Right after that, the program looks again at the game's
          files (a minute or two; the window says so).
  5. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop (the reports, the logs, the pictures of
     the trial run and a list of the files in the mod folder). Attach it to the chat, with your screenshots, and tell me:
       - what are the three items called in the inventory?
       - how do the two weapons look in your hands (and in the left hand)?
       - what did step 2 and step 5 of Prepare say, and did the game crash?
  6. You do NOT need Send-Model-Files.bat this time (it is still in the folder; it copies a few of the game's model files
     into a zip, only if you attach it yourself).

What it records (you can read every file yourself)
  ashenmarine\ds3-prepare\ds3-report.txt       what was read from Dark Souls III's files for the names, what was changed and what was
                                               written (and, for the game's text tables, how the game's own layout differs from what
                                               this program writes: the first differing bytes - that is how I check the layout)
  ashenmarine\ds3-probe\ds3-report.txt         where Dark Souls III keeps the files the mod needs (file names and sizes only), and how the
                                               model and texture files of two weapons are built (structure only, no copies)
  ashenmarine\ds3-models\ds3-models-report.txt the model trial run: every check, and every number of the new shapes
  ashenmarine\ds3-models\*-overlay.png         the pictures of the trial run (drawn by the program; no game file is in them)
  ashenmarine\logs\hook.log                    what the safety helper did; it also lists the files the game asks for in the mod folder
                                               ("MOD FILE asked for by the game") and the ones it opened from there ("MOD FILE OPENED")
  ashenmarine\logs\launcher.log                what the launcher did, ending with how many mod-folder files the game opened
  ashenmarine\logs\me2-hook.txt                a read-only look at what ModEngine2 left in the game's program file
  ashenmarine\logs\harvest.txt                 what the helper looked for in the game's memory (key fingerprints and places only - never
                                               the keys themselves)
  ashenmarine\prepare-sm2\prepare-report.txt   what was read from Space Marine 2 and how it became sound files
  ashenmarine\probe-sm2-mesh\mesh-report.txt   how Space Marine 2 stores the chainsword / bolt pistol models (and which pictures they use)
  ashenmarine\logs\sfx.txt, sfx-trace.csv      every sound that played, what was in your hands, stamina / buttons / ammunition
  ashenmarine\logs\probe-ds3.txt               game build, parameter tables, the beeps
  ashenmarine\mod\ashenmarine-msg.json         which text files were written (names, hashes) and what they changed
  ashenmarine\mod\ashenmarine-models.json      which model files were put into the game (names, sizes, hashes)
  Send-Logs.bat also lists the NAMES and SIZES of the files in ashenmarine\cache and ashenmarine\mod (not their contents).
  Your Windows user name and Steam id are masked in the copies Send-Logs makes. Your character's NAME is not logged.
  The sound files, the models and the cache stay on your PC (they are made from your copies of the games).

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - "Dark Souls III ran, but Ashen Marine's protection never started": Steam was probably not running. Start Steam,
    wait until it is ready, close Dark Souls III, press Play again.
  - Dark Souls III crashes: that is useful information - run Send-Logs.bat anyway and tell me roughly what you were
    doing (equipping a weapon? the first minute? opening the inventory?). If it crashed when you equipped a new weapon, double-click
    Remove-Models.bat before you start the game again. Your real save was not changed (it is backed up and checked each time).
    If the game crashes already at the main menu or when the inventory opens, the changed text is the suspect: delete the
    folder ashenmarine\mod\msg and tell me.
  - A weapon looks wrong: that is a result, not a failure. Send a screenshot and the logs. Remove-Models.bat takes the new
    models out again.
  - The weapons still have their old names: run Send-Logs.bat and tell me. The last lines of ashenmarine\logs\launcher.log
    and the lines "MOD FILE OPENED" in hook.log say whether the game used the changed text.
  - No sound at all: check that Prepare-AshenMarine.bat said it prepared the sounds, then look in
    ashenmarine\logs\sfx.txt (it says why sounds are off). Tell me what it says.
  - Prepare says "NOT READY YET" in step 2, 3 or 5: you have no key collection yet (no older kit folder was found). Start the
    game once with Play-AshenMarine.bat, quit it, and run Prepare-AshenMarine.bat again.
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Backups of your real save are in ashenmarine\backups.

Where things are
  ashenmarine\assets\sounds        the sound files made from your Space Marine 2, set A (private; safe to delete)
  ashenmarine\assets\sounds-exact  set B (the game's own volumes and delays), if it could be made
  ashenmarine\cache                what the helper saved from the running game (keys for the archives); private, safe to delete
  ashenmarine\mod                  what the game loads instead of its own files: the changed text in  mod\msg\engus  and the new weapon
                                   models in  mod\parts  (only if you said yes)
  ashenmarine\ds3-models           the model trial run: its report, the pictures and the candidate files (made from your copies
                                   of the games; private, safe to delete; the game does not load these)
  ashenmarine\model-files          only if you ran Send-Model-Files.bat: the copies that went into the zip (safe to delete)
  ashenmarine\save                 the private copy of your save (what the test plays; F8's items live only here)
  ashenmarine\backups              backups of your REAL save (original = first ever, session-* = newest three)
  ashenmarine\logs                 launcher.log, hook.log, harvest.txt, sfx files and probe files
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
  - The names are written for the ENGLISH game text only (your game asks for the English files). If the game is set to
    another language, the weapons keep their old names for now.
  - The first time the game loads the changed text it may feel like nothing happened: the text is read when the game starts, so
    the names are right from the first screen on (no waiting).
  - The chainsword is fitted to the Shortsword by size (same length, so the reach the moves were made for stays). The bolt pistol
    is placed by a rule I worked out from the shapes of both weapons: upright, about 0.36 m long (a human-sized version of the
    Space Marine 2 pistol, the same scale as the chainsword), held by its handle. Where the bolts come out of it is still the
    crossbow's place (a few decimetres ahead of the muzzle); tell me how it looks in the hand and where the bolts appear.
  - This kit never goes online and never modifies any game file. What it adds to the game: files in ashenmarine\mod (loaded
    through ModEngine2).
  - It is an offline sandbox. Do not use it to play online.
