//! A synthetic `item.msgbnd`: several text tables (names, summaries, descriptions of weapons, other categories) holding the
//! five test weapons under their original Dark Souls III names, in a BND4 of the format the game's text files use. All
//! the text is made up for the tests.
use super::bnd4::Bnd4Spec;
use crate::dcx::{self, DcxInfo};
use crate::fmg::FmgFile;

/// The five weapons of the test kit and the names they have in the game.
pub const TARGETS: [(u32, &str); 5] = [(2_000_000, "Shortsword"), (14_090_000, "Avelyn"), (404_000, "Standard Bolt"), (14_040_000, "Light Crossbow"), (14_190_000, "Repeating Crossbow")];

/// File ids of the tables in the container (names 10.., summaries 20.., descriptions 30..).
pub const WEAPON_NAME: i32 = 11;
pub const WEAPON_INFO: i32 = 21;
pub const WEAPON_CAPTION: i32 = 31;

pub struct Table {
    pub id: i32,
    pub name: String,
    pub fmg: FmgFile,
}

pub fn game_path(file: &str) -> String {
    format!("N:\\FDP\\data\\INTERROOT_win64\\msg\\ENGLISH\\{file}")
}

fn weapon_ids() -> Vec<(u32, String)> {
    let mut v: Vec<(u32, String)> = TARGETS.iter().map(|(id, name)| (*id, name.to_string())).collect();
    for (id, name) in [(1_000_000u32, "Dagger"), (1_100_000, "Parrying Dagger"), (2_100_000, "Longsword"), (405_000, "Heavy Bolt")] {
        v.push((id, name.to_string()));
    }
    v.sort();
    v
}

/// The tables: goods, weapon names / summaries / descriptions, armour names.
pub fn item_tables() -> Vec<Table> {
    let mut goods = FmgFile::new();
    for (id, name) in [(100u32, "Estus Flask"), (101, "Ashen Estus Flask"), (102, "Ember"), (103, "Homeward Bone")] {
        goods.set(id, name);
    }
    let mut names = FmgFile::new();
    let mut info = FmgFile::new();
    let mut caption = FmgFile::new();
    for (id, name) in weapon_ids() {
        names.set(id, &name);
        info.set(id, &format!("Made-up summary of the {name}"));
        caption.set(id, &format!("Made-up description of the {name}.\nIt has a second line of text,\nand a third one, so that this text reads as a long one."));
    }
    names.set_null(1_000_001); // a name that is absent in the game's table
    let mut armour = FmgFile::new();
    for (id, name) in [(10_000u32, "Knight Helm"), (10_001, "Knight Armor"), (11_000, "Hollow Soldier Helm")] {
        armour.set(id, name);
    }
    vec![
        Table { id: 10, name: game_path("GoodsName.fmg"), fmg: goods },
        Table { id: WEAPON_NAME, name: game_path("WeaponName.fmg"), fmg: names },
        Table { id: WEAPON_INFO, name: game_path("WeaponInfo.fmg"), fmg: info },
        Table { id: WEAPON_CAPTION, name: game_path("WeaponCaption.fmg"), fmg: caption },
        Table { id: 12, name: game_path("ProtectorName.fmg"), fmg: armour },
    ]
}

/// A BND4 (the game's text-file format, raw format byte 0x74, with the hash table) of the tables and some other files.
pub fn bnd4_of(tables: &[Table], extra: &[(i32, &str, Vec<u8>)]) -> Vec<u8> {
    let mut spec = Bnd4Spec::new(0x74);
    for t in tables {
        spec = spec.file(t.id, &t.name, &t.fmg.to_bytes());
    }
    for (id, name, data) in extra {
        spec = spec.file(*id, &game_path(name), data);
    }
    spec.build()
}

/// The container with the default tables and a file that is not a text table.
pub fn item_bnd4() -> Vec<u8> {
    bnd4_of(&item_tables(), &[(99, "Readme.txt", b"this one is not a text table".to_vec())])
}

/// The same, DCX-compressed like the game's file.
pub fn item_dcx() -> Vec<u8> {
    dcx::encode(&item_bnd4(), &DcxInfo::ds3_default()).expect("encode")
}
