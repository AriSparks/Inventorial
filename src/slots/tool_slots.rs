//! Dedicated tool slots logic and seamless temporary tool slot HUD activation.

use crate::model::{ItemStack, ModInventoryData, PlayerAction, ToolType};

/// Tracks the active temporary tool slot state for the hotbar HUD.
///
/// When an action is performed:
/// - Tools selected in the hotbar take priority over those in the toolbelt.
/// - If the hotbar item is not a matching tool, a matching tool from the dedicated
///   toolbelt slot is temporarily activated in a dedicated slot to the right of the hotbar.
/// - The player's selected hotbar slot is NOT replaced or mutated.
/// - When the action ends, the temporary slot is hidden.
#[derive(Debug, Clone, Default)]
pub struct AutoSwitchTracker {
    /// The tool item currently being used from the dedicated slot.
    pub active_tool: Option<ItemStack>,
    /// The tool type that was activated.
    pub active_tool_type: Option<ToolType>,
    /// Indicates an action using the toolbelt tool is currently in progress.
    pub in_progress: bool,
}

impl AutoSwitchTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates player action and initiates auto-switching if a matching tool is available.
    ///
    /// Priority rule: Tools selected in the hotbar take priority over toolbelt slots.
    /// If held item already matches the action, returns None (no toolbelt activation).
    /// If held item does not match, activates the tool from the dedicated slot into the
    /// temporary right-hand hotbar slot.
    pub fn on_action_start(
        &mut self,
        action: PlayerAction,
        inventory: &ModInventoryData,
        held_item: &ItemStack,
    ) -> Option<ItemStack> {
        let required = action.required_tool();

        // 1. Hotbar priority: if the held item in the hotbar already matches, use it!
        if held_item.tool_type() == Some(required) {
            self.reset();
            return None;
        }

        // 2. Check the dedicated toolbelt slot
        if let Some(tool) = inventory.get_tool(required) {
            if !tool.is_empty() {
                self.active_tool = Some(tool.clone());
                self.active_tool_type = Some(required);
                self.in_progress = true;
                return Some(tool.clone());
            }
        }

        // 3. Dedicated slot is empty: fallback to held item / hands
        self.reset();
        None
    }

    /// Called when the action terminates (block broken, attack completed, mouse released).
    /// Hides the temporary right-hand tool slot.
    pub fn on_action_end(&mut self) -> bool {
        if self.in_progress {
            self.reset();
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.active_tool = None;
        self.active_tool_type = None;
        self.in_progress = false;
    }

    pub fn is_active(&self) -> bool {
        self.in_progress && self.active_tool.is_some()
    }

    pub fn get_active_tool(&self) -> Option<&ItemStack> {
        if self.in_progress {
            self.active_tool.as_ref()
        } else {
            None
        }
    }
}
