//! Hotbar HUD overlay for cycling offhand slots and temporary active tool slot.

use crate::model::{ItemStack, ModInventoryData};
use crate::slots::{AutoSwitchTracker, OffhandManager};
use crate::ui::Rect;

#[derive(Debug, Clone)]
pub struct HotbarOffhandSlotDisplay {
    pub bounds: Rect,
    pub item: Option<ItemStack>,
    /// Scale factor for item rendering (1.0 for main slot, 0.75 for previews).
    pub scale: f32,
    /// Color multiplier (RGBA, e.g. [0.6, 0.6, 0.6, 0.8] for darker preview).
    pub tint: [f32; 4],
}

/// Visual representation of the temporary tool slot displayed on the right-hand side of the hotbar.
#[derive(Debug, Clone)]
pub struct HotbarToolSlotDisplay {
    /// Bounding rectangle for the tool slot frame on the right side of the hotbar.
    pub bounds: Rect,
    /// The tool item rendered inside the slot.
    pub tool_item: ItemStack,
    /// Texture path for the slot frame (visual parity with the vanilla offhand slot frame).
    pub frame_texture: &'static str,
    /// Whether the slot is currently visible (true during active tool use, hidden otherwise).
    pub visible: bool,
}

pub struct HotbarOverlayRenderer;

impl HotbarOverlayRenderer {
    /// Computes the layout for the 3 offhand slots on the hotbar HUD:
    /// - Center: selected offhand slot (standard 18x18 scale 1.0, full brightness)
    /// - Left: previous offhand slot (smaller scale 0.75, darker tint [0.5, 0.5, 0.5, 0.7])
    /// - Right: next offhand slot (smaller scale 0.75, darker tint [0.5, 0.5, 0.5, 0.7])
    pub fn compute_offhand_display(
        hotbar_offhand_center_x: f32,
        hotbar_offhand_center_y: f32,
        gui_scale: f32,
        inventory: &ModInventoryData,
    ) -> Vec<HotbarOffhandSlotDisplay> {
        let mut displays = Vec::new();
        let (prev_item, current_item, next_item) = OffhandManager::get_hotbar_preview(inventory);

        let main_slot_size = 24.0 * gui_scale;
        let preview_slot_size = 18.0 * gui_scale;
        let spacing = 2.0 * gui_scale;

        // 1. Center: Active Offhand Slot (aligned on vanilla's offhand slot at hotbar_center_x - 109)
        displays.push(HotbarOffhandSlotDisplay {
            bounds: Rect {
                x: hotbar_offhand_center_x - (main_slot_size / 2.0),
                y: hotbar_offhand_center_y - (main_slot_size / 2.0),
                width: main_slot_size,
                height: main_slot_size,
            },
            item: current_item.cloned(),
            scale: 1.0,
            tint: [1.0, 1.0, 1.0, 1.0],
        });

        // If only 1 offhand slot exists, don't show side previews
        if inventory.offhand_slots.len() <= 1 {
            return displays;
        }

        // 2. Previous Offhand Slot (smaller, darker, immediately left of the active offhand slot)
        let left_x = hotbar_offhand_center_x - (main_slot_size / 2.0) - spacing - preview_slot_size;
        let left_y = hotbar_offhand_center_y - (preview_slot_size / 2.0);
        displays.push(HotbarOffhandSlotDisplay {
            bounds: Rect {
                x: left_x,
                y: left_y,
                width: preview_slot_size,
                height: preview_slot_size,
            },
            item: prev_item.cloned(),
            scale: 0.75,
            tint: [0.5, 0.5, 0.5, 0.75],
        });

        // 3. Next Offhand Slot (smaller, darker, to the left of the previous slot so it never overlaps the hotbar)
        let right_x = left_x - spacing - preview_slot_size;
        let right_y = hotbar_offhand_center_y - (preview_slot_size / 2.0);
        displays.push(HotbarOffhandSlotDisplay {
            bounds: Rect {
                x: right_x,
                y: right_y,
                width: preview_slot_size,
                height: preview_slot_size,
            },
            item: next_item.cloned(),
            scale: 0.75,
            tint: [0.5, 0.5, 0.5, 0.75],
        });

        displays
    }

    /// Computes the layout for the temporary active tool slot on the right-hand side of the hotbar.
    ///
    /// Symmetrically positioned to the right of the 9-slot hotbar (similar to the offhand slot on the left).
    /// If the player is currently using a tool from their dedicated tool slots, this returns `Some(...)`.
    /// When the action finishes, returns `None` (completely hidden).
    /// Dynamically adopts Dark Mode frame textures when dark_mode is active.
    pub fn compute_tool_slot_display(
        hotbar_center_x: f32,
        hotbar_center_y: f32,
        gui_scale: f32,
        switcher: &AutoSwitchTracker,
        dark_mode: bool,
    ) -> Option<HotbarToolSlotDisplay> {
        let active_tool = switcher.get_active_tool()?;

        let slot_size = 22.0 * gui_scale;
        let hotbar_half_width = 91.0 * gui_scale;
        let margin_from_hotbar = 7.0 * gui_scale;

        // Position to the right-hand side of the hotbar
        let slot_x = hotbar_center_x + hotbar_half_width + margin_from_hotbar;
        let slot_y = hotbar_center_y - (slot_size / 2.0);

        let frame_texture = if dark_mode {
            "inventorial:textures/gui/dark/slot_frame.png"
        } else {
            "inventorial:textures/gui/slot_frame.png"
        };

        Some(HotbarToolSlotDisplay {
            bounds: Rect {
                x: slot_x,
                y: slot_y,
                width: slot_size,
                height: slot_size,
            },
            tool_item: active_tool.clone(),
            frame_texture,
            visible: true,
        })
    }
}
