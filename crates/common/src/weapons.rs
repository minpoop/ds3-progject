//! Which Dark Souls III weapon rows the mashup is built from (read from the design sheet) and what kind of weapon a
//! row is, so a swing sound is only played for a melee weapon and a shot sound only for a crossbow. Pure: the hook reads
//! the numbers out of the game, everything that decides lives here and is tested on any OS.
use crate::triggers::Hand;
use serde::Deserialize;

/// A weapon of the design sheet (`design/sheets/weapons.json`, embedded at build time so the sheet stays the source of truth).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetWeapon {
    pub id: String,
    pub display_name: String,
    pub ranged: bool,
    /// the Dark Souls III weapon-table row the weapon is built from
    pub base_row: u32,
    /// that row's name in the game (English)
    pub base_name: String,
    /// ranged weapons: the ammunition row
    pub ammo_row: Option<u32>,
}

pub fn sheet_weapons() -> Vec<SheetWeapon> {
    #[derive(Deserialize)]
    struct Sheet {
        rows: Vec<Row>,
    }
    #[derive(Deserialize)]
    struct Row {
        id: String,
        display_name: String,
        kind: String,
        ds3_base_row: u32,
        ds3_base_name: String,
        #[serde(default)]
        ammo_row: Option<u32>,
    }
    let sheet: Sheet = serde_json::from_str(include_str!("../../../design/sheets/weapons.json")).expect("design/sheets/weapons.json is valid (checked by tests)");
    sheet
        .rows
        .into_iter()
        .map(|r| SheetWeapon { id: r.id, display_name: r.display_name, ranged: r.kind == "ranged", base_row: r.ds3_base_row, base_name: r.ds3_base_name, ammo_row: r.ammo_row })
        .collect()
}

/// What a weapon-table row is, as far as sounds are concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeaponClass {
    /// a sword, axe, spear, hammer, whip, claw ...: swinging it makes the melee sounds
    Melee,
    Bow,
    Crossbow,
    Shield,
    /// staff, flame, chime, talisman: casting, not swinging
    Catalyst,
    /// bare hands (and the game's test rows below 1 000 000)
    Unarmed,
    /// arrows and bolts
    Ammo,
    /// the row could not be looked up
    Unknown,
}

impl WeaponClass {
    /// Does an attack with this in the hand play a swing sound? An unknown weapon does: a missing answer must not silence
    /// the mashup, and the log says what was seen.
    pub fn makes_swing_sound(self) -> bool {
        matches!(self, WeaponClass::Melee | WeaponClass::Unknown)
    }

    pub fn name(self) -> &'static str {
        match self {
            WeaponClass::Melee => "melee",
            WeaponClass::Bow => "bow",
            WeaponClass::Crossbow => "crossbow",
            WeaponClass::Shield => "shield",
            WeaponClass::Catalyst => "catalyst",
            WeaponClass::Unarmed => "unarmed",
            WeaponClass::Ammo => "ammo",
            WeaponClass::Unknown => "unknown",
        }
    }
}

/// The weapon-table categories seen in the game's own table (checked on a real install: 1 straight sword, 8 staff / flame /
/// chime, 9 fists and claws, 10 bow, 11 crossbow, 12 shield, 13 arrow, 14 bolt).
pub const CAT_CATALYST: u8 = 8;
pub const CAT_BOW: u8 = 10;
pub const CAT_CROSSBOW: u8 = 11;
pub const CAT_SHIELD: u8 = 12;
pub const CAT_ARROW: u8 = 13;
pub const CAT_BOLT: u8 = 14;

/// The row without its upgrade level and infusion (`2000005` is a `+5` Shortsword; `2000100` a Heavy one): the param table
/// is looked up with this.
pub fn base_row(row: u32) -> u32 {
    row / 100 * 100
}

pub fn classify(row: u32, category: u8) -> WeaponClass {
    match category {
        CAT_ARROW | CAT_BOLT => WeaponClass::Ammo,
        CAT_BOW => WeaponClass::Bow,
        CAT_CROSSBOW => WeaponClass::Crossbow,
        CAT_SHIELD => WeaponClass::Shield,
        CAT_CATALYST => WeaponClass::Catalyst,
        _ if row < 1_000_000 => WeaponClass::Unarmed,
        _ => WeaponClass::Melee,
    }
}

/// The seven numbers at the start of the game's equipment record (just before the slot-to-inventory table): the arm style,
/// then which of the three slots of each kind is in use. Read as: arm style, left weapon slot, right weapon slot, left
/// arrow slot, right arrow slot, left bolt slot, right bolt slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChrAsm {
    /// 1 = one weapon in each hand, 2 = the left weapon held in both hands, 3 = the right weapon held in both hands
    pub arm_style: i32,
    pub left_slot: usize,
    pub right_slot: usize,
    pub raw: [i32; 7],
}

/// `None` unless the numbers look like what they are supposed to be (slots 0..=2, arm style 0..=3): a wrong guess about the
/// layout then means "unknown", never a wrong sound.
pub fn parse_chr_asm(raw: [i32; 7]) -> Option<ChrAsm> {
    let slot = |v: i32| (0..=2).contains(&v).then_some(v as usize);
    let (arm_style, left, right) = (raw[0], slot(raw[1])?, slot(raw[2])?);
    if !(0..=3).contains(&arm_style) || !raw[3..].iter().all(|v| (0..=2).contains(v)) {
        return None;
    }
    Some(ChrAsm { arm_style, left_slot: left, right_slot: right, raw })
}

impl ChrAsm {
    /// Index into the equipment-slot table of the weapon in use in `hand` (slots 0, 2, 4 are the left hand's three, 1, 3, 5 the right's).
    pub fn equipment_index(&self, hand: Hand) -> usize {
        match hand {
            Hand::Left => self.left_slot * 2,
            Hand::Right => self.right_slot * 2 + 1,
        }
    }

    /// Which weapon an attack input uses. Holding the left weapon in both hands puts it on the right-hand buttons.
    pub fn attacking_hand(&self, input: Hand) -> Hand {
        match (self.arm_style, input) {
            (2, Hand::Right) => Hand::Left,
            _ => input,
        }
    }
}

/// An inventory record as the game keeps it: 16 bytes, `u32 handle, u32 item id, u32 quantity, u32`. The item id of an
/// empty record is not a valid item (all ones, or the high bits of a category that does not exist).
pub fn decode_entry(bytes: &[u8; 16]) -> Option<(u32, u32)> {
    let id = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let qty = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    // categories: 0 weapon, 1 protector, 2 accessory, 4 goods (high nibble); anything else is empty or garbage
    let valid = id != 0 && id != u32::MAX && matches!(id >> 28, 0 | 1 | 2 | 4);
    valid.then_some((id, qty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_names_the_rows_the_mashup_is_built_from() {
        let w = sheet_weapons();
        let chainsword = w.iter().find(|w| w.id == "chainsword").expect("chainsword");
        assert_eq!((chainsword.base_row, chainsword.base_name.as_str(), chainsword.display_name.as_str(), chainsword.ranged), (2_000_000, "Shortsword", "Chainsword", false));
        let pistol = w.iter().find(|w| w.id == "bolt_pistol").expect("bolt pistol");
        assert_eq!((pistol.base_row, pistol.base_name.as_str(), pistol.display_name.as_str(), pistol.ranged), (14_090_000, "Avelyn", "Bolt Pistol", true));
        assert_eq!(pistol.ammo_row, Some(404_000));
        assert_eq!(chainsword.ammo_row, None);
    }

    #[test]
    fn rows_are_classified_by_the_games_own_categories() {
        assert_eq!(classify(2_000_000, 1), WeaponClass::Melee, "Shortsword");
        assert_eq!(classify(2_010_000, 1), WeaponClass::Melee, "Longsword");
        assert_eq!(classify(14_090_000, 11), WeaponClass::Crossbow, "Avelyn");
        assert_eq!(classify(14_010_000, 10), WeaponClass::Bow, "Short Bow");
        assert_eq!(classify(21_040_000, 12), WeaponClass::Shield);
        assert_eq!(classify(13_000_000, 8), WeaponClass::Catalyst);
        assert_eq!(classify(110_000, 9), WeaponClass::Unarmed, "Fists");
        assert_eq!(classify(11_000_000, 9), WeaponClass::Melee, "a claw is a weapon");
        assert_eq!(classify(404_000, 14), WeaponClass::Ammo);
        assert_eq!(classify(400_000, 13), WeaponClass::Ammo);
    }

    #[test]
    fn only_melee_weapons_make_swing_sounds_and_unknown_does_not_silence_them() {
        assert!(WeaponClass::Melee.makes_swing_sound());
        assert!(WeaponClass::Unknown.makes_swing_sound());
        for c in [WeaponClass::Bow, WeaponClass::Crossbow, WeaponClass::Shield, WeaponClass::Catalyst, WeaponClass::Unarmed, WeaponClass::Ammo] {
            assert!(!c.makes_swing_sound(), "{c:?}");
        }
    }

    #[test]
    fn upgrade_levels_and_infusions_share_the_base_row() {
        assert_eq!(base_row(2_000_005), 2_000_000);
        assert_eq!(base_row(2_000_105), 2_000_100, "an infusion is a separate row");
        assert_eq!(base_row(14_090_000), 14_090_000);
    }

    #[test]
    fn the_equipment_numbers_are_checked_before_they_are_believed() {
        let asm = parse_chr_asm([1, 0, 1, 0, 0, 0, 0]).expect("plausible");
        assert_eq!((asm.arm_style, asm.left_slot, asm.right_slot), (1, 0, 1));
        assert!(parse_chr_asm([1, 3, 1, 0, 0, 0, 0]).is_none(), "slot 3 does not exist");
        assert!(parse_chr_asm([1, -1, 1, 0, 0, 0, 0]).is_none());
        assert!(parse_chr_asm([9, 0, 1, 0, 0, 0, 0]).is_none(), "arm style out of range");
        assert!(parse_chr_asm([1, 0, 1, 7, 0, 0, 0]).is_none(), "an arrow slot out of range");
        assert!(parse_chr_asm([0x4000_0000, 0, 0, 0, 0, 0, 0]).is_none(), "a pointer is not an arm style");
    }

    #[test]
    fn slots_map_to_the_equipment_table_as_the_games_own_log_showed() {
        // from a real session: equipping the new sword in the right hand changed table entry 1, the crossbow in the left hand entry 0
        let asm = parse_chr_asm([1, 0, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!((asm.equipment_index(Hand::Left), asm.equipment_index(Hand::Right)), (0, 1));
        let asm = parse_chr_asm([1, 2, 1, 0, 0, 0, 0]).unwrap();
        assert_eq!((asm.equipment_index(Hand::Left), asm.equipment_index(Hand::Right)), (4, 3));
    }

    #[test]
    fn two_handing_the_left_weapon_puts_it_on_the_right_hand_buttons() {
        let one = parse_chr_asm([1, 0, 0, 0, 0, 0, 0]).unwrap();
        let both_left = parse_chr_asm([2, 0, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(one.attacking_hand(Hand::Right), Hand::Right);
        assert_eq!(both_left.attacking_hand(Hand::Right), Hand::Left);
        assert_eq!(both_left.attacking_hand(Hand::Left), Hand::Left);
    }

    #[test]
    fn inventory_records_are_decoded_and_garbage_is_not() {
        let mut rec = [0u8; 16];
        rec[0..4].copy_from_slice(&0x8000_0001u32.to_le_bytes());
        rec[4..8].copy_from_slice(&2_000_000u32.to_le_bytes());
        rec[8..12].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(decode_entry(&rec), Some((2_000_000, 1)));
        rec[4..8].copy_from_slice(&0x4000_0097u32.to_le_bytes());
        assert_eq!(decode_entry(&rec), Some((0x4000_0097, 1)), "goods");
        rec[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_entry(&rec), None, "empty");
        rec[4..8].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        assert_eq!(decode_entry(&rec), None, "not an item category");
        assert_eq!(decode_entry(&[0u8; 16]), None);
    }
}
