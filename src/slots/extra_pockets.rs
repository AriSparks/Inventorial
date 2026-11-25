//! Deep Pockets extra inventory rows management and item evacuation.

use crate::model::{ItemStack, ModInventoryData};

pub struct ExtraPocketsManager;

impl ExtraPocketsManager {
    /// Safe item evacuation: tries to fit evacuated items into the vanilla 36 slots
    /// (hotbar + main inventory). Any items that do not fit are returned to be dropped into the world.
    pub fn evacuate_overflow(
        overflow_items: Vec<ItemStack>,
        vanilla_inventory: &mut [Option<ItemStack>],
    ) -> Vec<ItemStack> {
        let mut unplaced = Vec::new();

        for mut item in overflow_items {
            if item.is_empty() {
                continue;
            }

            // 1. Try to merge into existing stacks
            for slot in vanilla_inventory.iter_mut() {
                if let Some(target) = slot.as_mut() {
                    if target.id == item.id
                        && target.enchantments == item.enchantments
                        && target.count < 64
                    {
                        let space = 64 - target.count;
                        let transfer = item.count.min(space);
                        target.count += transfer;
                        item.count -= transfer;
                        if item.count == 0 {
                            break;
                        }
                    }
                }
            }

            // 2. If item still has count, place in first empty slot
            if item.count > 0 {
                for slot in vanilla_inventory.iter_mut() {
                    if slot.is_none() {
                        *slot = Some(item.clone());
                        item.count = 0;
                        break;
                    }
                }
            }

            // 3. If still not placed, item must be dropped on ground
            if item.count > 0 {
                unplaced.push(item);
            }
        }

        unplaced
    }

    /// Shift-click logic: moves an item stack from a custom slot into vanilla inventory.
    pub fn shift_click_from_custom(
        stack: &mut ItemStack,
        vanilla_inv: &mut [Option<ItemStack>],
    ) -> bool {
        if stack.is_empty() {
            return false;
        }

        let unplaced = Self::evacuate_overflow(vec![stack.clone()], vanilla_inv);
        if unplaced.is_empty() {
            stack.count = 0;
            true
        } else {
            stack.count = unplaced[0].count;
            true
        }
    }

    /// Shift-click logic: moves an item stack from vanilla inventory into custom extra rows or dedicated tool slot.
    pub fn shift_click_from_vanilla(
        stack: &mut ItemStack,
        custom_inv: &mut ModInventoryData,
    ) -> bool {
        if stack.is_empty() {
            return false;
        }

        if let Some(tool_type) = stack.tool_type() {
            let slot = tool_type.slot_index();
            if custom_inv.tool_slots[slot].is_none() {
                custom_inv.tool_slots[slot] = Some(stack.clone());
                stack.count = 0;
                return true;
            }
        }

        for slot in &mut custom_inv.extra_rows {
            if slot.is_none() {
                *slot = Some(stack.clone());
                stack.count = 0;
                return true;
            }
        }

        false
    }
}
