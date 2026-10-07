ASHEN MARINE - private test kit 2 (look around, change nothing)
================================================================

What this is
  Two read-only tests that tell me what your own games contain, so the Space Marine weapons can be built on real
  facts instead of guesses. Nothing here changes Dark Souls III, Space Marine 2 or your saves, and nothing is
  uploaded: you send me the logs yourself at the end.

  Test 1 - Probe-SM2.bat      reads your Space Marine 2 files (about 3-10 minutes) and writes a report.
  Test 2 - Play-AshenMarine.bat  starts Dark Souls III as an OFFLINE COPY of your save (your real save is backed up
                              first and checked afterwards) with a read-only probe running inside the game.
  Then   - Send-Logs.bat      collects the text logs into one zip on your Desktop.

How to do it (about 15 minutes)
  1. Unzip this whole folder anywhere (e.g. Desktop). Steam must be running; Dark Souls III must NOT be running.
  2. Double-click  Probe-SM2.bat  and wait for "Done". When it finishes, open
        ashenmarine\probe-sm2\textures\index.html
     in your browser: it shows pictures of Space Marine 2 weapon textures that the tool decoded from YOUR game.
     Tell me whether they look like real weapon textures (or garbled / black / empty). They stay on your PC.
  3. Double-click  Play-AshenMarine.bat . Dark Souls III starts; the title bar reads
     "DARK SOULS III - Ashen Marine (offline copy)". Load your character and
        - walk around for a minute,
        - swing your weapon a few times and roll once or twice,
        - use any consumable (an Estus sip is fine) and, if you have one, shoot a bow or crossbow once,
        - open the inventory menu once, then quit to the desktop from the in-game menu as usual.
     (If it asks about connecting online, that is expected: it is blocked on purpose.)
     About 6 seconds after your character appears in the world you should hear TWO short beeps (a low one, then a
     higher one). That is a test of playing sound inside the game; please note whether you heard them.
     Please do this once with your normal character - the probe records what the game reports while you play.
  4. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop. Attach it to the chat and tell me:
       - did Dark Souls III start and feel normal (load times, menus, anything odd)?
       - did you hear the two beeps, and did the game's own sound stay normal while they played?
       - did the pictures in step 2 look right?

What it records (you can read every file yourself)
  ashenmarine\probe-sm2\probe-report.txt   what is inside your Space Marine 2 weapon files (names, sizes, a few text lines)
  ashenmarine\logs\probe-ds3.txt           game build, what the game reports about weapons, inventory, stamina, the beep test
  ashenmarine\logs\probe-ds3-*.csv         the weapon table, your inventory item ids, stamina samples, item-name tables
  ashenmarine\logs\hook.log, launcher.log  what the safety helper did
  Your Windows user name and Steam id are masked in the copies Send-Logs makes. Your character's NAME is not logged.

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - Dark Souls III crashes during the test: that is useful information - run Send-Logs.bat anyway and tell me
    roughly when it happened. Your real save was not changed (it is backed up and checked each time).
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Backups of your real save are in ashenmarine\backups.

Where things are
  ashenmarine\save       the private copy of your save (what the test plays)
  ashenmarine\backups    backups of your REAL save (original = first ever, session-* = newest three)
  ashenmarine\logs       launcher.log, hook.log and the probe files
  modengine2\            ModEngine2 2.1.0 (MIT license) - the loader Melty will install for you in the real release

Good to know
  - If it cannot find Dark Souls III by itself, put the game's folder (the one that contains Game\DarkSoulsIII.exe)
    on the first line of a new text file  ashenmarine\game-folder.txt  and press Play again.
  - If it cannot find Space Marine 2, put its folder on the first line of  ashenmarine\sm2-folder.txt .
  - The private save copy is a snapshot of your character taken the first time you press Play. To start a fresh
    copy of your current real character later, close the game and delete the  ashenmarine\save  folder.
  - This kit never goes online, never launches Space Marine 2, and never modifies any game file.
  - It is an offline sandbox. Do not use it to play online.
