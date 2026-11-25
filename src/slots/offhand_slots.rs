//! Cycling offhand slots logic and rendering layout helpers.

use crate::model::{ItemStack, ModInventoryData};

/// Helpers for manipulating and querying offhand slots.
pub struct OffhandManager;

impl OffhandManager {
    /// Cycles to the next offhand slot (scroll down / positive delta).
    pub fn cycle_next(inventory: &mut ModInventoryData) -> usize {
        inventory.cycle_offhand(1)
    }

    /// Cycles to the previous offhand slot (scroll up / negative delta).
    pub fn cycle_prev(inventory: &mut ModInventoryData) -> usize {
        inventory.cycle_offhand(-1)
    }

    /// Retrieves previous, current, and next offhand items for the hotbar UI.
    ///
    /// Returns `(prev_item, current_item, next_item)`.
    pub fn get_hotbar_preview<'a>(
        inventory: &'a ModInventoryData,
    ) -> (
        Option<&'a ItemStack>,
        Option<&'a ItemStack>,
        Option<&'a ItemStack>,
    ) {
        let total = inventory.offhand_slots.len();
        if total == 0 {
            return (None, None, None);
        }

        let current_idx = inventory.active_offhand_index;
        let current = inventory.offhand_slots.get(current_idx).and_then(|i| i.as_ref());

        if total == 1 {
            return (None, current, None);
        }

        let prev_idx = (current_idx as isize - 1).rem_euclid(total as isize) as usize;
        let next_idx = (current_idx as isize + 1).rem_euclid(total as isize) as usize;

        let prev = inventory.offhand_slots.get(prev_idx).and_then(|i| i.as_ref());
        let next = inventory.offhand_slots.get(next_idx).and_then(|i| i.as_ref());

        (prev, current, next)
    }
}
