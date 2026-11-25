//! Dynamic inventory screen rendering, slot positioning, and hit testing.

use super::scaling::{InventoryLayoutMetrics, SLOT_SIZE, VANILLA_INV_WIDTH};
use crate::model::{ItemStack, ModInventoryData, ToolType};
use crate::network::SlotTypeTarget;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

#[derive(Debug, Clone)]
pub struct CustomSlotView {
    pub target: SlotTypeTarget,
    pub slot_index: usize,
    pub bounds: Rect,
    pub silhouette: Option<&'static str>,
    pub item: Option<ItemStack>,
}

pub struct InventoryScreenLayout;

impl InventoryScreenLayout {
    /// Builds all custom slot views based on the current layout metrics, player inventory,
    /// and dark mode resource pack setting.
    pub fn build_slots(
        metrics: &InventoryLayoutMetrics,
        inventory: &ModInventoryData,
        dark_mode: bool,
    ) -> Vec<CustomSlotView> {
        let mut slots = Vec::new();
        let scale = metrics.effective_gui_scale;
        let ox = metrics.origin_x;
        let oy = metrics.origin_y;

        // Extra rows offset shift: main inventory moves down by extra_rows * 18
        let extra_y_shift = metrics.extra_rows as f32 * SLOT_SIZE;

        // 1. Tool slots (5 slots vertically on the right side)
        let tool_base_x = ox + (VANILLA_INV_WIDTH + 6.0) * scale;
        let tool_base_y = oy + (extra_y_shift + 8.0) * scale;

        for (i, tool_type) in ToolType::ALL.iter().enumerate() {
            let bounds = Rect {
                x: tool_base_x,
                y: tool_base_y + (i as f32 * SLOT_SIZE) * scale,
                width: SLOT_SIZE * scale,
                height: SLOT_SIZE * scale,
            };

            let item = inventory.tool_slots[i].clone();
            let silhouette = if item.is_none() {
                Some(tool_type.silhouette_texture_path(dark_mode))
            } else {
                None
            };

            slots.push(CustomSlotView {
                target: SlotTypeTarget::Tool,
                slot_index: i,
                bounds,
                silhouette,
                item,
            });
        }

        // 2. Offhand slots (vertically to the right of player paper-doll, opposite armor slots)
        // Paper-doll is at x=26..74; armor slots are at x=8 (4 vertical slots: 8, 26, 44, 62).
        // Offhand slots are located at x=77. Big Hands adds columns to the right (+18 each).
        let offhand_base_x = ox + 77.0 * scale;
        let offhand_base_y = oy + (extra_y_shift + 8.0) * scale;

        for (i, item) in inventory.offhand_slots.iter().enumerate() {
            let col = i / 4;
            let row = i % 4;

            let bounds = Rect {
                x: offhand_base_x + (col as f32 * SLOT_SIZE) * scale,
                y: offhand_base_y + (row as f32 * SLOT_SIZE) * scale,
                width: SLOT_SIZE * scale,
                height: SLOT_SIZE * scale,
            };

            slots.push(CustomSlotView {
                target: SlotTypeTarget::Offhand,
                slot_index: i,
                bounds,
                silhouette: None,
                item: item.clone(),
            });
        }

        // 3. Deep Pockets extra inventory rows (placed above the standard inventory slots)
        // Vanilla inventory slots start at x=8, and main inventory top is at y=84.
        // Deep Pockets rows sit directly above the main inventory: y = 84 - (row + 1) * 18.
        let extra_base_x = ox + 8.0 * scale;
        let extra_top_y = oy + 8.0 * scale; // top of the expanded frame

        for row in 0..metrics.extra_rows {
            for col in 0..9 {
                let slot_idx = row * 9 + col;
                if slot_idx >= inventory.extra_rows.len() {
                    break;
                }

                let bounds = Rect {
                    x: extra_base_x + (col as f32 * SLOT_SIZE) * scale,
                    y: extra_top_y + (row as f32 * SLOT_SIZE) * scale,
                    width: SLOT_SIZE * scale,
                    height: SLOT_SIZE * scale,
                };

                slots.push(CustomSlotView {
                    target: SlotTypeTarget::ExtraRow,
                    slot_index: slot_idx,
                    bounds,
                    silhouette: None,
                    item: inventory.extra_rows.get(slot_idx).cloned().flatten(),
                });
            }
        }

        slots
    }

    /// Builds all custom slot views anchored directly to vanilla's `event.screen.rect` (`[rx, ry, rw, rh]`).
    ///
    /// This guarantees zero overlap with the vanilla inventory frame or player paper-doll:
    /// - 5 tool slots sit cleanly in a side panel to the right of `rx + rw`.
    /// - Offhand slots sit vertically at `rx + 76` (to the right of the paper-doll, opposite armor),
    ///   with Big Hands extending rightwards.
    /// - Deep Pockets extra rows sit in an attached panel directly above `ry`.
    pub fn build_slots_anchored(
        inv_rect: [f32; 4],
        inventory: &ModInventoryData,
        dark_mode: bool,
    ) -> Vec<CustomSlotView> {
        let mut slots = Vec::new();
        let [rx, ry, rw, _rh] = inv_rect;
        let scale = (rw / VANILLA_INV_WIDTH).max(0.5);
        let extra_rows = inventory.extra_rows.len() / 9;

        // 1. Tool slots (5 slots vertically in an attached panel on the right of the vanilla frame)
        let tool_base_x = rx + rw + 6.0 * scale;
        let tool_base_y = ry + 8.0 * scale;

        for (i, tool_type) in ToolType::ALL.iter().enumerate() {
            let bounds = Rect {
                x: tool_base_x,
                y: tool_base_y + (i as f32 * SLOT_SIZE) * scale,
                width: SLOT_SIZE * scale,
                height: SLOT_SIZE * scale,
            };

            let item = inventory.tool_slots[i].clone();
            let silhouette = if item.is_none() {
                Some(tool_type.silhouette_texture_path(dark_mode))
            } else {
                None
            };

            slots.push(CustomSlotView {
                target: SlotTypeTarget::Tool,
                slot_index: i,
                bounds,
                silhouette,
                item,
            });
        }

        // 2. Offhand slots (vertically at rx + 76, to the right of the paper-doll, opposite armor)
        let offhand_base_x = rx + 76.0 * scale;
        let offhand_base_y = ry + 7.0 * scale;

        for (i, item) in inventory.offhand_slots.iter().enumerate() {
            let col = i / 4;
            let row = i % 4;

            let bounds = Rect {
                x: offhand_base_x + (col as f32 * SLOT_SIZE) * scale,
                y: offhand_base_y + (row as f32 * SLOT_SIZE) * scale,
                width: SLOT_SIZE * scale,
                height: SLOT_SIZE * scale,
            };

            slots.push(CustomSlotView {
                target: SlotTypeTarget::Offhand,
                slot_index: i,
                bounds,
                silhouette: None,
                item: item.clone(),
            });
        }

        // 3. Deep Pockets extra rows (attached panel directly above the vanilla inventory panel)
        if extra_rows > 0 {
            let panel_h = (extra_rows as f32 * SLOT_SIZE + 8.0) * scale;
            let panel_y = (ry - panel_h + 2.0 * scale).max(2.0);
            let extra_base_x = rx + 7.0 * scale;
            let extra_top_y = panel_y + 4.0 * scale;

            for row in 0..extra_rows {
                for col in 0..9 {
                    let slot_idx = row * 9 + col;
                    if slot_idx >= inventory.extra_rows.len() {
                        break;
                    }

                    let bounds = Rect {
                        x: extra_base_x + (col as f32 * SLOT_SIZE) * scale,
                        y: extra_top_y + (row as f32 * SLOT_SIZE) * scale,
                        width: SLOT_SIZE * scale,
                        height: SLOT_SIZE * scale,
                    };

                    slots.push(CustomSlotView {
                        target: SlotTypeTarget::ExtraRow,
                        slot_index: slot_idx,
                        bounds,
                        silhouette: None,
                        item: inventory.extra_rows.get(slot_idx).cloned().flatten(),
                    });
                }
            }
        }

        slots
    }

    /// Hit-tests custom slots anchored to `event.screen.rect`.
    pub fn hit_test_anchored(
        inv_rect: [f32; 4],
        inventory: &ModInventoryData,
        mouse_x: f32,
        mouse_y: f32,
    ) -> Option<(SlotTypeTarget, usize)> {
        let views = Self::build_slots_anchored(inv_rect, inventory, false);
        for view in views {
            if view.bounds.contains(mouse_x, mouse_y) {
                return Some((view.target, view.slot_index));
            }
        }
        None
    }

    /// Performs hit-testing to detect which custom slot the mouse is hovering over.
    pub fn hit_test(
        metrics: &InventoryLayoutMetrics,
        inventory: &ModInventoryData,
        mouse_x: f32,
        mouse_y: f32,
    ) -> Option<(SlotTypeTarget, usize)> {
        let views = Self::build_slots(metrics, inventory, false);
        for view in views {
            if view.bounds.contains(mouse_x, mouse_y) {
                return Some((view.target, view.slot_index));
            }
        }
        None
    }
}

/// Identifies any slot on the unified Inventorial screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnifiedSlotId {
    /// Main 0..36 (0..9 hotbar, 9..36 main 3x9).
    Main(u8),
    /// Armor 0..4 (0=helmet, 1=chestplate, 2=leggings, 3=boots).
    Armor(u8),
    /// Offhand 0..offhand_count.
    Offhand(u8),
    /// Toolbelt 0..5 (0=sword, 1=axe, 2=pickaxe, 3=shovel, 4=hoe).
    Toolbelt(u8),
    /// Deep Pockets extra row slots 0..(extra_rows * 9).
    Extra(u8),
    /// 2x2 crafting grid inputs 0..4.
    CraftInput(u8),
    /// 2x2 crafting output slot.
    CraftOutput,
    /// Ender chest slot 0..(rows * 9).
    EnderChest(u8),
    /// Extra Elytra slot (worn alongside chestplate).
    Elytra,
}

#[derive(Debug, Clone, Copy)]
pub struct UnifiedSlotPos {
    pub id: UnifiedSlotId,
    /// Top-left of the 18x18 slot box in window-local GUI coordinates.
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct UnifiedLayout {
    pub width: f32,
    pub height: f32,
    pub paper_doll_rect: [f32; 4],
    pub craft_label_pos: [f32; 2],
    pub craft_arrow_pos: [f32; 2],
    pub recipe_book_btn: [f32; 4],
    pub slots: Vec<UnifiedSlotPos>,
}

impl UnifiedLayout {
    pub fn origin(&self, screen_w: f32, screen_h: f32) -> [f32; 2] {
        self.origin_with_book(screen_w, screen_h, false)
    }

    pub fn origin_with_book(&self, screen_w: f32, screen_h: f32, book_open: bool) -> [f32; 2] {
        let shift = if book_open { 77.0 } else { 0.0 };
        let ox = (((screen_w - self.width) * 0.5 + shift)
            .min(screen_w - self.width - 4.0)
            .max(180.0 * f32::from(book_open)))
        .floor();
        [ox, ((screen_h - self.height) * 0.5).floor()]
    }

    pub fn hit_test(&self, origin: [f32; 2], mouse_x: f32, mouse_y: f32) -> Option<UnifiedSlotId> {
        let lx = mouse_x - origin[0];
        let ly = mouse_y - origin[1];
        self.slots
            .iter()
            .find(|s| lx >= s.x && lx < s.x + SLOT_SIZE && ly >= s.y && ly < s.y + SLOT_SIZE)
            .map(|s| s.id)
    }
}

/// Computes the unified Inventorial inventory window layout matching the mockup:
/// - Armor (left) + Paper-doll + Offhand grid (pushing Crafting right)
/// - 2x2 Crafting UI on top-right
/// - Deep Pockets extra rows under Paper-doll & above regular 3x9 inventory
/// - Toolbelt integrated into the right side of the main window
pub fn compute_unified_layout(extra_rows: usize, offhand_count: usize) -> UnifiedLayout {
    let mut slots = Vec::with_capacity(50 + extra_rows * 9 + offhand_count);

    // 1. Armor (top-left)
    for i in 0..4 {
        slots.push(UnifiedSlotPos {
            id: UnifiedSlotId::Armor(i as u8),
            x: 8.0,
            y: 8.0 + i as f32 * SLOT_SIZE,
        });
    }

    // 2. Paper-doll box (to the right of armor, aligned with 4x18 armor/offhand slots: x=26..77, y=8..80)
    let paper_doll_rect = [26.0, 8.0, 77.0, 80.0];

    // 3. Offhand slots (to the right of paper-doll, 2 columns x up to 4 rows, pushing crafting right)
    let offhand_cols = if offhand_count <= 8 {
        2
    } else {
        (offhand_count + 3) / 4
    };
    let offhand_base_x = 77.0;
    let offhand_base_y = 8.0;
    for i in 0..offhand_count {
        let col = i % offhand_cols;
        let row = i / offhand_cols;
        slots.push(UnifiedSlotPos {
            id: UnifiedSlotId::Offhand(i as u8),
            x: offhand_base_x + col as f32 * SLOT_SIZE,
            y: offhand_base_y + row as f32 * SLOT_SIZE,
        });
    }

    // 4. Crafting UI (pushed right by offhand columns)
    let craft_x = offhand_base_x + offhand_cols as f32 * SLOT_SIZE + 6.0;
    let craft_label_pos = [craft_x, 6.0];
    for i in 0..4 {
        slots.push(UnifiedSlotPos {
            id: UnifiedSlotId::CraftInput(i as u8),
            x: craft_x + (i % 2) as f32 * SLOT_SIZE,
            y: 18.0 + (i / 2) as f32 * SLOT_SIZE,
        });
    }
    let craft_arrow_pos = [craft_x + 38.0, 27.0];
    slots.push(UnifiedSlotPos {
        id: UnifiedSlotId::CraftOutput,
        x: craft_x + 56.0,
        y: 26.0,
    });
    let recipe_book_btn = [craft_x + 8.0, 58.0, 20.0, 18.0];

    // 5. Deep Pockets extra rows (under paper-doll/crafting, above main inventory)
    let extra_y = 84.0;
    for row in 0..extra_rows {
        for col in 0..9 {
            let idx = row * 9 + col;
            slots.push(UnifiedSlotPos {
                id: UnifiedSlotId::Extra(idx as u8),
                x: 8.0 + col as f32 * SLOT_SIZE,
                y: extra_y + row as f32 * SLOT_SIZE,
            });
        }
    }

    // 6. Regular 3x9 inventory + 1x9 hotbar (pushed down by extra rows)
    let inv_y = if extra_rows > 0 {
        extra_y + extra_rows as f32 * SLOT_SIZE + 6.0
    } else {
        84.0
    };
    for i in 9..36 {
        let idx = i - 9;
        slots.push(UnifiedSlotPos {
            id: UnifiedSlotId::Main(i as u8),
            x: 8.0 + (idx % 9) as f32 * SLOT_SIZE,
            y: inv_y + (idx / 9) as f32 * SLOT_SIZE,
        });
    }
    let hotbar_y = inv_y + 3.0 * SLOT_SIZE + 4.0;
    for i in 0..9 {
        slots.push(UnifiedSlotPos {
            id: UnifiedSlotId::Main(i as u8),
            x: 8.0 + i as f32 * SLOT_SIZE,
            y: hotbar_y,
        });
    }

    // 7. Toolbelt (5 vertical slots integrated into the right side of the main window)
    let toolbelt_x = 8.0 + 9.0 * SLOT_SIZE + 6.0;
    let toolbelt_y = hotbar_y + SLOT_SIZE - 5.0 * SLOT_SIZE;
    for i in 0..5 {
        slots.push(UnifiedSlotPos {
            id: UnifiedSlotId::Toolbelt(i as u8),
            x: toolbelt_x,
            y: toolbelt_y + i as f32 * SLOT_SIZE,
        });
    }

    let width = (toolbelt_x + SLOT_SIZE + 8.0).max(craft_x + 56.0 + SLOT_SIZE + 8.0);
    let height = (hotbar_y + SLOT_SIZE + 8.0).max(toolbelt_y + 5.0 * SLOT_SIZE + 8.0);

    UnifiedLayout {
        width,
        height,
        paper_doll_rect,
        craft_label_pos,
        craft_arrow_pos,
        recipe_book_btn,
        slots,
    }
}

/// Computes the layout for the configurable Ender Chest screen (`rows` rows of 9 + player inventory).
pub fn compute_ender_chest_layout(rows: usize) -> UnifiedLayout {
    let rows = rows.clamp(1, 9);
    let mut slots = Vec::with_capacity(rows * 9 + 36);
    let top_y = 18.0;
    for row in 0..rows {
        for col in 0..9 {
            let idx = row * 9 + col;
            slots.push(UnifiedSlotPos {
                id: UnifiedSlotId::EnderChest(idx as u8),
                x: 8.0 + col as f32 * SLOT_SIZE,
                y: top_y + row as f32 * SLOT_SIZE,
            });
        }
    }
    let inv_y = top_y + rows as f32 * SLOT_SIZE + 14.0;
    for i in 9..36 {
        let idx = i - 9;
        slots.push(UnifiedSlotPos {
            id: UnifiedSlotId::Main(i as u8),
            x: 8.0 + (idx % 9) as f32 * SLOT_SIZE,
            y: inv_y + (idx / 9) as f32 * SLOT_SIZE,
        });
    }
    let hotbar_y = inv_y + 3.0 * SLOT_SIZE + 4.0;
    for i in 0..9 {
        slots.push(UnifiedSlotPos {
            id: UnifiedSlotId::Main(i as u8),
            x: 8.0 + i as f32 * SLOT_SIZE,
            y: hotbar_y,
        });
    }
    UnifiedLayout {
        width: 176.0,
        height: hotbar_y + SLOT_SIZE + 8.0,
        paper_doll_rect: [0.0; 4],
        craft_label_pos: [8.0, 6.0],
        craft_arrow_pos: [8.0, inv_y - 11.0],
        recipe_book_btn: [0.0; 4],
        slots,
    }
}
