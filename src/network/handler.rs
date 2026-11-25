//! Network packet encoding, decoding, and dispatching.

use super::packets::{ClickType, ModPacket, SlotTypeTarget};
use crate::model::{ItemStack, ModInventoryData};
use crate::slots::ExtraPocketsManager;
use crate::storage::persistence::{decode_inventory, encode_inventory, read_u32, write_u32};

pub struct PacketHandler;

impl PacketHandler {
    /// Serializes a packet into bytes to transmit over Horizon's custom payload channel.
    pub fn encode(packet: &ModPacket) -> Result<Vec<u8>, &'static str> {
        let mut buf = Vec::new();
        match packet {
            ModPacket::CycleOffhand { delta } => {
                buf.push(0);
                write_u32(&mut buf, *delta as u32);
            }
            ModPacket::SlotClick {
                slot_type,
                slot_index,
                click_type,
            } => {
                buf.push(1);
                buf.push(match slot_type {
                    SlotTypeTarget::Tool => 0,
                    SlotTypeTarget::Offhand => 1,
                    SlotTypeTarget::ExtraRow => 2,
                });
                write_u32(&mut buf, *slot_index as u32);
                match click_type {
                    ClickType::Normal => buf.push(0),
                    ClickType::ShiftClick => buf.push(1),
                    ClickType::RightClickSplit => buf.push(2),
                    ClickType::HotbarSwap(idx) => {
                        buf.push(3);
                        write_u32(&mut buf, *idx as u32);
                    }
                }
            }
            ModPacket::SyncInventory { data } => {
                buf.push(2);
                buf.extend_from_slice(&encode_inventory(data));
            }
            ModPacket::SyncOffhandIndex { new_index } => {
                buf.push(3);
                write_u32(&mut buf, *new_index as u32);
            }
            ModPacket::ToolSwitchNotice {
                is_active,
                tool_type,
            } => {
                buf.push(4);
                buf.push(u8::from(*is_active));
                match tool_type {
                    None => buf.push(0),
                    Some(t) => {
                        buf.push(1);
                        write_u32(&mut buf, *t as u32);
                    }
                }
            }
        }
        Ok(buf)
    }

    /// Deserializes a packet received from Horizon's custom payload channel.
    pub fn decode(payload: &[u8]) -> Result<ModPacket, &'static str> {
        if payload.is_empty() {
            return Err("empty packet");
        }
        let mut pos = 1;
        match payload[0] {
            0 => {
                let delta = read_u32(payload, &mut pos)? as i32;
                Ok(ModPacket::CycleOffhand { delta })
            }
            1 => {
                if pos >= payload.len() {
                    return Err("truncated slot click");
                }
                let slot_type = match payload[pos] {
                    0 => SlotTypeTarget::Tool,
                    1 => SlotTypeTarget::Offhand,
                    2 => SlotTypeTarget::ExtraRow,
                    _ => return Err("invalid slot_type"),
                };
                pos += 1;
                let slot_index = read_u32(payload, &mut pos)? as usize;
                if pos >= payload.len() {
                    return Err("truncated click_type");
                }
                let click_type = match payload[pos] {
                    0 => ClickType::Normal,
                    1 => ClickType::ShiftClick,
                    2 => ClickType::RightClickSplit,
                    3 => {
                        pos += 1;
                        let idx = read_u32(payload, &mut pos)? as usize;
                        ClickType::HotbarSwap(idx)
                    }
                    _ => return Err("invalid click_type"),
                };
                Ok(ModPacket::SlotClick {
                    slot_type,
                    slot_index,
                    click_type,
                })
            }
            2 => {
                let data = decode_inventory(payload, &mut pos)?;
                Ok(ModPacket::SyncInventory { data })
            }
            3 => {
                let new_index = read_u32(payload, &mut pos)? as usize;
                Ok(ModPacket::SyncOffhandIndex { new_index })
            }
            4 => {
                if pos + 1 >= payload.len() {
                    return Err("truncated tool switch notice");
                }
                let is_active = payload[pos] != 0;
                pos += 1;
                let has_tool = payload[pos] != 0;
                pos += 1;
                let tool_type = if has_tool {
                    Some(read_u32(payload, &mut pos)? as usize)
                } else {
                    None
                };
                Ok(ModPacket::ToolSwitchNotice {
                    is_active,
                    tool_type,
                })
            }
            _ => Err("unknown packet tag"),
        }
    }

    /// Server-side packet processor.
    pub fn handle_packet_server(
        packet: ModPacket,
        player_inv: &mut ModInventoryData,
        vanilla_inv: &mut [Option<ItemStack>],
        cursor_item: &mut Option<ItemStack>,
    ) -> Option<ModPacket> {
        match packet {
            ModPacket::CycleOffhand { delta } => {
                let new_idx = player_inv.cycle_offhand(delta);
                Some(ModPacket::SyncOffhandIndex { new_index: new_idx })
            }
            ModPacket::SlotClick {
                slot_type,
                slot_index,
                click_type,
            } => {
                Self::process_slot_click(
                    slot_type,
                    slot_index,
                    click_type,
                    player_inv,
                    vanilla_inv,
                    cursor_item,
                );
                Some(ModPacket::SyncInventory {
                    data: player_inv.clone(),
                })
            }
            ModPacket::ToolSwitchNotice { .. } => None,
            _ => None,
        }
    }

    fn process_slot_click(
        slot_type: SlotTypeTarget,
        slot_index: usize,
        click_type: ClickType,
        custom_inv: &mut ModInventoryData,
        vanilla_inv: &mut [Option<ItemStack>],
        cursor_item: &mut Option<ItemStack>,
    ) {
        let slot_ref = match slot_type {
            SlotTypeTarget::Tool => {
                if slot_index < custom_inv.tool_slots.len() {
                    &mut custom_inv.tool_slots[slot_index]
                } else {
                    return;
                }
            }
            SlotTypeTarget::Offhand => {
                if slot_index < custom_inv.offhand_slots.len() {
                    &mut custom_inv.offhand_slots[slot_index]
                } else {
                    return;
                }
            }
            SlotTypeTarget::ExtraRow => {
                if slot_index < custom_inv.extra_rows.len() {
                    &mut custom_inv.extra_rows[slot_index]
                } else {
                    return;
                }
            }
        };

        match click_type {
            ClickType::Normal => {
                // If targeting tool slot, validate tool type
                if slot_type == SlotTypeTarget::Tool {
                    if let Some(cursor) = cursor_item.as_ref() {
                        if !cursor.is_empty()
                            && !ModInventoryData::can_place_in_tool_slot(slot_index, cursor)
                        {
                            return; // Invalid tool for this slot
                        }
                    }
                }
                std::mem::swap(slot_ref, cursor_item);
            }
            ClickType::ShiftClick => {
                if let Some(stack) = slot_ref.as_mut() {
                    ExtraPocketsManager::shift_click_from_custom(stack, vanilla_inv);
                    if stack.is_empty() {
                        *slot_ref = None;
                    }
                }
            }
            ClickType::RightClickSplit => {
                // Pick up half or drop one item
                if let Some(cursor) = cursor_item.as_mut() {
                    if slot_type == SlotTypeTarget::Tool
                        && !ModInventoryData::can_place_in_tool_slot(slot_index, cursor)
                    {
                        return;
                    }

                    if slot_ref.is_none() {
                        let mut single = cursor.clone();
                        single.count = 1;
                        cursor.count -= 1;
                        *slot_ref = Some(single);
                        if cursor.count == 0 {
                            *cursor_item = None;
                        }
                    } else if let Some(target) = slot_ref.as_mut() {
                        if target.id == cursor.id
                            && target.enchantments == cursor.enchantments
                            && target.count < 64
                        {
                            target.count += 1;
                            cursor.count -= 1;
                            if cursor.count == 0 {
                                *cursor_item = None;
                            }
                        }
                    }
                } else if let Some(target) = slot_ref.as_mut() {
                    let half = (target.count + 1) / 2;
                    let mut picked = target.clone();
                    picked.count = half;
                    target.count -= half;
                    *cursor_item = Some(picked);
                    if target.count == 0 {
                        *slot_ref = None;
                    }
                }
            }
            ClickType::HotbarSwap(hotbar_idx) => {
                if hotbar_idx < 9 && hotbar_idx < vanilla_inv.len() {
                    let hotbar_ref = &mut vanilla_inv[hotbar_idx];
                    if slot_type == SlotTypeTarget::Tool {
                        if let Some(hb_item) = hotbar_ref.as_ref() {
                            if !hb_item.is_empty()
                                && !ModInventoryData::can_place_in_tool_slot(slot_index, hb_item)
                            {
                                return;
                            }
                        }
                    }
                    std::mem::swap(slot_ref, hotbar_ref);
                }
            }
        }
    }
}
