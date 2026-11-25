//! Inventory model holding custom slots for Inventorial.

use super::item::{ItemStack, ToolType};
use crate::config::InventorialConfig;

#[derive(Debug, Clone)]
pub struct ModInventoryData {
    /// 5 dedicated tool slots: Sword(0), Axe(1), Pickaxe(2), Shovel(3), Hoe(4).
    pub tool_slots: [Option<ItemStack>; 5],
    /// Configurable offhand slots (default 4, extended by Big Hands).
    pub offhand_slots: Vec<Option<ItemStack>>,
    /// Currently active offhand slot index.
    pub active_offhand_index: usize,
    /// Extra inventory rows added by Deep Pockets (9 slots per row).
    pub extra_rows: Vec<Option<ItemStack>>,
    /// Configurable Ender Chest storage (default 6 rows = 54 slots).
    pub ender_chest: Vec<Option<ItemStack>>,
}

impl Default for ModInventoryData {
    fn default() -> Self {
        Self::new(2, 0, 6)
    }
}

impl ModInventoryData {
    pub fn new(offhand_slots_count: usize, extra_rows_count: usize, ender_chest_rows: usize) -> Self {
        let offhand_len = offhand_slots_count.max(1);
        let ender_len = ender_chest_rows.max(1) * 9;
        Self {
            tool_slots: [None, None, None, None, None],
            offhand_slots: vec![None; offhand_len],
            active_offhand_index: 0,
            extra_rows: vec![None; extra_rows_count * 9],
            ender_chest: vec![None; ender_len],
        }
    }

    /// Returns the currently active offhand item stack.
    pub fn active_offhand(&self) -> Option<&ItemStack> {
        self.offhand_slots
            .get(self.active_offhand_index)
            .and_then(|opt| opt.as_ref())
    }

    /// Returns a mutable reference to the currently active offhand item stack.
    pub fn active_offhand_mut(&mut self) -> Option<&mut Option<ItemStack>> {
        let idx = self.active_offhand_index;
        self.offhand_slots.get_mut(idx)
    }

    /// Searches all offhand slots for a Totem of Undying and consumes one.
    /// Returns true if a totem was found and consumed (resurrecting the player).
    pub fn consume_totem_of_undying(&mut self) -> bool {
        for slot in &mut self.offhand_slots {
            if let Some(stack) = slot.as_mut() {
                let id = stack.id.strip_prefix("minecraft:").unwrap_or(&stack.id);
                if id == "totem_of_undying" && stack.count > 0 {
                    stack.count -= 1;
                    if stack.count == 0 {
                        *slot = None;
                    }
                    return true;
                }
            }
        }
        false
    }

    /// Cycles the active offhand slot with wrap-around.
    /// A positive delta cycles next; negative delta cycles previous.
    pub fn cycle_offhand(&mut self, delta: i32) -> usize {
        let total = self.offhand_slots.len();
        if total <= 1 {
            self.active_offhand_index = 0;
            return 0;
        }

        let current = self.active_offhand_index as isize;
        let next = (current + delta as isize).rem_euclid(total as isize) as usize;
        self.active_offhand_index = next;
        next
    }

    /// Validates whether a tool slot accepts the given item.
    pub fn can_place_in_tool_slot(slot_index: usize, item: &ItemStack) -> bool {
        if item.is_empty() {
            return true;
        }
        match ToolType::from_slot_index(slot_index) {
            Some(expected_type) => item.tool_type() == Some(expected_type),
            None => false,
        }
    }

    /// Get tool of a specific type if present in the dedicated slot.
    pub fn get_tool(&self, tool_type: ToolType) -> Option<&ItemStack> {
        self.tool_slots[tool_type.slot_index()].as_ref()
    }

    /// Dynamically adjusts capacities based on enchantment levels.
    ///
    /// If capacity shrinks (e.g., armor unequipped), excess items are removed
    /// from slots and returned as a list of `ItemStack` so the caller can drop
    /// them or place them into free player inventory space. Items are NEVER deleted.
    pub fn update_capacities(
        &mut self,
        deep_pockets_level: u32,
        big_hands_level: u32,
        config: &InventorialConfig,
    ) -> Vec<ItemStack> {
        let mut overflow_items = Vec::new();

        // 1. Update Offhand capacity (default + level * slots_per_level)
        let target_offhand_count = config.default_offhand_slots
            + (big_hands_level.min(config.big_hands_max_level) as usize
                * config.big_hands_slots_per_level);

        if self.offhand_slots.len() < target_offhand_count {
            self.offhand_slots.resize(target_offhand_count, None);
        } else if self.offhand_slots.len() > target_offhand_count {
            // Drain excess slots
            for item in self.offhand_slots.drain(target_offhand_count..) {
                if let Some(stack) = item {
                    if !stack.is_empty() {
                        overflow_items.push(stack);
                    }
                }
            }
        }

        if self.active_offhand_index >= self.offhand_slots.len() {
            self.active_offhand_index = self.offhand_slots.len().saturating_sub(1);
        }

        // 2. Update Deep Pockets extra rows
        let target_extra_slots = deep_pockets_level.min(config.deep_pockets_max_level) as usize
            * config.deep_pockets_slots_per_level;

        if self.extra_rows.len() < target_extra_slots {
            self.extra_rows.resize(target_extra_slots, None);
        } else if self.extra_rows.len() > target_extra_slots {
            // Drain excess extra rows
            for item in self.extra_rows.drain(target_extra_slots..) {
                if let Some(stack) = item {
                    if !stack.is_empty() {
                        overflow_items.push(stack);
                    }
                }
            }
        }

        // 3. Update Ender Chest capacity
        let target_ender_slots = config.ender_chest_rows.max(1) * 9;
        if self.ender_chest.len() < target_ender_slots {
            self.ender_chest.resize(target_ender_slots, None);
        } else if self.ender_chest.len() > target_ender_slots {
            for item in self.ender_chest.drain(target_ender_slots..) {
                if let Some(stack) = item {
                    if !stack.is_empty() {
                        overflow_items.push(stack);
                    }
                }
            }
        }

        overflow_items
    }

    /// Clear all slots (for death when keepInventory is false) and return all items.
    /// Note: Ender Chest contents persist across player death, matching vanilla rules.
    pub fn drain_all_items(&mut self) -> Vec<ItemStack> {
        let mut items = Vec::new();
        for slot in &mut self.tool_slots {
            if let Some(item) = slot.take() {
                if !item.is_empty() {
                    items.push(item);
                }
            }
        }
        for slot in &mut self.offhand_slots {
            if let Some(item) = slot.take() {
                if !item.is_empty() {
                    items.push(item);
                }
            }
        }
        for slot in &mut self.extra_rows {
            if let Some(item) = slot.take() {
                if !item.is_empty() {
                    items.push(item);
                }
            }
        }
        items
    }
}
