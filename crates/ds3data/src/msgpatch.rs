//! Changes item names and descriptions in the decoded `item.msgbnd.dcx` (a BND4 of text tables), and nothing else.
//!
//! The patch **fails closed**: every edit says which name it expects to find (`"Shortsword"` at id 2000000); if no table
//! holds exactly that name there, nothing is produced, and the error says what the tables hold instead. That way a game
//! update that moved or renamed things can never lead to a wrong override being written.
//!
//! * The *name tables* of an edit are all tables whose text at the id equals the expected name; the text is replaced.
//! * The *description tables* are the other tables that have a text at the same id and look like siblings of a name table
//!   (their ids overlap a name table's ids: summaries and descriptions list the same weapons). Their text is replaced by
//!   the long text when it is longer than 160 characters or has a line break, else by the short text. A table that has no
//!   text at the id is never touched, nor one whose ids hardly overlap a name table's (an unrelated table that happens to
//!   have an entry at the same id).
//! * Every changed table is written back with [`crate::bnd4::replace_file`], which leaves every other byte of the container
//!   alone; the result is read back and compared before it is returned.
use crate::bnd4::{replace_file, Bnd4, Bnd4Error, Bnd4File};
use crate::fmg::FmgFile;
use crate::util::snippet;
use std::fmt;

/// A description longer than this many characters counts as a long one.
pub const LONG_TEXT_CHARS: usize = 160;

/// One change: the name at `id`, which must be `expect_name` now, becomes `new_name`; the descriptions at that id get the
/// short or the long text (`None`: they are left as they are).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemEdit<'a> {
    pub id: u32,
    pub expect_name: &'a str,
    pub new_name: &'a str,
    pub short_text: Option<&'a str>,
    pub long_text: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// The container could not be read or patched.
    Bnd4(Bnd4Error),
    /// No file of the container is a text table.
    NoTables,
    /// No table has a text at this id.
    IdAbsent { id: u32 },
    /// Tables have a text at this id but none holds the expected name. `found` lists `(table, first 30 characters)`.
    NameMismatch { id: u32, expected: String, found: Vec<(String, String)> },
    /// The edits themselves are not usable.
    BadEdit(&'static str),
    /// The patched container did not read back as intended.
    SelfCheck(String),
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchError::Bnd4(e) => write!(f, "{e}"),
            PatchError::NoTables => write!(f, "none of the files inside could be read as a text table"),
            PatchError::IdAbsent { id } => write!(f, "no text table has an entry for id {id}"),
            PatchError::NameMismatch { id, expected, found } => {
                write!(f, "id {id} should be called \"{expected}\" but the text tables hold: ")?;
                for (i, (table, text)) in found.iter().enumerate() {
                    write!(f, "{}{table} = \"{text}\"", if i == 0 { "" } else { "; " })?;
                }
                Ok(())
            }
            PatchError::BadEdit(why) => write!(f, "unusable edit: {why}"),
            PatchError::SelfCheck(why) => write!(f, "internal check failed after patching: {why}"),
        }
    }
}

impl std::error::Error for PatchError {}

impl From<Bnd4Error> for PatchError {
    fn from(e: Bnd4Error) -> Self {
        PatchError::Bnd4(e)
    }
}

/// What a patch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOutcome {
    /// The patched container.
    pub bytes: Vec<u8>,
    /// One line per decision, for the report.
    pub log: Vec<String>,
    /// Indexes (in the container) of the tables that were rewritten.
    pub changed_files: Vec<usize>,
}

/// How a file is called in messages: its number, the last part of its name and its id.
pub fn file_label(f: &Bnd4File) -> String {
    let name = f.name.as_deref().map(|n| n.rsplit(['\\', '/']).next().unwrap_or(n)).filter(|n| !n.is_empty());
    match (name, f.id) {
        (Some(n), Some(id)) => format!("file {} {n} (id {id})", f.index),
        (Some(n), None) => format!("file {} {n}", f.index),
        (None, Some(id)) => format!("file {} (id {id})", f.index),
        (None, None) => format!("file {}", f.index),
    }
}

struct Table {
    index: usize,
    label: String,
    original: FmgFile,
    fmg: FmgFile,
    edited_ids: Vec<u32>,
}

fn ids_overlap(a: &FmgFile, b: &FmgFile) -> usize {
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    small.iter().filter(|(id, _)| large.contains(*id)).count()
}

/// [`patch_item_msgbnd_detailed`] without the list of changed files.
pub fn patch_item_msgbnd(bnd4_bytes: &[u8], edits: &[ItemEdit]) -> Result<(Vec<u8>, Vec<String>), PatchError> {
    let outcome = patch_item_msgbnd_detailed(bnd4_bytes, edits)?;
    Ok((outcome.bytes, outcome.log))
}

/// Applies the edits to a decoded `item.msgbnd` (see the module docs). On any problem nothing is returned.
pub fn patch_item_msgbnd_detailed(bnd4_bytes: &[u8], edits: &[ItemEdit]) -> Result<PatchOutcome, PatchError> {
    for (i, e) in edits.iter().enumerate() {
        if e.expect_name.is_empty() || e.new_name.is_empty() {
            return Err(PatchError::BadEdit("names must not be empty"));
        }
        if edits[..i].iter().any(|other| other.id == e.id) {
            return Err(PatchError::BadEdit("an id is edited twice"));
        }
    }
    let parsed = Bnd4::parse(bnd4_bytes)?;
    let mut log: Vec<String> = Vec::new();
    let mut tables: Vec<Table> = Vec::new();
    for f in &parsed.files {
        let label = file_label(f);
        if f.is_compressed() {
            log.push(format!("{label}: compressed on its own, not read as a text table"));
            continue;
        }
        let Some(bytes) = parsed.file_bytes(bnd4_bytes, f.index) else { continue };
        match FmgFile::parse(bytes) {
            Ok(fmg) => tables.push(Table { index: f.index, label, original: fmg.clone(), fmg, edited_ids: Vec::new() }),
            Err(e) => log.push(format!("{label}: not read as a text table ({e})")),
        }
    }
    if tables.is_empty() {
        return Err(PatchError::NoTables);
    }
    log.push(format!("{} of the {} files are text tables", tables.len(), parsed.files.len()));

    for edit in edits {
        let name_tables: Vec<usize> = (0..tables.len()).filter(|i| tables[*i].fmg.get(edit.id) == Some(edit.expect_name)).collect();
        if name_tables.is_empty() {
            let found: Vec<(String, String)> = tables.iter().filter_map(|t| t.fmg.get(edit.id).map(|text| (t.label.clone(), snippet(text, 30)))).collect();
            return Err(if found.is_empty() { PatchError::IdAbsent { id: edit.id } } else { PatchError::NameMismatch { id: edit.id, expected: edit.expect_name.to_string(), found } });
        }
        log.push(format!("id {}: \"{}\" -> \"{}\"", edit.id, edit.expect_name, edit.new_name));
        for &i in &name_tables {
            let t = &mut tables[i];
            t.fmg.set(edit.id, edit.new_name);
            t.edited_ids.push(edit.id);
            log.push(format!("  name in {}: replaced", t.label));
        }
        let mut untouched = 0usize;
        for i in 0..tables.len() {
            if name_tables.contains(&i) {
                continue;
            }
            let Some(old) = tables[i].fmg.get(edit.id).map(str::to_string) else {
                untouched += 1;
                continue;
            };
            // a sibling of a name table lists (about) the same ids; an unrelated table that happens to have an entry here does not
            let (overlap, of) = name_tables
                .iter()
                .map(|n| (ids_overlap(&tables[*n].original, &tables[i].original), tables[*n].original.len().min(tables[i].original.len())))
                .max_by_key(|(overlap, _)| *overlap)
                .unwrap_or((0, 0));
            let label = tables[i].label.clone();
            if overlap * 2 < of {
                log.push(format!("  {label} has a text at this id but shares only {overlap} of its {of} ids with a name table: not a description, left alone"));
                continue;
            }
            let long = old.chars().count() > LONG_TEXT_CHARS || old.contains('\n');
            let (kind, replacement) = if long { ("long", edit.long_text) } else { ("short", edit.short_text) };
            let Some(text) = replacement else {
                log.push(format!("  description in {label}: old text is {kind}, no {kind} replacement given, left as it is"));
                continue;
            };
            let lines = old.lines().count();
            tables[i].fmg.set(edit.id, text);
            tables[i].edited_ids.push(edit.id);
            log.push(format!("  description in {label}: replaced with the {kind} text (old text: {} characters, {lines} line{})", old.chars().count(), if lines == 1 { "" } else { "s" }));
        }
        if untouched > 0 {
            log.push(format!("  {untouched} other text tables have nothing at this id and are not touched"));
        }
    }

    // write the changed tables back, one at a time (every replacement moves the files after it)
    let mut current = bnd4_bytes.to_vec();
    let mut changed_files = Vec::new();
    for t in tables.iter().filter(|t| !t.edited_ids.is_empty()) {
        let state = Bnd4::parse(&current)?;
        let before = state.files.get(t.index).map_or(0, |f| f.stored_size);
        let bytes = t.fmg.to_bytes();
        current = replace_file(&current, &state, t.index, &bytes)?;
        changed_files.push(t.index);
        log.push(format!("{} rewritten ({before} -> {} bytes)", t.label, bytes.len()));
    }

    // read it back: the changed tables hold exactly the intended texts, every other file is byte-identical
    let after = Bnd4::parse(&current)?;
    if after.files.len() != parsed.files.len() {
        return Err(PatchError::SelfCheck("the number of files changed".to_string()));
    }
    for f in &parsed.files {
        let old = parsed.file_bytes(bnd4_bytes, f.index);
        let new = after.file_bytes(&current, f.index);
        match tables.iter().find(|t| t.index == f.index && !t.edited_ids.is_empty()) {
            None if old != new => return Err(PatchError::SelfCheck(format!("{} changed although it was not meant to", file_label(f)))),
            None => {}
            Some(t) => {
                let reread = new.and_then(|b| FmgFile::parse(b).ok()).ok_or_else(|| PatchError::SelfCheck(format!("{} does not read back", t.label)))?;
                if reread != t.fmg {
                    return Err(PatchError::SelfCheck(format!("{} does not hold what was written", t.label)));
                }
                // only the edited ids differ from the original
                let same_elsewhere = reread.len() == t.original.len() && t.original.iter().all(|(id, text)| t.edited_ids.contains(&id) || (reread.contains(id) && reread.get(id) == text));
                if !same_elsewhere {
                    return Err(PatchError::SelfCheck(format!("{} changed at ids that were not edited", t.label)));
                }
            }
        }
    }
    Ok(PatchOutcome { bytes: current, log, changed_files })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dcx::{self, DcxInfo};
    use crate::testing::bnd4::Bnd4Spec;
    use crate::testing::items::{bnd4_of, game_path, item_bnd4, item_dcx, item_tables, Table, TARGETS};

    const SHORT: [&str; 3] = ["short one", "short two", "short three"];
    const LONG: [&str; 3] = ["long one\nsecond line", "long two\nsecond line", "long three\nsecond line"];

    fn edits() -> [ItemEdit<'static>; 3] {
        [
            ItemEdit { id: 2_000_000, expect_name: "Shortsword", new_name: "Chainsword", short_text: Some(SHORT[0]), long_text: Some(LONG[0]) },
            ItemEdit { id: 14_090_000, expect_name: "Avelyn", new_name: "Bolt Pistol", short_text: Some(SHORT[1]), long_text: Some(LONG[1]) },
            ItemEdit { id: 404_000, expect_name: "Standard Bolt", new_name: "Bolt Rounds", short_text: Some(SHORT[2]), long_text: Some(LONG[2]) },
        ]
    }

    fn tables_of(bytes: &[u8]) -> Vec<(i32, FmgFile)> {
        let b = Bnd4::parse(bytes).unwrap();
        b.files.iter().filter_map(|f| FmgFile::parse(b.file_bytes(bytes, f.index)?).ok().map(|t| (f.id.unwrap(), t))).collect()
    }

    fn table(tables: &[(i32, FmgFile)], id: i32) -> &FmgFile {
        &tables.iter().find(|(i, _)| *i == id).unwrap().1
    }

    #[test]
    fn names_summaries_and_descriptions_change_and_nothing_else() {
        let src = item_bnd4();
        let out = patch_item_msgbnd_detailed(&src, &edits()).unwrap();
        let before = tables_of(&src);
        let after = tables_of(&out.bytes);
        assert_eq!(after.len(), 5);
        let names = table(&after, 11);
        assert_eq!(names.get(2_000_000), Some("Chainsword"));
        assert_eq!(names.get(14_090_000), Some("Bolt Pistol"));
        assert_eq!(names.get(404_000), Some("Bolt Rounds"));
        // the two other test weapons and everything else in the table are as before
        assert_eq!((names.get(14_040_000), names.get(14_190_000)), (Some("Light Crossbow"), Some("Repeating Crossbow")));
        assert_eq!((names.get(1_000_000), names.get(1_000_001), names.contains(1_000_001)), (Some("Dagger"), None, true), "a null string stays null");
        assert_eq!(names.len(), table(&before, 11).len());
        // short summaries and long descriptions
        let info = table(&after, 21);
        assert_eq!((info.get(2_000_000), info.get(14_090_000), info.get(404_000)), (Some(SHORT[0]), Some(SHORT[1]), Some(SHORT[2])));
        assert_eq!(info.get(14_040_000), table(&before, 21).get(14_040_000));
        let caption = table(&after, 31);
        assert_eq!((caption.get(2_000_000), caption.get(14_090_000), caption.get(404_000)), (Some(LONG[0]), Some(LONG[1]), Some(LONG[2])));
        assert_eq!(caption.get(14_190_000), table(&before, 31).get(14_190_000));
        // the tables without these ids and the file that is no table are byte-identical
        let (b0, b1) = (Bnd4::parse(&src).unwrap(), Bnd4::parse(&out.bytes).unwrap());
        for i in [0usize, 4, 5] {
            assert_eq!(b0.file_bytes(&src, i), b1.file_bytes(&out.bytes, i), "file {i}");
        }
        assert_eq!(out.changed_files, vec![1, 2, 3]);
        assert!(b1.layout.verified);
        let log = out.log.join("\n");
        for needle in [
            "\"Shortsword\" -> \"Chainsword\"",
            "name in file 1 WeaponName.fmg (id 11): replaced",
            "description in file 2 WeaponInfo.fmg (id 21): replaced with the short text",
            "description in file 3 WeaponCaption.fmg (id 31): replaced with the long text",
            "not read as a text table",
        ] {
            assert!(log.contains(needle), "{needle:?} missing from:\n{log}");
        }
        // the plain wrapper gives the same bytes
        let (bytes, lines) = patch_item_msgbnd(&src, &edits()).unwrap();
        assert_eq!((bytes, lines), (out.bytes.clone(), out.log.clone()));
        // patching the result again fails closed: the names are not the expected ones any more
        assert!(matches!(patch_item_msgbnd(&out.bytes, &edits()), Err(PatchError::NameMismatch { .. })));
    }

    #[test]
    fn the_whole_chain_through_dcx_works() {
        let (decoded, info) = dcx::decode(&item_dcx()).unwrap();
        assert_eq!(decoded, item_bnd4());
        let (patched, _) = patch_item_msgbnd(&decoded, &edits()).unwrap();
        let file = dcx::encode(&patched, &info).unwrap();
        let (again, _) = dcx::decode(&file).unwrap();
        assert_eq!(table(&tables_of(&again), 11).get(2_000_000), Some("Chainsword"));
        // an edit list that is empty changes nothing
        let (same, _) = patch_item_msgbnd(&decoded, &[]).unwrap();
        assert_eq!(same, decoded);
    }

    #[test]
    fn a_different_name_fails_closed_and_says_what_is_there() {
        let mut tables = item_tables();
        tables[1].fmg.set(2_000_000, "Broadsword");
        let src = bnd4_of(&tables, &[]);
        let err = patch_item_msgbnd(&src, &edits()).unwrap_err();
        let PatchError::NameMismatch { id, expected, found } = &err else { panic!("{err:?}") };
        assert_eq!((*id, expected.as_str()), (2_000_000, "Shortsword"));
        assert_eq!(found.len(), 3, "the name, summary and description tables all have a text there: {found:?}");
        assert_eq!(found[0], ("file 1 WeaponName.fmg (id 11)".to_string(), "Broadsword".to_string()));
        assert!(found[1].1.contains("summary"), "{found:?}");
        assert!(found[2].1.ends_with("..."), "long texts are cut to 30 characters: {found:?}");
        assert!(found.iter().all(|(_, text)| text.chars().count() <= 33));
        let message = err.to_string();
        assert!(message.contains("id 2000000") && message.contains("\"Shortsword\"") && message.contains("Broadsword"), "{message}");
    }

    #[test]
    fn an_id_that_no_table_has_is_an_error_too() {
        let mut tables = item_tables();
        for t in &mut tables {
            t.fmg.remove(14_090_000);
        }
        let src = bnd4_of(&tables, &[]);
        assert_eq!(patch_item_msgbnd(&src, &edits()).unwrap_err(), PatchError::IdAbsent { id: 14_090_000 });
        // a null string is no text
        let mut tables = item_tables();
        for t in &mut tables {
            t.fmg.set_null(14_090_000);
        }
        let src = bnd4_of(&tables, &[]);
        assert_eq!(patch_item_msgbnd(&src, &edits()).unwrap_err(), PatchError::IdAbsent { id: 14_090_000 });
        // the name is null in the name table but the other tables still have the id
        let mut tables = item_tables();
        tables[1].fmg.set_null(2_000_000);
        let src = bnd4_of(&tables, &[]);
        let PatchError::NameMismatch { found, .. } = patch_item_msgbnd(&src, &edits()).unwrap_err() else { panic!() };
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn short_and_long_replacements_follow_the_old_text() {
        let long_line = "x".repeat(200);
        let mut tables = item_tables();
        // summary: 161 characters on one line -> long; description: short but with a line break -> long
        tables[2].fmg.set(2_000_000, &"y".repeat(161));
        tables[3].fmg.set(2_000_000, "short\nwith a break");
        // exactly 160 characters on one line -> still short
        tables[2].fmg.set(14_090_000, &"z".repeat(160));
        tables[3].fmg.set(14_090_000, &long_line);
        let src = bnd4_of(&tables, &[]);
        let after = tables_of(&patch_item_msgbnd(&src, &edits()).unwrap().0);
        assert_eq!(table(&after, 21).get(2_000_000), Some(LONG[0]));
        assert_eq!(table(&after, 31).get(2_000_000), Some(LONG[0]));
        assert_eq!(table(&after, 21).get(14_090_000), Some(SHORT[1]), "160 characters are not more than 160");
        assert_eq!(table(&after, 31).get(14_090_000), Some(LONG[1]), "200 characters on one line are long");
        // no replacement of one kind: those texts stay, the rest is still done
        let no_long = [ItemEdit { long_text: None, ..edits()[0] }];
        let (bytes, log) = patch_item_msgbnd(&src, &no_long).unwrap();
        let t = tables_of(&bytes);
        assert_eq!(table(&t, 11).get(2_000_000), Some("Chainsword"));
        assert_eq!(table(&t, 21).get(2_000_000), table(&tables_of(&src), 21).get(2_000_000), "the summary was long and has no long replacement");
        assert!(log.join("\n").contains("no long replacement given"));
        let nothing = [ItemEdit { short_text: None, long_text: None, ..edits()[1] }];
        let (bytes, _) = patch_item_msgbnd(&item_bnd4(), &nothing).unwrap();
        assert_eq!(table(&tables_of(&bytes), 11).get(14_090_000), Some("Bolt Pistol"));
        assert_eq!(table(&tables_of(&bytes), 31).get(14_090_000), table(&tables_of(&item_bnd4()), 31).get(14_090_000));
    }

    #[test]
    fn every_table_with_the_expected_name_is_changed_and_unrelated_tables_are_left_alone() {
        let mut tables = item_tables();
        // a second name table (like a patch or DLC variant of the same names)
        let second = tables[1].fmg.clone();
        tables.push(Table { id: 41, name: game_path("WeaponName_dlc.fmg"), fmg: second });
        // an unrelated table with an entry at one of the ids
        let mut other = FmgFile::new();
        for i in 0..30u32 {
            other.set(7_000_000 + i, "something else");
        }
        other.set(404_000, "Something unrelated");
        tables.push(Table { id: 42, name: game_path("Misc.fmg"), fmg: other.clone() });
        let src = bnd4_of(&tables, &[]);
        let out = patch_item_msgbnd_detailed(&src, &edits()).unwrap();
        let after = tables_of(&out.bytes);
        assert_eq!(table(&after, 11).get(404_000), Some("Bolt Rounds"));
        assert_eq!(table(&after, 41).get(404_000), Some("Bolt Rounds"), "both name tables");
        assert_eq!(table(&after, 42), &other, "the unrelated table is untouched");
        assert!(out.log.join("\n").contains("not a description, left alone"));
        assert!(!out.changed_files.contains(&6));
        // two name tables with different texts: only the one with the expected name is a name table
        let mut tables = item_tables();
        let mut variant = tables[1].fmg.clone();
        variant.set(404_000, "Something else");
        tables.push(Table { id: 43, name: game_path("WeaponName_alt.fmg"), fmg: variant });
        let src = bnd4_of(&tables, &[]);
        let after = tables_of(&patch_item_msgbnd(&src, &edits()).unwrap().0);
        assert_eq!(table(&after, 11).get(404_000), Some("Bolt Rounds"));
        assert_eq!(table(&after, 43).get(404_000), Some(SHORT[2]), "a sibling with the same ids counts as a description table");
    }

    #[test]
    fn files_that_cannot_be_read_as_tables_are_skipped_and_logged() {
        // a damaged copy of the caption table next to the good one
        let tables = item_tables();
        let mut damaged = tables[3].fmg.to_bytes();
        damaged[2] = 7;
        let src = bnd4_of(&tables, &[(50, "Damaged.fmg", damaged), (51, "Empty.bin", b"x".to_vec())]);
        let out = patch_item_msgbnd_detailed(&src, &edits()).unwrap();
        let log = out.log.join("\n");
        assert!(log.contains("file 5 Damaged.fmg (id 50): not read as a text table"), "{log}");
        assert!(log.contains("file 6 Empty.bin (id 51): not read as a text table"), "{log}");
        // when the name table itself is damaged, nothing is made
        let mut bad_names = tables[1].fmg.to_bytes();
        bad_names[0x14] = 0;
        let mut spec = Bnd4Spec::new(0x74);
        for (i, t) in tables.iter().enumerate() {
            let bytes = if i == 1 { bad_names.clone() } else { t.fmg.to_bytes() };
            spec = spec.file(t.id, &t.name, &bytes);
        }
        let err = patch_item_msgbnd(&spec.build(), &edits()).unwrap_err();
        assert!(matches!(err, PatchError::NameMismatch { .. }), "{err:?}: the summary and description tables are there, the names are not");
        // no tables at all
        let junk = Bnd4Spec::new(0x74).file(1, "a.bin", b"junk").file(2, "b.bin", b"more junk").build();
        assert_eq!(patch_item_msgbnd(&junk, &edits()).unwrap_err(), PatchError::NoTables);
        // a table that is compressed on its own is not read (and so, if it is the name table, nothing is made)
        let mut spec = Bnd4Spec::new(0x74);
        for t in &tables {
            spec = spec.file(t.id, &t.name, &t.fmg.to_bytes());
        }
        spec.files[1].flags = 0x03;
        let err = patch_item_msgbnd(&spec.build(), &edits()).unwrap_err();
        assert!(matches!(err, PatchError::NameMismatch { .. }));
    }

    #[test]
    fn containers_that_cannot_be_patched_are_refused() {
        let tables = item_tables();
        let mut spec = Bnd4Spec::new(0x74);
        spec.alignment = 24;
        for t in &tables {
            spec = spec.file(t.id, &t.name, &t.fmg.to_bytes());
        }
        let err = patch_item_msgbnd(&spec.build(), &edits()).unwrap_err();
        assert!(matches!(err, PatchError::Bnd4(Bnd4Error::LayoutNotVerified(_))), "{err:?}");
        assert!(err.to_string().contains("not laid out"));
        assert!(matches!(patch_item_msgbnd(b"not a container", &edits()), Err(PatchError::Bnd4(Bnd4Error::NotBnd4))));
        assert!(matches!(patch_item_msgbnd(&[], &edits()), Err(PatchError::Bnd4(_))));
    }

    #[test]
    fn unusable_edits_are_refused_before_anything_is_done() {
        let src = item_bnd4();
        let dup = [edits()[0], ItemEdit { new_name: "Other", ..edits()[0] }];
        assert_eq!(patch_item_msgbnd(&src, &dup).unwrap_err(), PatchError::BadEdit("an id is edited twice"));
        let empty_new = [ItemEdit { new_name: "", ..edits()[0] }];
        assert!(matches!(patch_item_msgbnd(&src, &empty_new), Err(PatchError::BadEdit(_))));
        let empty_expect = [ItemEdit { expect_name: "", ..edits()[0] }];
        assert!(matches!(patch_item_msgbnd(&src, &empty_expect), Err(PatchError::BadEdit(_))));
    }

    #[test]
    fn all_five_test_weapons_can_be_edited_and_texts_may_grow_a_lot() {
        let big = "word ".repeat(5000);
        let edits: Vec<ItemEdit> = TARGETS
            .iter()
            .map(|(id, name)| ItemEdit { id: *id, expect_name: name, new_name: "A much, much longer replacement name than the original", short_text: Some("s"), long_text: Some(&big) })
            .collect();
        let (bytes, _) = patch_item_msgbnd(&item_bnd4(), &edits).unwrap();
        let after = tables_of(&bytes);
        for (id, _) in TARGETS {
            assert_eq!(table(&after, 11).get(id), Some("A much, much longer replacement name than the original"));
            assert_eq!(table(&after, 31).get(id), Some(big.as_str()));
        }
        assert!(Bnd4::parse(&bytes).unwrap().layout.verified);
    }

    #[test]
    fn nothing_panics_on_damaged_containers() {
        let good = item_bnd4();
        let mut seed = 0xDEAD_BEEF_CAFE_F00Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for cut in (0..good.len()).step_by(3) {
            let _ = patch_item_msgbnd(&good[..cut], &edits());
        }
        for _ in 0..400 {
            let mut x = good.clone();
            for _ in 0..(1 + next() % 6) {
                let at = (next() as usize) % x.len();
                x[at] = if next() % 2 == 0 { next() as u8 } else { x[at] ^ (1 << (next() % 8)) };
            }
            let _ = patch_item_msgbnd(&x, &edits());
        }
        // the compressed form, too
        let dcx_bytes = item_dcx();
        for _ in 0..200 {
            let mut x = dcx_bytes.clone();
            let at = (next() as usize) % x.len();
            x[at] ^= 1 << (next() % 8);
            if let Ok((inner, _)) = dcx::decode(&x) {
                let _ = patch_item_msgbnd(&inner, &edits());
            }
        }
        assert_eq!(DcxInfo::ds3_default().variant_name(), "DCX_DFLT_10000_44_9");
    }
}
