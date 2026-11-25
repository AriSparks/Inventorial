//! Big Hands enchantment logic.

use crate::config::InventorialConfig;
use crate::model::ItemStack;

pub const BIG_HANDS_ID: &str = "inventorial:big_hands";

pub struct BigHandsEnchantment;

impl BigHandsEnchantment {
    /// Inspects the player's equipped armor items and determines the total effective level.
    /// Big Hands is strictly compatible only with chestplates.
    pub fn calculate_level(armor_items: &[Option<ItemStack>], config: &InventorialConfig) -> u32 {
        let mut total_level = 0;
        for armor in armor_items.iter().flatten() {
            let id = armor.id.strip_prefix("minecraft:").unwrap_or(&armor.id);
            if id.ends_with("_chestplate") || id == "chestplate" {
                let lvl = armor.enchantment_level(BIG_HANDS_ID);
                total_level += lvl;
            }
        }
        total_level.min(config.big_hands_max_level)
    }

    /// Computes the total number of offhand slots available.
    pub fn total_offhand_slots(level: u32, config: &InventorialConfig) -> usize {
        config.default_offhand_slots
            + (level.min(config.big_hands_max_level) as usize * config.big_hands_slots_per_level)
    }
}
