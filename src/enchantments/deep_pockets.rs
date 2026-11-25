//! Deep Pockets enchantment logic.

use crate::config::InventorialConfig;
use crate::model::ItemStack;

pub const DEEP_POCKETS_ID: &str = "inventorial:deep_pockets";

pub struct DeepPocketsEnchantment;

impl DeepPocketsEnchantment {
    /// Inspects the player's equipped armor items and determines the total effective level.
    /// Deep Pockets is strictly compatible only with leggings.
    pub fn calculate_level(armor_items: &[Option<ItemStack>], config: &InventorialConfig) -> u32 {
        let mut total_level = 0;
        for armor in armor_items.iter().flatten() {
            let id = armor.id.strip_prefix("minecraft:").unwrap_or(&armor.id);
            if id.ends_with("_leggings") || id == "leggings" {
                let lvl = armor.enchantment_level(DEEP_POCKETS_ID);
                total_level += lvl;
            }
        }
        total_level.min(config.deep_pockets_max_level)
    }

    /// Computes the number of extra rows (each with 9 slots) added by the given level.
    pub fn extra_rows_count(level: u32, config: &InventorialConfig) -> usize {
        level.min(config.deep_pockets_max_level) as usize
    }
}
