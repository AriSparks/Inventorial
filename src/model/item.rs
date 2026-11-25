//! Item and tool type abstractions for Horizon.

use std::collections::HashMap;

/// Representation of an item stack in the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStack {
    pub id: String,
    pub count: u8,
    pub enchantments: HashMap<String, u32>,
    pub custom_data: HashMap<String, String>,
}

impl ItemStack {
    pub fn new(id: impl Into<String>, count: u8) -> Self {
        Self {
            id: id.into(),
            count,
            enchantments: HashMap::new(),
            custom_data: HashMap::new(),
        }
    }

    pub fn with_enchantment(mut self, enchant_id: impl Into<String>, level: u32) -> Self {
        self.enchantments.insert(enchant_id.into(), level);
        self
    }

    pub fn enchantment_level(&self, enchant_id: &str) -> u32 {
        self.enchantments.get(enchant_id).copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.id.is_empty() || self.id == "minecraft:air"
    }

    /// Detects if this item is a recognized tool type.
    pub fn tool_type(&self) -> Option<ToolType> {
        if self.is_empty() {
            return None;
        }

        let name = self.id.split(':').next_back().unwrap_or(&self.id);
        if name.ends_with("_sword") || name == "sword" || name == "trident" || name.ends_with("_trident") {
            Some(ToolType::Sword)
        } else if name.ends_with("_axe") || name == "axe" {
            Some(ToolType::Axe)
        } else if name.ends_with("_pickaxe") || name == "pickaxe" {
            Some(ToolType::Pickaxe)
        } else if name.ends_with("_shovel") || name == "shovel" {
            Some(ToolType::Shovel)
        } else if name.ends_with("_hoe") || name == "hoe" || name == "shears" || name.ends_with("_shears") {
            Some(ToolType::Hoe)
        } else {
            None
        }
    }

    /// Check if item is wearable as armor (for receiving Deep Pockets / Big Hands).
    pub fn is_armor(&self) -> bool {
        let name = self.id.strip_prefix("minecraft:").unwrap_or(&self.id);
        name.ends_with("_chestplate")
            || name.ends_with("_leggings")
            || name.ends_with("_helmet")
            || name.ends_with("_boots")
            || name.ends_with("_elytra")
            || name == "enchanted_book"
    }
}

/// The five dedicated tool categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolType {
    Sword = 0,
    Axe = 1,
    Pickaxe = 2,
    Shovel = 3,
    Hoe = 4,
}

impl ToolType {
    pub const ALL: [ToolType; 5] = [
        ToolType::Sword,
        ToolType::Axe,
        ToolType::Pickaxe,
        ToolType::Shovel,
        ToolType::Hoe,
    ];

    pub fn slot_index(self) -> usize {
        self as usize
    }

    pub fn from_slot_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(ToolType::Sword),
            1 => Some(ToolType::Axe),
            2 => Some(ToolType::Pickaxe),
            3 => Some(ToolType::Shovel),
            4 => Some(ToolType::Hoe),
            _ => None,
        }
    }

    pub fn silhouette_texture_path(self, dark_mode: bool) -> &'static str {
        if dark_mode {
            match self {
                ToolType::Sword => "inventorial:textures/gui/dark/slot_sword.png",
                ToolType::Axe => "inventorial:textures/gui/dark/slot_axe.png",
                ToolType::Pickaxe => "inventorial:textures/gui/dark/slot_pickaxe.png",
                ToolType::Shovel => "inventorial:textures/gui/dark/slot_shovel.png",
                ToolType::Hoe => "inventorial:textures/gui/dark/slot_hoe.png",
            }
        } else {
            match self {
                ToolType::Sword => "inventorial:textures/gui/slot_sword.png",
                ToolType::Axe => "inventorial:textures/gui/slot_axe.png",
                ToolType::Pickaxe => "inventorial:textures/gui/slot_pickaxe.png",
                ToolType::Shovel => "inventorial:textures/gui/slot_shovel.png",
                ToolType::Hoe => "inventorial:textures/gui/slot_hoe.png",
            }
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            ToolType::Sword => "slot.inventorial.sword",
            ToolType::Axe => "slot.inventorial.axe",
            ToolType::Pickaxe => "slot.inventorial.pickaxe",
            ToolType::Shovel => "slot.inventorial.shovel",
            ToolType::Hoe => "slot.inventorial.hoe",
        }
    }
}

/// Actions that trigger automatic tool switching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerAction {
    AttackingEntity,
    BreakingWood,
    MiningStoneOrOre,
    DiggingDirtOrSand,
    TillingSoil,
}

impl PlayerAction {
    pub fn required_tool(self) -> ToolType {
        match self {
            PlayerAction::AttackingEntity => ToolType::Sword,
            PlayerAction::BreakingWood => ToolType::Axe,
            PlayerAction::MiningStoneOrOre => ToolType::Pickaxe,
            PlayerAction::DiggingDirtOrSand => ToolType::Shovel,
            PlayerAction::TillingSoil => ToolType::Hoe,
        }
    }
}
