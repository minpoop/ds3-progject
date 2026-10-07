ASHEN MARINE - private test kit 3 (look around, change nothing)
================================================================

What this is
  Read-only tests that tell me what your own games contain, so the Space Marine weapons can be built on real facts.
  Nothing here changes Dark Souls III, Space Marine 2 or your saves, and nothing is uploaded: you send me the files
  yourself at the end.

  Test 1 - Probe-SM2.bat         reads your Space Marine 2 files (under a minute) and writes a report.
  Test 2 - Play-AshenMarine.bat  starts Dark Souls III as an OFFLINE COPY of your save (your real save is backed up
                                 first and checked afterwards) with a read-only probe running inside the game.
  Then   - Send-Logs.bat         collects the text logs into one zip on your Desktop.
  Optional - Send-Samples.bat    a handful of short sound clips from your own Space Marine 2 (about half a megabyte),
                                 so I can test the sound converter. Only attach that zip if you are happy to.

How to do it (about 15 minutes)
  1. Unzip this whole folder anywhere (e.g. Desktop). Steam must be running; Dark Souls III must NOT be running.
  2. Double-click  Probe-SM2.bat  and wait for "Done".
  3. Double-click  Play-AshenMarine.bat .  Dark Souls III starts; the title bar reads
     "DARK SOULS III - Ashen Marine (offline copy)".
       - About 20 seconds after it starts (the title screen) you should hear TWO short beeps (low, then high).
       - Load your normal character. About 6 seconds after you appear in the world you should hear TWO more beeps.
         (Please tell me whether you heard each pair, and whether the game's own sound stayed normal.)
       - Then play for about 3 minutes, in this order, with a pause of 2-3 seconds between actions:
            a. stand still for 10 seconds
            b. five single light attacks
            c. three heavy attacks
            d. three dodge rolls
            e. sprint for 3 seconds
            f. one sip of Estus (or any consumable)
            g. if you have a bow or crossbow: shoot three arrows or bolts
            h. open the inventory menu once
         then quit to the desktop from the in-game menu as usual.
       (If it asks about connecting online, that is expected: it is blocked on purpose.)
  4. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop. Attach it to the chat and tell me:
       - did Dark Souls III start and feel normal?
       - did you hear the two pairs of beeps?
  5. Optional: double-click  Send-Samples.bat  and attach  AshenMarine-sound-samples.zip  as well.

What it records (you can read every file yourself)
  ashenmarine\probe-sm2\probe-report.txt   what is inside your Space Marine 2 weapon files (names, sizes, text of the
                                           weapon definition files)
  ashenmarine\logs\probe-ds3.txt           game build, what the game reports about weapons, inventory, stamina, the beeps
  ashenmarine\logs\probe-ds3-*.csv         the weapon table, your inventory item ids, stamina samples, item-name tables
  ashenmarine\logs\hook.log, launcher.log  what the safety helper did
  Your Windows user name and Steam id are masked in the copies Send-Logs makes. Your character's NAME is not logged.

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - Dark Souls III crashes during the test: that is useful information - run Send-Logs.bat anyway and tell me roughly
    when it happened. Your real save was not changed (it is backed up and checked each time).
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
