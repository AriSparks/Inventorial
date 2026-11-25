//! Dynamic screen scaling and layout recalculation.

use crate::config::InventorialConfig;

pub const VANILLA_INV_WIDTH: f32 = 176.0;
pub const VANILLA_INV_HEIGHT: f32 = 166.0;
pub const SLOT_SIZE: f32 = 18.0;

/// Represents calculated layout dimensions for the inventory screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InventoryLayoutMetrics {
    /// Bounding width of the expanded inventory.
    pub total_width: f32,
    /// Bounding height of the expanded inventory.
    pub total_height: f32,
    /// Screen center X offset.
    pub origin_x: f32,
    /// Screen center Y offset.
    pub origin_y: f32,
    /// Effective GUI scale to fit window without clipping.
    pub effective_gui_scale: f32,
    /// Extra rows count above main inventory.
    pub extra_rows: usize,
    /// Tool slots count on the right.
    pub tool_slots: usize,
    /// Number of offhand slot columns.
    pub offhand_columns: usize,
}

pub struct ScreenScaler;

impl ScreenScaler {
    /// Computes layout metrics and dynamic scaling.
    ///
    /// Recomputes whenever slot counts change or the window is resized.
    /// Ensures everything fits within the window without clipping at any resolution.
    pub fn compute_layout(
        window_width: f32,
        window_height: f32,
        user_gui_scale: f32,
        extra_rows: usize,
        tool_slots: usize,
        total_offhand_slots: usize,
        config: &InventorialConfig,
    ) -> InventoryLayoutMetrics {
        // Base dimensions
        let mut width = VANILLA_INV_WIDTH;
        let mut height = VANILLA_INV_HEIGHT;

        // Tool slots: 5 slots vertically on the right side (+28px margin & frame)
        if tool_slots > 0 {
            width += 28.0;
        }

        // Deep Pockets extra rows: each row adds 18px height above the inventory
        height += extra_rows as f32 * SLOT_SIZE;

        // Big Hands offhand extension: offhand slots are displayed to the right of paper doll
        // Default 4 fits in 1 column (4 slots high). Each +4 adds another column (+18px width)
        let offhand_columns = ((total_offhand_slots + 3) / 4).max(1);
        if offhand_columns > 1 {
            width += (offhand_columns - 1) as f32 * SLOT_SIZE;
        }

        // Calculate available area inside window
        let avail_w = (window_width - (config.screen_padding * 2.0)).max(100.0);
        let avail_h = (window_height - (config.screen_padding * 2.0)).max(100.0);

        // Find scale that fits both dimensions
        let max_fit_scale_w = avail_w / width;
        let max_fit_scale_h = avail_h / height;
        let max_fit_scale = max_fit_scale_w.min(max_fit_scale_h);

        // Clamped effective scale
        let base_scale = if user_gui_scale <= 0.0 { 1.0 } else { user_gui_scale };
        let effective_scale = base_scale
            .min(max_fit_scale)
            .clamp(config.min_gui_scale, config.max_gui_scale);

        // Centered position
        let scaled_w = width * effective_scale;
        let scaled_h = height * effective_scale;
        let origin_x = ((window_width - scaled_w) / 2.0).max(0.0);
        let origin_y = ((window_height - scaled_h) / 2.0).max(0.0);

        InventoryLayoutMetrics {
            total_width: width,
            total_height: height,
            origin_x,
            origin_y,
            effective_gui_scale: effective_scale,
            extra_rows,
            tool_slots,
            offhand_columns,
        }
    }
}
