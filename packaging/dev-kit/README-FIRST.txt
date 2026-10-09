ASHEN MARINE - private test kit 8 ("a first look at the new weapon shapes")
==========================================================================

What this is
  Your Dark Souls III character with Space Marine 2's chainsword and bolt pistol sounds, the right NAMES for the two
  test weapons (what kits 5 to 7 were about) and - new in this kit - the first careful step towards their LOOKS: the
  program builds the Space Marine 2 chainsword and bolt pistol as Dark Souls III weapon models, checks them in every
  way it can and draws a picture of the result. In this kit that is only a TRIAL RUN: nothing is put into the game
  yet, the weapons still look like the Shortsword and the Avelyn. It is a private test kit, so it also writes logs
  that tell me how to make it better.

  Everything runs on your PC. Nothing is uploaded: you send me the files yourself at the end. Dark Souls III runs as an
  OFFLINE COPY of your save (your real save is backed up first and checked afterwards). Space Marine 2 and the
  Dark Souls III game files are only READ, never changed.

Kit 7 and kit 8
  Kit 8 does everything kit 7 did (opens Data0 without a key, takes over the keys the older kits collected, finds the
  item text by what it contains) and adds the model work. If you have already run kit 7 and sent me its logs, fine:
  this is the next step. If you have NOT run kit 7 yet, skip it and use this one: nothing is lost.

What is new: the model trial run
  Dark Souls III keeps each weapon's shape in a file of its own (the Shortsword's is parts\wp_a_0200.partsbnd.dcx, the
  Avelyn's wp_a_1409). The program reads that file from the game's archives, reads the Space Marine 2 chainsword and
  bolt pistol shape and picture from its game files, and builds a copy of the Dark Souls III weapon file that holds the
  Space Marine 2 shape: the bones, the points where the game holds and aims the weapon, the file's other parts and the
  shader settings stay the game's own. Before it trusts anything it checks that it understands the game's own file
  (it must be able to write the same bytes back that it read), and after it has made the new file it reads that back and
  compares. If any check fails it makes nothing and says why in the report - that is a result too.
  The new file (a "candidate") and a picture go into ashenmarine\ds3-models. NOTHING is put where the game would load it.

How to do it (about 10-15 minutes)
  1. Unzip this whole folder into the SAME place as the older kit folders (e.g. all in Downloads), as a new folder
     next to them. Do not delete the older ones yet. Steam must be running and signed in; Dark Souls III must NOT be
     running.
  2. Double-click  Prepare-AshenMarine.bat  and wait for "Done". It does five things, one after the other:
       - makes the sound files from your Space Marine 2 (a minute or two),
       - makes the item-name file from your Dark Souls III (it first says "Took over the key cache of an older kit
         folder"; it may say "Looking at the start of every file in the archives": please let it run). LOOK AT THE
         LINES OF STEP 2: "wrote msg\ENGLISH\item.msgbnd.dcx" means the names are ready,
       - writes a report about where Dark Souls III keeps its files (no copies of them),
       - writes the model reports (a minute),
       - STEP 5, THE MODEL TRIAL RUN (a minute or two): LOOK AT THE LAST LINES: "N model file(s) made, M not made".
  3. Open the pictures in  ashenmarine\ds3-models  with a double-click:  wp_a_0200-overlay.png  (the chainsword) and
     wp_a_1409-overlay.png  (the bolt pistol); the names ending in _l are the left-hand versions. Each shows three views side by side (front, side, top). ORANGE is the outline of the
     Dark Souls III weapon that is being replaced; GREY is the Space Marine 2 weapon, turned and scaled to fit it. If you see a
     grey chainsword lying along the orange sword, the fit worked.
  4. Double-click  Play-AshenMarine.bat  . Dark Souls III starts; the title bar reads
     "DARK SOULS III - Ashen Marine (offline copy)".  (A question about connecting online is expected: it is blocked
     on purpose.)  Load your normal character, then:
       a. Stand still (not in a menu) and press  F8  ONCE. Three test items go into your inventory (this kit folder has
          its own private copy of your save; this is the offline copy only). Open the inventory - Weapons and
          Ammunition. WHAT ARE THEY CALLED? What do the descriptions say?
       b. Equip them as before (the first in your right hand, the second in your left hand, the bolts in a bolt slot),
          swing and fire for a minute: the sounds should work as before. F9 switches between sound sets A and B.
       c. Quit to the desktop from the in-game menu as usual.
  5. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop (the reports, the logs and the
     pictures of the trial run). Attach it to the chat and tell me:
       - what did step 2 of Prepare say (did it write the item text), and what are the three items called?
       - what did step 5 say, and does the grey weapon sit on the orange one in the pictures?
  6. OPTIONAL: double-click  Send-Model-Files.bat . It copies the chainsword and bolt pistol model files of your Space
     Marine 2 (with the pictures they use) and five weapon model files of your Dark Souls III into ONE zip on your
     Desktop, AshenMarine-model-files.zip. You do not have to: nothing is sent unless YOU attach the zip to the chat. If you
     do, I can try the models on exactly your files here instead of waiting for your next run. The finished mashup never
     contains any Space Marine 2 or Dark Souls III file.

What it records (you can read every file yourself)
  ashenmarine\ds3-prepare\ds3-report.txt       what was read from Dark Souls III's files for the names, and what was found
  ashenmarine\ds3-probe\ds3-report.txt         where Dark Souls III keeps the files the mod needs (file names and sizes only), and how the
                                               model and texture files of two weapons are built (structure only, no copies)
  ashenmarine\logs\harvest.txt                 what the helper looked for in the game's memory and what it found
                                               (key fingerprints and places only - never the keys themselves)
  ashenmarine\prepare-sm2\prepare-report.txt   what was read from Space Marine 2 and how it became sound files
  ashenmarine\probe-sm2-mesh\mesh-report.txt   how Space Marine 2 stores the chainsword / bolt pistol models (and which pictures they use)
  ashenmarine\ds3-models\ds3-models-report.txt the model trial run: every check, and every number of the new shapes
  ashenmarine\ds3-models\*-overlay.png         the pictures of the trial run (drawn by the program; no game file is in them)
  ashenmarine\logs\sfx.txt, sfx-trace.csv      every sound that played, what was in your hands, stamina / buttons /
                                               ammunition (this is how I tune what counts as a swing or a shot)
  ashenmarine\logs\probe-ds3.txt               game build, parameter tables, the beeps
  ashenmarine\logs\hook.log, launcher.log      what the safety helper did (hook.log also says when the game loads the changed
                                               name file from the mod folder: "MOD FILE opened by the game")
  ashenmarine\mod\ashenmarine-msg.json         which text the name file was made from (hashes and names only)
  Send-Logs.bat also lists the NAMES and SIZES of the files in ashenmarine\cache (not their contents).
  Your Windows user name and Steam id are masked in the copies Send-Logs makes. Your character's NAME is not logged.
  The sound files, the name file and the cache stay on your PC (they are made from your copies of the games).

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - "Dark Souls III ran, but Ashen Marine's protection never started": Steam was probably not running. Start Steam,
    wait until it is ready, close Dark Souls III, press Play again.
  - Dark Souls III crashes: that is useful information - run Send-Logs.bat anyway and tell me roughly what you were
    doing (and whether it was in the first minute, when the helper is busy). Your real save was not changed (it is
    backed up and checked each time).
  - No sound at all: check that Prepare-AshenMarine.bat said it prepared the sounds, then look in
    ashenmarine\logs\sfx.txt (it says why sounds are off). Tell me what it says.
  - The weapons still have their old names: run Send-Logs.bat and tell me. The files ashenmarine\ds3-prepare\ds3-report.txt
    and ashenmarine\ds3-probe\ds3-report.txt say exactly what was found and what was not.
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Backups of your real save are in ashenmarine\backups.

Where things are
  ashenmarine\assets\sounds        the sound files made from your Space Marine 2, set A (private; safe to delete)
  ashenmarine\assets\sounds-exact  set B (the game's own volumes and delays), if it could be made
  ashenmarine\cache                what the helper saved from the running game (keys for the archives); private, safe to delete
  ashenmarine\mod                  the name file the game loads instead of its own (made from your Dark Souls III)
  ashenmarine\ds3-models           the model trial run: its report, the pictures and the candidate files (made from your copies
                                   of the games; private, safe to delete; the game does not load them)
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
  - This kit never goes online and never modifies any game file. The only thing it adds to the game's world is the
    name file in ashenmarine\mod, which Dark Souls III loads through ModEngine2.
  - It is an offline sandbox. Do not use it to play online.
