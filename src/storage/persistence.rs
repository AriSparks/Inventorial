//! Persistence and save data management for custom inventory slots (zero external dependencies).

use crate::model::{ItemStack, ModInventoryData};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PlayerSaveData {
    pub schema_version: u32,
    pub player_uuid: String,
    pub inventory: ModInventoryData,
}

impl PlayerSaveData {
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;

    pub fn new(player_uuid: impl Into<String>, inventory: ModInventoryData) -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            player_uuid: player_uuid.into(),
            inventory,
        }
    }
}

pub fn write_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

pub fn read_str(data: &[u8], pos: &mut usize) -> Result<String, &'static str> {
    let len = read_u32(data, pos)? as usize;
    if *pos + len > data.len() {
        return Err("unexpected EOF reading string");
    }
    let s = std::str::from_utf8(&data[*pos..*pos + len])
        .map_err(|_| "invalid UTF-8")?
        .to_string();
    *pos += len;
    Ok(s)
}

pub fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn read_u32(data: &[u8], pos: &mut usize) -> Result<u32, &'static str> {
    if *pos + 4 > data.len() {
        return Err("unexpected EOF reading u32");
    }
    let v = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    Ok(v)
}

fn write_opt_item(buf: &mut Vec<u8>, item: &Option<ItemStack>) {
    match item {
        None => buf.push(0),
        Some(stack) => {
            buf.push(1);
            write_str(buf, &stack.id);
            buf.push(stack.count);
            write_u32(buf, stack.enchantments.len() as u32);
            for (k, v) in &stack.enchantments {
                write_str(buf, k);
                write_u32(buf, *v);
            }
            write_u32(buf, stack.custom_data.len() as u32);
            for (k, v) in &stack.custom_data {
                write_str(buf, k);
                write_str(buf, v);
            }
        }
    }
}

fn read_opt_item(data: &[u8], pos: &mut usize) -> Result<Option<ItemStack>, &'static str> {
    if *pos >= data.len() {
        return Err("unexpected EOF reading item flag");
    }
    let present = data[*pos];
    *pos += 1;
    if present == 0 {
        return Ok(None);
    }
    let id = read_str(data, pos)?;
    if *pos >= data.len() {
        return Err("unexpected EOF reading item count");
    }
    let count = data[*pos];
    *pos += 1;
    let ench_len = read_u32(data, pos)? as usize;
    let mut enchantments = HashMap::with_capacity(ench_len);
    for _ in 0..ench_len {
        let k = read_str(data, pos)?;
        let v = read_u32(data, pos)?;
        enchantments.insert(k, v);
    }
    let custom_len = read_u32(data, pos)? as usize;
    let mut custom_data = HashMap::with_capacity(custom_len);
    for _ in 0..custom_len {
        let k = read_str(data, pos)?;
        let v = read_str(data, pos)?;
        custom_data.insert(k, v);
    }
    Ok(Some(ItemStack {
        id,
        count,
        enchantments,
        custom_data,
    }))
}

pub fn encode_inventory(inv: &ModInventoryData) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"INV1");
    for slot in &inv.tool_slots {
        write_opt_item(&mut buf, slot);
    }
    write_u32(&mut buf, inv.offhand_slots.len() as u32);
    for slot in &inv.offhand_slots {
        write_opt_item(&mut buf, slot);
    }
    write_u32(&mut buf, inv.active_offhand_index as u32);
    write_u32(&mut buf, inv.extra_rows.len() as u32);
    for slot in &inv.extra_rows {
        write_opt_item(&mut buf, slot);
    }
    write_u32(&mut buf, inv.ender_chest.len() as u32);
    for slot in &inv.ender_chest {
        write_opt_item(&mut buf, slot);
    }
    buf
}

pub fn decode_inventory(data: &[u8], pos: &mut usize) -> Result<ModInventoryData, &'static str> {
    if *pos + 4 > data.len() || &data[*pos..*pos + 4] != b"INV1" {
        return Err("invalid inventory magic header");
    }
    *pos += 4;
    let mut tool_slots: [Option<ItemStack>; 5] = [None, None, None, None, None];
    for slot in &mut tool_slots {
        *slot = read_opt_item(data, pos)?;
    }
    let offhand_len = read_u32(data, pos)? as usize;
    let mut offhand_slots = Vec::with_capacity(offhand_len);
    for _ in 0..offhand_len {
        offhand_slots.push(read_opt_item(data, pos)?);
    }
    let active_offhand_index = read_u32(data, pos)? as usize;
    let extra_len = read_u32(data, pos)? as usize;
    let mut extra_rows = Vec::with_capacity(extra_len);
    for _ in 0..extra_len {
        extra_rows.push(read_opt_item(data, pos)?);
    }
    let ender_len = read_u32(data, pos)? as usize;
    let mut ender_chest = Vec::with_capacity(ender_len);
    for _ in 0..ender_len {
        ender_chest.push(read_opt_item(data, pos)?);
    }
    let active_offhand_index = active_offhand_index.min(offhand_slots.len().saturating_sub(1));
    Ok(ModInventoryData {
        tool_slots,
        offhand_slots,
        active_offhand_index,
        extra_rows,
        ender_chest,
    })
}

pub struct SaveManager {
    save_directory: PathBuf,
}

impl SaveManager {
    pub fn new(world_save_dir: impl AsRef<Path>) -> Self {
        let dir = world_save_dir.as_ref().join("inventorial_data");
        Self { save_directory: dir }
    }

    /// Saves the player's custom inventory data to disk.
    pub fn save_player_data(
        &self,
        player_uuid: &str,
        data: &ModInventoryData,
    ) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.save_directory)?;
        let file_path = self.save_directory.join(format!("{}.bin", player_uuid));
        let mut bytes = Vec::new();
        write_u32(&mut bytes, PlayerSaveData::CURRENT_SCHEMA_VERSION);
        write_str(&mut bytes, player_uuid);
        bytes.extend_from_slice(&encode_inventory(data));

        // Write via temp file to prevent partial write corruption
        let temp_path = self.save_directory.join(format!("{}.tmp", player_uuid));
        fs::write(&temp_path, bytes)?;
        fs::rename(temp_path, file_path)?;
        Ok(())
    }

    /// Loads the player's custom inventory data from disk.
    /// Returns default inventory if no save data exists (e.g., first run or save created without mod).
    pub fn load_player_data(&self, player_uuid: &str) -> ModInventoryData {
        let file_path = self.save_directory.join(format!("{}.bin", player_uuid));
        if !file_path.exists() {
            return ModInventoryData::default();
        }

        match fs::read(&file_path) {
            Ok(bytes) => {
                let mut pos = 0;
                let parsed = (|| -> Result<ModInventoryData, &'static str> {
                    let _schema = read_u32(&bytes, &mut pos)?;
                    let _uuid = read_str(&bytes, &mut pos)?;
                    decode_inventory(&bytes, &mut pos)
                })();
                match parsed {
                    Ok(inv) => inv,
                    Err(err) => {
                        eprintln!(
                            "[Inventorial] Warning: failed to parse save data for player {}: {}. Using default.",
                            player_uuid, err
                        );
                        ModInventoryData::default()
                    }
                }
            }
            Err(err) => {
                eprintln!(
                    "[Inventorial] Warning: failed to read save data file {}: {}. Using default.",
                    file_path.display(),
                    err
                );
                ModInventoryData::default()
            }
        }
    }
}

/// Encodes a `mod_api::Tag` into a byte buffer.
pub fn encode_tag(buf: &mut Vec<u8>, tag: &mod_api::Tag) {
    use mod_api::Tag;
    match tag {
        Tag::End => buf.push(0),
        Tag::Byte(v) => {
            buf.push(1);
            buf.push(*v as u8);
        }
        Tag::Short(v) => {
            buf.push(2);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Tag::Int(v) => {
            buf.push(3);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Tag::Long(v) => {
            buf.push(4);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Tag::Float(v) => {
            buf.push(5);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Tag::Double(v) => {
            buf.push(6);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Tag::ByteArray(arr) => {
            buf.push(7);
            write_u32(buf, arr.len() as u32);
            buf.extend_from_slice(arr);
        }
        Tag::String(s) => {
            buf.push(8);
            let lossy = s.to_string_lossy();
            write_str(buf, &lossy);
        }
        Tag::List(list) => {
            buf.push(9);
            write_u32(buf, list.len() as u32);
            for item in list {
                encode_tag(buf, item);
            }
        }
        Tag::Compound(map) => {
            buf.push(10);
            encode_compound(buf, map);
        }
        Tag::IntArray(arr) => {
            buf.push(11);
            write_u32(buf, arr.len() as u32);
            for &v in arr {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        Tag::LongArray(arr) => {
            buf.push(12);
            write_u32(buf, arr.len() as u32);
            for &v in arr {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
}

/// Decodes a `mod_api::Tag` from a byte buffer.
pub fn decode_tag(data: &[u8], pos: &mut usize) -> Result<mod_api::Tag, &'static str> {
    use mod_api::Tag;
    if *pos >= data.len() {
        return Err("truncated tag id");
    }
    let id = data[*pos];
    *pos += 1;
    match id {
        0 => Ok(Tag::End),
        1 => {
            if *pos >= data.len() {
                return Err("truncated byte tag");
            }
            let v = data[*pos] as i8;
            *pos += 1;
            Ok(Tag::Byte(v))
        }
        2 => {
            if *pos + 2 > data.len() {
                return Err("truncated short tag");
            }
            let v = i16::from_le_bytes([data[*pos], data[*pos + 1]]);
            *pos += 2;
            Ok(Tag::Short(v))
        }
        3 => {
            let v = read_u32(data, pos)? as i32;
            Ok(Tag::Int(v))
        }
        4 => {
            if *pos + 8 > data.len() {
                return Err("truncated long tag");
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&data[*pos..*pos + 8]);
            *pos += 8;
            Ok(Tag::Long(i64::from_le_bytes(b)))
        }
        5 => {
            let v = read_u32(data, pos)?;
            Ok(Tag::Float(v))
        }
        6 => {
            if *pos + 8 > data.len() {
                return Err("truncated double tag");
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&data[*pos..*pos + 8]);
            *pos += 8;
            Ok(Tag::Double(u64::from_le_bytes(b)))
        }
        7 => {
            let len = read_u32(data, pos)? as usize;
            if *pos + len > data.len() {
                return Err("truncated byte array tag");
            }
            let arr = data[*pos..*pos + len].to_vec();
            *pos += len;
            Ok(Tag::ByteArray(arr))
        }
        8 => {
            let s = read_str(data, pos)?;
            Ok(Tag::String(s.as_str().into()))
        }
        9 => {
            let len = read_u32(data, pos)? as usize;
            let mut list = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                list.push(decode_tag(data, pos)?);
            }
            Ok(Tag::List(list))
        }
        10 => Ok(Tag::Compound(decode_compound(data, pos)?)),
        11 => {
            let len = read_u32(data, pos)? as usize;
            let mut arr = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                arr.push(read_u32(data, pos)? as i32);
            }
            Ok(Tag::IntArray(arr))
        }
        12 => {
            let len = read_u32(data, pos)? as usize;
            let mut arr = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                if *pos + 8 > data.len() {
                    return Err("truncated long array tag");
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&data[*pos..*pos + 8]);
                *pos += 8;
                arr.push(i64::from_le_bytes(b));
            }
            Ok(Tag::LongArray(arr))
        }
        _ => Err("invalid tag id"),
    }
}

pub fn encode_compound(buf: &mut Vec<u8>, map: &mod_api::Compound) {
    write_u32(buf, map.len() as u32);
    for (k, v) in map.iter() {
        let key_str = k.to_string_lossy();
        write_str(buf, &key_str);
        encode_tag(buf, v);
    }
}

pub fn decode_compound(data: &[u8], pos: &mut usize) -> Result<mod_api::Compound, &'static str> {
    let len = read_u32(data, pos)? as usize;
    let mut map = mod_api::Compound::new();
    for _ in 0..len {
        let k = read_str(data, pos)?;
        let v = decode_tag(data, pos)?;
        map.insert(k.as_str().into(), v);
    }
    Ok(map)
}

/// Encodes an optional `mod_api::ItemStack` losslessly (id, count, damage, components).
pub fn encode_api_stack(
    host: &dyn mod_api::Host,
    stack: &Option<mod_api::ItemStack>,
    buf: &mut Vec<u8>,
) {
    match stack {
        None => buf.push(0),
        Some(s) => {
            buf.push(1);
            write_str(buf, &s.item().id(host));
            write_u32(buf, s.count());
            write_u32(buf, s.damage());
            match s.components() {
                None => buf.push(0),
                Some(comps) => {
                    buf.push(1);
                    encode_compound(buf, comps.as_nbt());
                }
            }
        }
    }
}

/// Decodes an optional `mod_api::ItemStack` using `host` to resolve the item key.
pub fn decode_api_stack(
    host: &dyn mod_api::Host,
    data: &[u8],
    pos: &mut usize,
) -> Result<Option<mod_api::ItemStack>, &'static str> {
    if *pos >= data.len() {
        return Err("truncated api stack flag");
    }
    let flag = data[*pos];
    *pos += 1;
    if flag == 0 {
        return Ok(None);
    }
    let id_str = read_str(data, pos)?;
    let count = read_u32(data, pos)?;
    let damage = read_u32(data, pos)?;
    if *pos >= data.len() {
        return Err("truncated api stack components flag");
    }
    let comp_flag = data[*pos];
    *pos += 1;
    let comp_nbt = if comp_flag != 0 {
        Some(decode_compound(data, pos)?)
    } else {
        None
    };

    let Ok(ident) = mod_api::Identifier::parse(&id_str) else {
        return Ok(None);
    };
    let Some(key) = host.item_key(&ident) else {
        return Ok(None);
    };
    let Some(mut stack) = mod_api::ItemStack::new(key, count) else {
        return Ok(None);
    };
    if damage > 0 {
        stack.set_damage(damage);
    }
    if let Some(nbt) = comp_nbt {
        let _ = stack.set_components(nbt);
    }
    Ok(Some(stack))
}
