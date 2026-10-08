//! What the player is holding right now, read from the game's own equipment record: which of the three slots of each hand
//! is in use, which inventory entry sits in it and what kind of weapon that is. Read-only; every address is checked with
//! `ReadProcessMemory` (which answers with an error, not a crash, for a bad one), and a layout that does not look like what
//! it should makes the answer "unknown", never a guess.
use super::memscan;
use ashen_common::{
    triggers::Hand,
    weapons::{self, ChrAsm, WeaponClass},
};
use darksouls3::{
    param::EQUIP_PARAM_WEAPON_ST,
    sprj::{CSRegulationManager, ItemCategory, ItemId, PlayerGameData},
};

/// Offset of the seven slot numbers inside the equipment record (the crate's `EquipGameData`: a vtable pointer, then 0x1c
/// bytes, then the slot-to-inventory table).
const ASM_OFFSET: usize = 0x08;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandItem {
    /// the inventory item id as the game stores it (weapon table row + upgrade level)
    pub raw_id: u32,
    pub class: WeaponClass,
}

/// The two hands. `asm: None` = the equipment record did not look as expected, nothing else is trusted then.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Hands {
    pub asm: Option<ChrAsm>,
    /// the seven numbers as read, for the log
    pub raw: [i32; 7],
    pub left: Option<HandItem>,
    pub right: Option<HandItem>,
}

impl Hands {
    /// The class of the weapon an attack with `input` uses; `None` when the hands could not be read.
    pub fn class_for(&self, input: Hand) -> Option<WeaponClass> {
        let asm = self.asm?;
        let item = match asm.attacking_hand(input) {
            Hand::Left => self.left,
            Hand::Right => self.right,
        };
        Some(item.map_or(WeaponClass::Unarmed, |i| i.class))
    }

    pub fn has_crossbow(&self) -> bool {
        [self.left, self.right].iter().flatten().any(|i| i.class == WeaponClass::Crossbow)
    }

    /// One line for the log.
    pub fn describe(&self) -> String {
        let item = |i: &Option<HandItem>| match i {
            Some(i) => format!("{} (item 0x{:08X})", i.class.name(), i.raw_id),
            None => "empty".to_string(),
        };
        match self.asm {
            Some(a) => format!("left hand slot {}: {}; right hand slot {}: {}; arm style {}; raw {:?}", a.left_slot + 1, item(&self.left), a.right_slot + 1, item(&self.right), a.arm_style, self.raw),
            None => format!("the equipment record does not look as expected (raw {:?}); hands unknown", self.raw),
        }
    }
}

/// The weapon-table category of a row (any upgrade level of it), if the game has such a row.
pub fn weapon_category(reg: &CSRegulationManager, raw_id: u32) -> Option<u8> {
    let item = ItemId::try_from(raw_id).ok()?;
    if item.category() != ItemCategory::Weapon {
        return None;
    }
    reg.get_param::<EQUIP_PARAM_WEAPON_ST>().get(u64::from(weapons::base_row(item.param_id()))).map(|r| r.weapon_category())
}

fn classify_item(reg: Option<&CSRegulationManager>, raw_id: u32) -> WeaponClass {
    let Some(reg) = reg else { return WeaponClass::Unknown };
    match weapon_category(reg, raw_id) {
        Some(cat) => weapons::classify(weapons::base_row(raw_id & 0x0FFF_FFFF), cat),
        None => WeaponClass::Unknown,
    }
}

/// The inventory entry (item id) at a table index, using the game's own indexing: indexes below the key-item capacity are
/// key items, the rest normal items.
unsafe fn inventory_item(pgd: &PlayerGameData, index: i32) -> Option<u32> {
    if index < 0 {
        return None;
    }
    let items = &pgd.equipment.equip_inventory_data.items_data;
    let (key_cap, normal_cap) = (items.key_items_capacity, items.normal_items_capacity);
    if key_cap > 1024 || normal_cap > 8192 {
        return None;
    }
    let index = index as u32;
    let addr = if index < key_cap {
        items.key_items_head.as_ptr() as usize + index as usize * 16
    } else if index - key_cap < normal_cap {
        items.normal_items_head.as_ptr() as usize + (index - key_cap) as usize * 16
    } else {
        return None;
    };
    let mut rec = [0u8; 16];
    if memscan::read_into(addr, &mut rec) != 16 {
        return None;
    }
    weapons::decode_entry(&rec).map(|(id, _)| id)
}

/// Read both hands. Cheap enough for every frame: seven numbers, two inventory records, two parameter rows.
///
/// # Safety
/// `pgd` must be the player's live `PlayerGameData` (the caller has checked that it is readable).
pub unsafe fn read(pgd: &PlayerGameData, reg: Option<&CSRegulationManager>) -> Hands {
    let base = &pgd.equipment as *const _ as usize;
    let mut buf = [0u8; 28];
    if memscan::read_into(base + ASM_OFFSET, &mut buf) != buf.len() {
        return Hands::default();
    }
    let mut raw = [0i32; 7];
    for (k, v) in raw.iter_mut().enumerate() {
        *v = i32::from_le_bytes([buf[k * 4], buf[k * 4 + 1], buf[k * 4 + 2], buf[k * 4 + 3]]);
    }
    let Some(asm) = weapons::parse_chr_asm(raw) else { return Hands { raw, ..Hands::default() } };
    let table = pgd.equipment.equipment_indexes;
    let hand = |h: Hand| -> Option<HandItem> {
        let slot = *table.get(asm.equipment_index(h))?;
        let raw_id = inventory_item(pgd, slot)?;
        Some(HandItem { raw_id, class: classify_item(reg, raw_id) })
    };
    Hands { asm: Some(asm), raw, left: hand(Hand::Left), right: hand(Hand::Right) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(class: WeaponClass) -> Option<HandItem> {
        Some(HandItem { raw_id: 2_000_000, class })
    }

    fn hands(left: Option<HandItem>, right: Option<HandItem>, arm_style: i32) -> Hands {
        let raw = [arm_style, 0, 0, 0, 0, 0, 0];
        Hands { asm: weapons::parse_chr_asm(raw), raw, left, right }
    }

    #[test]
    fn each_input_uses_the_weapon_of_its_hand() {
        let h = hands(item(WeaponClass::Crossbow), item(WeaponClass::Melee), 1);
        assert_eq!(h.class_for(Hand::Right), Some(WeaponClass::Melee));
        assert_eq!(h.class_for(Hand::Left), Some(WeaponClass::Crossbow));
        assert!(h.has_crossbow());
    }

    #[test]
    fn an_empty_hand_is_unarmed_and_an_unreadable_record_is_unknown() {
        let h = hands(None, item(WeaponClass::Melee), 1);
        assert_eq!(h.class_for(Hand::Left), Some(WeaponClass::Unarmed));
        assert_eq!(Hands::default().class_for(Hand::Right), None, "no answer, not a guess");
        assert!(!Hands::default().has_crossbow());
    }

    #[test]
    fn two_handing_the_left_weapon_uses_it_for_the_right_hand_buttons() {
        let h = hands(item(WeaponClass::Melee), item(WeaponClass::Crossbow), 2);
        assert_eq!(h.class_for(Hand::Right), Some(WeaponClass::Melee));
    }

    #[test]
    fn the_description_says_what_was_read() {
        let h = hands(item(WeaponClass::Crossbow), item(WeaponClass::Melee), 1);
        let d = h.describe();
        assert!(d.contains("left hand slot 1: crossbow") && d.contains("right hand slot 1: melee") && d.contains("arm style 1"), "{d}");
        assert!(Hands::default().describe().contains("hands unknown"));
    }
}
