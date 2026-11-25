//! Central configuration for the Inventorial mod.
//!
//! Provides customizable settings for slot counts, enchantment level caps,
//! keybinds, auto-switching behavior, GUI scaling limits, Ender Chest capacity,
//! and Dark Mode resource pack compatibility.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentedHotbarMode {
    Off,
    VisualOnly,
    On,
    FunctionOnly,
}

impl SegmentedHotbarMode {
    pub fn from_setting_str(s: &str) -> Self {
        match s {
            "visual_only" | "ONLY_VISUAL" | "VisualOnly" => Self::VisualOnly,
            "on" | "ON" | "On" => Self::On,
            "function_only" | "ONLY_FUNCTION" | "FunctionOnly" => Self::FunctionOnly,
            _ => Self::Off,
        }
    }

    pub fn as_setting_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::VisualOnly => "visual_only",
            Self::On => "on",
            Self::FunctionOnly => "function_only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DarkModeSetting {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone)]
pub struct InventorialConfig {
    /// Number of dedicated tool slots (Sword, Axe, Pickaxe, Shovel, Hoe).
    pub tool_slots_count: usize,
    /// Default number of offhand slots when no enchantments are applied.
    pub default_offhand_slots: usize,
    /// Maximum level for Deep Pockets enchantment.
    pub deep_pockets_max_level: u32,
    /// Slots added per Deep Pockets level (vanilla row size is 9).
    pub deep_pockets_slots_per_level: usize,
    /// Maximum level for Big Hands enchantment.
    pub big_hands_max_level: u32,
    /// Offhand slots added per Big Hands level.
    pub big_hands_slots_per_level: usize,
    /// Whether automatic tool switching is enabled.
    pub auto_switch_enabled: bool,
    /// Whether auto-switch restores the original item after action finishes.
    pub auto_switch_restore_on_finish: bool,
    /// Key code for cycling modifier (default: Left Control, GLFW key 341).
    pub cycle_modifier_key: i32,
    /// Minimum GUI scale clamp during dynamic inventory scaling.
    pub min_gui_scale: f32,
    /// Maximum GUI scale clamp during dynamic inventory scaling.
    pub max_gui_scale: f32,
    /// Margin in pixels to leave around the inventory screen edges.
    pub screen_padding: f32,
    /// Config option to set Ender Chest capacity in rows (vanilla is 3 rows = 27 slots; default 6 rows = 54 slots).
    pub ender_chest_rows: usize,
    /// Whether Totems of Undying function if placed in any offhand slot (not just the active one).
    pub totem_any_offhand_slot: bool,
    /// Enable Dark Mode resource pack detection and theme adaptation.
    pub dark_mode_support: bool,
    /// Dark Mode GUI theme mode (Auto detects dark resource packs, On forces dark, Off forces light).
    pub dark_mode_setting: DarkModeSetting,
    /// Allow empty-hand right-click on dirt/grass to till with toolbelt hoe, and shift-right-click to make path with toolbelt shovel.
    pub empty_hand_till_and_path: bool,
    /// Key name (e.g. "key.keyboard.v" or "v") to trigger elytra firework boost from offhand slots.
    pub firework_rocket_key: String,
    /// Whether tridents without Loyalty (or Riptide) can be thrown instead of melee-only.
    pub can_throw_unloyal_trident: bool,
    /// Whether scrolling/cycling offhand slots skips empty slots.
    pub skip_empty_offhand_slots: bool,
    /// Segmented hotbar mode (Off, VisualOnly, On, FunctionOnly).
    pub segmented_hotbar: SegmentedHotbarMode,
    /// Whether Infinity bows require no arrows in inventory.
    pub infinity_requires_no_arrows: bool,
    /// Whether main hand and offhand/utility belt are visually/functionally swapped.
    pub swapped_hands: bool,
}

impl Default for InventorialConfig {
    fn default() -> Self {
        Self {
            tool_slots_count: 5,
            default_offhand_slots: 2,
            deep_pockets_max_level: 4,
            deep_pockets_slots_per_level: 9,
            big_hands_max_level: 3,
            big_hands_slots_per_level: 2,
            auto_switch_enabled: true,
            auto_switch_restore_on_finish: true,
            cycle_modifier_key: 341, // GLFW_KEY_LEFT_CONTROL
            min_gui_scale: 0.5,
            max_gui_scale: 4.0,
            screen_padding: 10.0,
            ender_chest_rows: 6, // 6 rows * 9 slots = 54 slots (double chest size)
            totem_any_offhand_slot: true,
            dark_mode_support: true,
            dark_mode_setting: DarkModeSetting::Auto,
            empty_hand_till_and_path: true,
            firework_rocket_key: "key.keyboard.v".to_string(),
            can_throw_unloyal_trident: false,
            skip_empty_offhand_slots: false,
            segmented_hotbar: SegmentedHotbarMode::Off,
            infinity_requires_no_arrows: true,
            swapped_hands: false,
        }
    }
}
