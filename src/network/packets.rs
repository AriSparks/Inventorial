//! Network packets for client-server synchronization in Horizon.

use crate::model::ModInventoryData;

pub const CHANNEL_NAME: &str = "inventorial:main";

#[derive(Debug, Clone)]
pub enum ModPacket {
    /// Client -> Server: Player cycled offhand slots.
    CycleOffhand { delta: i32 },
    /// Client -> Server: Player clicked a custom slot in inventory.
    SlotClick {
        slot_type: SlotTypeTarget,
        slot_index: usize,
        click_type: ClickType,
    },
    /// Server -> Client: Synchronizes full custom inventory state.
    SyncInventory { data: ModInventoryData },
    /// Server -> Client: Synchronizes active offhand index.
    SyncOffhandIndex { new_index: usize },
    /// Client <-> Server: Temporary tool switch confirmation.
    ToolSwitchNotice {
        is_active: bool,
        tool_type: Option<usize>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotTypeTarget {
    Tool,
    Offhand,
    ExtraRow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickType {
    Normal,
    ShiftClick,
    RightClickSplit,
    HotbarSwap(usize),
}
