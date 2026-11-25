//! Authoritative server-side & client-side unified inventory & ender chest screen controller (Path A).

use crate::model::{ItemStack as ModelItemStack, ToolType};
use crate::storage::persistence::{
    decode_api_stack, encode_api_stack, read_str, read_u32, write_str, write_u32,
};
use crate::ui::inventory_screen::{
    compute_ender_chest_layout, compute_unified_layout, UnifiedLayout, UnifiedSlotId,
};
use mod_api::{
    ClientHost, DrawContext, GuiEntity, GuiEntityLook, GuiEntityView, Host, IPayloadContext,
    IPayloadHandler, Identifier, ItemStack as ApiItemStack, PayloadBytes, PayloadHandleError,
    PayloadId, PlayerRef, RecipeInput, RecipeMatch, ServerHost, Tag, Text, TooltipElement,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

pub const CHANNEL_SCREEN_ACTION: &str = "inventorial:screen_action";
pub const CHANNEL_SCREEN_SYNC: &str = "inventorial:screen_sync";
pub const MOD_SCREEN_ID: &str = "inventorial";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenMode {
    #[default]
    Inventory,
    EnderChest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenAction {
    Open,
    Close,
    LeftClick { slot: UnifiedSlotId, shift: bool },
    RightClick { slot: UnifiedSlotId },
    NumberKey { slot: UnifiedSlotId, hotbar: u8 },
    Drop { slot: Option<UnifiedSlotId>, whole_stack: bool },
    CycleOffhand { delta: i32 },
    OpenEnderChest,
    FillRecipe {
        grid_items: Vec<Option<String>>,
        shift: bool,
    },
    DoubleClick {
        slot: UnifiedSlotId,
    },
    DragDistribute {
        button: u8,
        slots: Vec<UnifiedSlotId>,
    },
    ShiftDoubleClick {
        slot: UnifiedSlotId,
    },
    FireworkBoost,
    ContainerQuickMove {
        slot: UnifiedSlotId,
    },
    CreativePlace {
        slot: UnifiedSlotId,
        stack_bytes: Vec<u8>,
    },
    MoveMainToSlot {
        main_slot: u8,
        target: UnifiedSlotId,
    },
    MoveSlotToMain {
        src: UnifiedSlotId,
        main_slot: u8,
        half: bool,
    },
}

fn encode_slot_id(buf: &mut Vec<u8>, slot: UnifiedSlotId) {
    match slot {
        UnifiedSlotId::Main(i) => {
            buf.push(0);
            buf.push(i);
        }
        UnifiedSlotId::Armor(i) => {
            buf.push(1);
            buf.push(i);
        }
        UnifiedSlotId::Offhand(i) => {
            buf.push(2);
            buf.push(i);
        }
        UnifiedSlotId::Toolbelt(i) => {
            buf.push(3);
            buf.push(i);
        }
        UnifiedSlotId::Extra(i) => {
            buf.push(4);
            buf.push(i);
        }
        UnifiedSlotId::CraftInput(i) => {
            buf.push(5);
            buf.push(i);
        }
        UnifiedSlotId::CraftOutput => {
            buf.push(6);
            buf.push(0);
        }
        UnifiedSlotId::EnderChest(i) => {
            buf.push(7);
            buf.push(i);
        }
        UnifiedSlotId::Elytra => {
            buf.push(8);
            buf.push(0);
        }
    }
}

fn decode_slot_id(data: &[u8], pos: &mut usize) -> Result<UnifiedSlotId, &'static str> {
    if *pos + 2 > data.len() {
        return Err("truncated slot id");
    }
    let kind = data[*pos];
    let idx = data[*pos + 1];
    *pos += 2;
    match kind {
        0 => Ok(UnifiedSlotId::Main(idx)),
        1 => Ok(UnifiedSlotId::Armor(idx)),
        2 => Ok(UnifiedSlotId::Offhand(idx)),
        3 => Ok(UnifiedSlotId::Toolbelt(idx)),
        4 => Ok(UnifiedSlotId::Extra(idx)),
        5 => Ok(UnifiedSlotId::CraftInput(idx)),
        6 => Ok(UnifiedSlotId::CraftOutput),
        7 => Ok(UnifiedSlotId::EnderChest(idx)),
        8 => Ok(UnifiedSlotId::Elytra),
        _ => Err("invalid slot id kind"),
    }
}

impl ScreenAction {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            ScreenAction::Open => buf.push(0),
            ScreenAction::Close => buf.push(1),
            ScreenAction::LeftClick { slot, shift } => {
                buf.push(2);
                encode_slot_id(&mut buf, *slot);
                buf.push(u8::from(*shift));
            }
            ScreenAction::RightClick { slot } => {
                buf.push(3);
                encode_slot_id(&mut buf, *slot);
            }
            ScreenAction::NumberKey { slot, hotbar } => {
                buf.push(4);
                encode_slot_id(&mut buf, *slot);
                buf.push(*hotbar);
            }
            ScreenAction::Drop { slot, whole_stack } => {
                buf.push(5);
                match slot {
                    None => buf.push(0),
                    Some(s) => {
                        buf.push(1);
                        encode_slot_id(&mut buf, *s);
                    }
                }
                buf.push(u8::from(*whole_stack));
            }
            ScreenAction::CycleOffhand { delta } => {
                buf.push(6);
                write_u32(&mut buf, *delta as u32);
            }
            ScreenAction::OpenEnderChest => buf.push(7),
            ScreenAction::FillRecipe { grid_items, shift } => {
                buf.push(8);
                buf.push(u8::from(*shift));
                write_u32(&mut buf, grid_items.len() as u32);
                for item in grid_items {
                    match item {
                        None => buf.push(0),
                        Some(id) => {
                            buf.push(1);
                            write_str(&mut buf, id);
                        }
                    }
                }
            }
            ScreenAction::DoubleClick { slot } => {
                buf.push(9);
                encode_slot_id(&mut buf, *slot);
            }
            ScreenAction::DragDistribute { button, slots } => {
                buf.push(10);
                buf.push(*button);
                write_u32(&mut buf, slots.len() as u32);
                for &s in slots {
                    encode_slot_id(&mut buf, s);
                }
            }
            ScreenAction::ShiftDoubleClick { slot } => {
                buf.push(11);
                encode_slot_id(&mut buf, *slot);
            }
            ScreenAction::FireworkBoost => {
                buf.push(12);
            }
            ScreenAction::ContainerQuickMove { slot } => {
                buf.push(13);
                encode_slot_id(&mut buf, *slot);
            }
            ScreenAction::CreativePlace { slot, stack_bytes } => {
                buf.push(14);
                encode_slot_id(&mut buf, *slot);
                write_u32(&mut buf, stack_bytes.len() as u32);
                buf.extend_from_slice(stack_bytes);
            }
            ScreenAction::MoveMainToSlot { main_slot, target } => {
                buf.push(15);
                buf.push(*main_slot);
                encode_slot_id(&mut buf, *target);
            }
            ScreenAction::MoveSlotToMain {
                src,
                main_slot,
                half,
            } => {
                buf.push(16);
                encode_slot_id(&mut buf, *src);
                buf.push(*main_slot);
                buf.push(u8::from(*half));
            }
        }
        buf
    }

    pub fn decode(data: &[u8]) -> Result<Self, &'static str> {
        if data.is_empty() {
            return Err("empty screen action");
        }
        let mut pos = 1;
        match data[0] {
            0 => Ok(ScreenAction::Open),
            1 => Ok(ScreenAction::Close),
            2 => {
                let slot = decode_slot_id(data, &mut pos)?;
                if pos >= data.len() {
                    return Err("truncated shift flag");
                }
                let shift = data[pos] != 0;
                Ok(ScreenAction::LeftClick { slot, shift })
            }
            3 => {
                let slot = decode_slot_id(data, &mut pos)?;
                Ok(ScreenAction::RightClick { slot })
            }
            4 => {
                let slot = decode_slot_id(data, &mut pos)?;
                if pos >= data.len() {
                    return Err("truncated hotbar index");
                }
                Ok(ScreenAction::NumberKey {
                    slot,
                    hotbar: data[pos],
                })
            }
            5 => {
                if pos >= data.len() {
                    return Err("truncated drop slot flag");
                }
                let slot = if data[pos] == 0 {
                    pos += 1;
                    None
                } else {
                    pos += 1;
                    Some(decode_slot_id(data, &mut pos)?)
                };
                if pos >= data.len() {
                    return Err("truncated whole_stack flag");
                }
                Ok(ScreenAction::Drop {
                    slot,
                    whole_stack: data[pos] != 0,
                })
            }
            6 => {
                let delta = read_u32(data, &mut pos)? as i32;
                Ok(ScreenAction::CycleOffhand { delta })
            }
            7 => Ok(ScreenAction::OpenEnderChest),
            8 => {
                if pos >= data.len() {
                    return Err("truncated fill recipe shift");
                }
                let shift = data[pos] != 0;
                pos += 1;
                let len = read_u32(data, &mut pos)? as usize;
                let mut grid_items = Vec::with_capacity(len.min(4));
                for _ in 0..len {
                    if pos >= data.len() {
                        return Err("truncated recipe item flag");
                    }
                    let flag = data[pos];
                    pos += 1;
                    if flag == 0 {
                        grid_items.push(None);
                    } else {
                        grid_items.push(Some(read_str(data, &mut pos)?));
                    }
                }
                Ok(ScreenAction::FillRecipe { grid_items, shift })
            }
            9 => {
                let slot = decode_slot_id(data, &mut pos)?;
                Ok(ScreenAction::DoubleClick { slot })
            }
            10 => {
                if pos >= data.len() {
                    return Err("truncated drag button");
                }
                let button = data[pos];
                pos += 1;
                let len = read_u32(data, &mut pos)? as usize;
                let mut slots = Vec::with_capacity(len.min(128));
                for _ in 0..len {
                    slots.push(decode_slot_id(data, &mut pos)?);
                }
                Ok(ScreenAction::DragDistribute { button, slots })
            }
            11 => {
                let slot = decode_slot_id(data, &mut pos)?;
                Ok(ScreenAction::ShiftDoubleClick { slot })
            }
            12 => Ok(ScreenAction::FireworkBoost),
            13 => {
                let slot = decode_slot_id(data, &mut pos)?;
                Ok(ScreenAction::ContainerQuickMove { slot })
            }
            14 => {
                let slot = decode_slot_id(data, &mut pos)?;
                let len = read_u32(data, &mut pos)? as usize;
                if pos + len > data.len() {
                    return Err("truncated creative place stack");
                }
                let stack_bytes = data[pos..pos + len].to_vec();
                Ok(ScreenAction::CreativePlace { slot, stack_bytes })
            }
            15 => {
                if pos >= data.len() {
                    return Err("truncated main_slot");
                }
                let main_slot = data[pos];
                pos += 1;
                let target = decode_slot_id(data, &mut pos)?;
                Ok(ScreenAction::MoveMainToSlot { main_slot, target })
            }
            16 => {
                let src = decode_slot_id(data, &mut pos)?;
                if pos + 2 > data.len() {
                    return Err("truncated move_slot_to_main");
                }
                let main_slot = data[pos];
                let half = data[pos + 1] != 0;
                Ok(ScreenAction::MoveSlotToMain {
                    src,
                    main_slot,
                    half,
                })
            }
            _ => Err("unknown screen action"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FullScreenSnapshot {
    pub is_creative: bool,
    pub mode: ScreenMode,
    pub open_ender_request: bool,
    pub selected_slot: usize,
    pub active_offhand: usize,
    pub offhand_count: usize,
    pub extra_rows: usize,
    pub ender_rows: usize,
    pub main: Vec<Option<ApiItemStack>>,
    pub armor: [Option<ApiItemStack>; 4],
    pub offhand: Vec<Option<ApiItemStack>>,
    pub toolbelt: [Option<ApiItemStack>; 5],
    pub extra: Vec<Option<ApiItemStack>>,
    pub craft_in: [Option<ApiItemStack>; 4],
    pub craft_out: Option<ApiItemStack>,
    pub ender_chest: Vec<Option<ApiItemStack>>,
    pub cursor: Option<ApiItemStack>,
    pub elytra: Option<ApiItemStack>,
    pub firework_boost_ticks: u32,
}

impl FullScreenSnapshot {
    pub fn encode(&self, host: &dyn Host) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(u8::from(self.is_creative));
        buf.push(match self.mode {
            ScreenMode::Inventory => 0,
            ScreenMode::EnderChest => 1,
        });
        buf.push(u8::from(self.open_ender_request));
        write_u32(&mut buf, self.selected_slot as u32);
        write_u32(&mut buf, self.active_offhand as u32);
        write_u32(&mut buf, self.offhand_count as u32);
        write_u32(&mut buf, self.extra_rows as u32);
        write_u32(&mut buf, self.ender_rows as u32);
        write_u32(&mut buf, self.main.len() as u32);
        for s in &self.main {
            encode_api_stack(host, s, &mut buf);
        }
        for s in &self.armor {
            encode_api_stack(host, s, &mut buf);
        }
        write_u32(&mut buf, self.offhand.len() as u32);
        for s in &self.offhand {
            encode_api_stack(host, s, &mut buf);
        }
        for s in &self.toolbelt {
            encode_api_stack(host, s, &mut buf);
        }
        write_u32(&mut buf, self.extra.len() as u32);
        for s in &self.extra {
            encode_api_stack(host, s, &mut buf);
        }
        for s in &self.craft_in {
            encode_api_stack(host, s, &mut buf);
        }
        encode_api_stack(host, &self.craft_out, &mut buf);
        write_u32(&mut buf, self.ender_chest.len() as u32);
        for s in &self.ender_chest {
            encode_api_stack(host, s, &mut buf);
        }
        encode_api_stack(host, &self.cursor, &mut buf);
        encode_api_stack(host, &self.elytra, &mut buf);
        write_u32(&mut buf, self.firework_boost_ticks);
        buf
    }

    pub fn decode(host: &dyn Host, data: &[u8]) -> Result<Self, &'static str> {
        let mut pos = 0;
        if pos + 3 > data.len() {
            return Err("empty snapshot");
        }
        let is_creative = data[pos] != 0;
        let mode = if data[pos + 1] == 1 {
            ScreenMode::EnderChest
        } else {
            ScreenMode::Inventory
        };
        let open_ender_request = data[pos + 2] != 0;
        pos += 3;
        let selected_slot = read_u32(data, &mut pos)? as usize;
        let active_offhand = read_u32(data, &mut pos)? as usize;
        let offhand_count = read_u32(data, &mut pos)? as usize;
        let extra_rows = read_u32(data, &mut pos)? as usize;
        let ender_rows = read_u32(data, &mut pos)? as usize;
        let main_len = read_u32(data, &mut pos)? as usize;
        let mut main = Vec::with_capacity(main_len.min(64));
        for _ in 0..main_len {
            main.push(decode_api_stack(host, data, &mut pos)?);
        }
        let mut armor: [Option<ApiItemStack>; 4] = Default::default();
        for slot in &mut armor {
            *slot = decode_api_stack(host, data, &mut pos)?;
        }
        let offhand_len = read_u32(data, &mut pos)? as usize;
        let mut offhand = Vec::with_capacity(offhand_len.min(32));
        for _ in 0..offhand_len {
            offhand.push(decode_api_stack(host, data, &mut pos)?);
        }
        let mut toolbelt: [Option<ApiItemStack>; 5] = Default::default();
        for slot in &mut toolbelt {
            *slot = decode_api_stack(host, data, &mut pos)?;
        }
        let extra_len = read_u32(data, &mut pos)? as usize;
        let mut extra = Vec::with_capacity(extra_len.min(64));
        for _ in 0..extra_len {
            extra.push(decode_api_stack(host, data, &mut pos)?);
        }
        let mut craft_in: [Option<ApiItemStack>; 4] = Default::default();
        for slot in &mut craft_in {
            *slot = decode_api_stack(host, data, &mut pos)?;
        }
        let craft_out = decode_api_stack(host, data, &mut pos)?;
        let ender_len = read_u32(data, &mut pos)? as usize;
        let mut ender_chest = Vec::with_capacity(ender_len.min(81));
        for _ in 0..ender_len {
            ender_chest.push(decode_api_stack(host, data, &mut pos)?);
        }
        let cursor = decode_api_stack(host, data, &mut pos)?;
        let elytra = decode_api_stack(host, data, &mut pos)?;
        let firework_boost_ticks = read_u32(data, &mut pos)?;
        Ok(Self {
            is_creative,
            mode,
            open_ender_request,
            selected_slot,
            active_offhand,
            offhand_count,
            extra_rows,
            ender_rows,
            main,
            armor,
            offhand,
            toolbelt,
            extra,
            craft_in,
            craft_out,
            ender_chest,
            cursor,
            elytra,
            firework_boost_ticks,
        })
    }

    pub fn get_slot(&self, id: UnifiedSlotId) -> Option<&ApiItemStack> {
        match id {
            UnifiedSlotId::Main(i) => self.main.get(i as usize).and_then(Option::as_ref),
            UnifiedSlotId::Armor(i) => self.armor.get(i as usize).and_then(Option::as_ref),
            UnifiedSlotId::Offhand(i) => self.offhand.get(i as usize).and_then(Option::as_ref),
            UnifiedSlotId::Toolbelt(i) => self.toolbelt.get(i as usize).and_then(Option::as_ref),
            UnifiedSlotId::Extra(i) => self.extra.get(i as usize).and_then(Option::as_ref),
            UnifiedSlotId::CraftInput(i) => self.craft_in.get(i as usize).and_then(Option::as_ref),
            UnifiedSlotId::CraftOutput => self.craft_out.as_ref(),
            UnifiedSlotId::EnderChest(i) => self.ender_chest.get(i as usize).and_then(Option::as_ref),
            UnifiedSlotId::Elytra => self.elytra.as_ref(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServerPlayerSlots {
    pub mode: ScreenMode,
    pub toolbelt: [Option<ApiItemStack>; 5],
    pub offhand: Vec<Option<ApiItemStack>>,
    pub extra: Vec<Option<ApiItemStack>>,
    pub ender_chest: Vec<Option<ApiItemStack>>,
    pub craft_in: [Option<ApiItemStack>; 4],
    pub cursor: Option<ApiItemStack>,
    pub elytra: Option<ApiItemStack>,
}

static SERVER_SLOTS: OnceLock<Mutex<HashMap<String, ServerPlayerSlots>>> = OnceLock::new();
static CLIENT_SNAPSHOT: OnceLock<Mutex<FullScreenSnapshot>> = OnceLock::new();
static OPEN_ENDER_CLIENT: AtomicBool = AtomicBool::new(false);
static ELYTRA_POPUP_OPEN: AtomicBool = AtomicBool::new(false);
static RECIPE_BOOK_OPEN: AtomicBool = AtomicBool::new(false);
static RECIPE_BOOK_PAGE: AtomicUsize = AtomicUsize::new(0);
static RECIPE_BOOK_TAB: AtomicUsize = AtomicUsize::new(0);
static RECIPE_BOOK_CRAFTABLE_ONLY: AtomicBool = AtomicBool::new(false);
static RECIPE_BOOK_SEARCH_FOCUSED: AtomicBool = AtomicBool::new(false);
static RECIPE_BOOK_SEARCH: OnceLock<Mutex<String>> = OnceLock::new();
static UNLOCKED_RECIPES: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
static UNLOCKED_POPULATED: AtomicBool = AtomicBool::new(false);

pub fn reset_unlocked_recipes() {
    UNLOCKED_POPULATED.store(false, Ordering::Relaxed);
}

pub fn refresh_unlocked_recipes(host: &mut dyn ClientHost) {
    if UNLOCKED_POPULATED.load(Ordering::Relaxed) {
        return;
    }
    let known: std::collections::HashSet<i32> = host.known_recipes().iter().copied().collect();
    let recipes = host.recipes_of_type("minecraft:crafting");
    let mut unlocked = std::collections::HashSet::new();
    for r in recipes.iter() {
        let ids = host.recipe_book_ids(&r.id);
        if ids.iter().any(|id| known.contains(id)) {
            unlocked.insert(r.id.clone());
        }
    }
    *UNLOCKED_RECIPES
        .get_or_init(|| Mutex::new(std::collections::HashSet::new()))
        .lock()
        .unwrap() = unlocked;
    UNLOCKED_POPULATED.store(true, Ordering::Relaxed);
}

#[derive(Debug, Clone)]
pub struct DragState {
    pub button: u8,
    pub slots: Vec<UnifiedSlotId>,
}

static DRAG_STATE: OnceLock<Mutex<Option<DragState>>> = OnceLock::new();

pub fn drag_state_lock() -> &'static Mutex<Option<DragState>> {
    DRAG_STATE.get_or_init(|| Mutex::new(None))
}

fn recipe_book_search_lock() -> &'static Mutex<String> {
    RECIPE_BOOK_SEARCH.get_or_init(|| Mutex::new(String::new()))
}

pub fn is_recipe_search_focused() -> bool {
    RECIPE_BOOK_OPEN.load(Ordering::Relaxed) && RECIPE_BOOK_SEARCH_FOCUSED.load(Ordering::Relaxed)
}

pub fn on_recipe_book_char(ch: char) {
    if !is_recipe_search_focused() || ch.is_control() {
        return;
    }
    let mut s = recipe_book_search_lock().lock().unwrap();
    if s.len() < 32 {
        s.push(ch);
        RECIPE_BOOK_PAGE.store(0, Ordering::Relaxed);
    }
}

pub fn on_recipe_book_backspace() {
    if !is_recipe_search_focused() {
        return;
    }
    let mut s = recipe_book_search_lock().lock().unwrap();
    s.pop();
    RECIPE_BOOK_PAGE.store(0, Ordering::Relaxed);
}

pub fn take_open_ender_client() -> bool {
    OPEN_ENDER_CLIENT.swap(false, Ordering::Relaxed)
}

pub fn toggle_recipe_book() {
    let cur = RECIPE_BOOK_OPEN.load(Ordering::Relaxed);
    RECIPE_BOOK_OPEN.store(!cur, Ordering::Relaxed);
}

pub fn is_recipe_book_open() -> bool {
    RECIPE_BOOK_OPEN.load(Ordering::Relaxed)
}

pub fn server_slots_map() -> &'static Mutex<HashMap<String, ServerPlayerSlots>> {
    SERVER_SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn client_snapshot() -> &'static Mutex<FullScreenSnapshot> {
    CLIENT_SNAPSHOT.get_or_init(|| Mutex::new(FullScreenSnapshot::default()))
}

fn api_to_model(host: &dyn Host, api: &ApiItemStack) -> ModelItemStack {
    let id = api.item().id(host);
    let count = api.count().min(u8::MAX as u32) as u8;
    let mut stack = ModelItemStack::new(id, count);
    if let Some(comps) = api.components() {
        let nbt = comps.as_nbt();
        let ench_tag = nbt
            .get(&"minecraft:enchantments".into())
            .or_else(|| nbt.get(&"enchantments".into()));
        if let Some(Tag::Compound(map)) = ench_tag {
            for (k, v) in map.iter() {
                if let Tag::Int(lvl) = v {
                    if *lvl > 0 {
                        if let Ok(key_str) = k.to_string() {
                            stack.enchantments.insert(key_str, *lvl as u32);
                        }
                    }
                }
            }
        }
    }
    stack
}

pub fn sync_server_slots_to_model(host: &dyn Host, uuid: &str, s: &ServerPlayerSlots) {
    let mut state = crate::get_state().lock().unwrap();
    let inv = state.get_or_create_inventory(uuid);
    for i in 0..5 {
        inv.tool_slots[i] = s.toolbelt[i].as_ref().map(|a| api_to_model(host, a));
    }
    if inv.offhand_slots.len() != s.offhand.len() {
        inv.offhand_slots.resize(s.offhand.len(), None);
    }
    for (i, slot) in s.offhand.iter().enumerate() {
        inv.offhand_slots[i] = slot.as_ref().map(|a| api_to_model(host, a));
    }
    if inv.extra_rows.len() != s.extra.len() {
        inv.extra_rows.resize(s.extra.len(), None);
    }
    for (i, slot) in s.extra.iter().enumerate() {
        inv.extra_rows[i] = slot.as_ref().map(|a| api_to_model(host, a));
    }
    if inv.ender_chest.len() != s.ender_chest.len() {
        inv.ender_chest.resize(s.ender_chest.len(), None);
    }
    for (i, slot) in s.ender_chest.iter().enumerate() {
        inv.ender_chest[i] = slot.as_ref().map(|a| api_to_model(host, a));
    }
    let snapshot = inv.clone();
    state.inventories.insert("local_player".to_string(), snapshot);
}

pub fn load_server_slots_from_model(host: &dyn Host, uuid: &str) {
    let inv_opt = {
        let state = crate::get_state().lock().unwrap();
        state.inventories.get(uuid).cloned()
    };
    let Some(inv) = inv_opt else {
        return;
    };
    let mut map = server_slots_map().lock().unwrap();
    let entry = map.entry(uuid.to_string()).or_default();
    for i in 0..5 {
        entry.toolbelt[i] = inv.tool_slots[i]
            .as_ref()
            .and_then(|st| crate::to_api_stack(host, st));
    }
    entry.offhand = inv
        .offhand_slots
        .iter()
        .map(|opt| opt.as_ref().and_then(|st| crate::to_api_stack(host, st)))
        .collect();
    entry.extra = inv
        .extra_rows
        .iter()
        .map(|opt| opt.as_ref().and_then(|st| crate::to_api_stack(host, st)))
        .collect();
    entry.ender_chest = inv
        .ender_chest
        .iter()
        .map(|opt| opt.as_ref().and_then(|st| crate::to_api_stack(host, st)))
        .collect();
}

fn can_accept_slot(host: &dyn Host, slot: UnifiedSlotId, stack: &ApiItemStack) -> bool {
    let id = stack.item().id(host);
    let bare = id.strip_prefix("minecraft:").unwrap_or(&id);
    match slot {
        UnifiedSlotId::Toolbelt(i) => match ToolType::from_slot_index(i as usize) {
            Some(expected) => {
                if ModelItemStack::new(id.clone(), 1).tool_type() == Some(expected) {
                    return true;
                }
                let tag = match expected {
                    ToolType::Sword => "minecraft:swords",
                    ToolType::Axe => "minecraft:axes",
                    ToolType::Pickaxe => "minecraft:pickaxes",
                    ToolType::Shovel => "minecraft:shovels",
                    ToolType::Hoe => "minecraft:hoes",
                };
                if let (Ok(reg), Ok(item_id)) = (
                    mod_api::Identifier::parse("minecraft:item"),
                    mod_api::Identifier::parse(&id),
                ) {
                    let entry = mod_api::RegistryEntry::new(reg, item_id);
                    return host.registry_entry_in_tag(&entry, tag).unwrap_or(false);
                }
                false
            }
            None => false,
        },
        UnifiedSlotId::Armor(0) => {
            bare.ends_with("_helmet")
                || bare == "turtle_helmet"
                || bare == "carved_pumpkin"
                || bare.ends_with("_head")
                || bare.ends_with("_skull")
        }
        UnifiedSlotId::Armor(1) => bare.ends_with("_chestplate") || bare == "elytra",
        UnifiedSlotId::Armor(2) => bare.ends_with("_leggings"),
        UnifiedSlotId::Armor(3) => bare.ends_with("_boots"),
        UnifiedSlotId::Elytra => bare == "elytra" || bare.ends_with("_elytra"),
        UnifiedSlotId::CraftOutput => false,
        _ => true,
    }
}

fn compute_craft_match(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    craft_in: &[Option<ApiItemStack>; 4],
) -> Option<RecipeMatch> {
    let level = player.level(host)?;
    let recipe_type = Identifier::parse("minecraft:crafting").ok()?;
    let input = RecipeInput::Crafting {
        width: 2,
        height: 2,
        slots: craft_in.to_vec(),
    };
    level.recipe_for(host, &recipe_type, &input).ok().flatten()
}

pub fn player_key(player: PlayerRef) -> String {
    format!("{:032x}", player.uuid())
}

fn read_vanilla_map(host: &dyn ServerHost, player: PlayerRef) -> HashMap<u32, ApiItemStack> {
    player
        .inventory_slots(host)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn read_vanilla_ender_map(host: &dyn ServerHost, player: PlayerRef) -> HashMap<u32, ApiItemStack> {
    player
        .ender_chest(host)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn build_snapshot(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    open_ender_request: bool,
) -> FullScreenSnapshot {
    let uuid = player_key(player);
    let is_creative = player.is_creative(host).unwrap_or(false);
    let selected_slot = player.selected_slot(host).unwrap_or(0).min(8);
    let vmap = read_vanilla_map(host, player);
    let emap = read_vanilla_ender_map(host, player);

    let (active_offhand, offhand_count, extra_rows, ender_rows) = {
        let mut st = crate::get_state().lock().unwrap();
        let ender_rows = st.config.ender_chest_rows.clamp(1, 9);
        let inv = st.get_or_create_inventory(&uuid);
        (
            inv.active_offhand_index,
            inv.offhand_slots.len().max(1),
            inv.extra_rows.len() / 9,
            ender_rows,
        )
    };

    let mut main = vec![None; 36];
    for i in 0..36u32 {
        main[i as usize] = vmap.get(&i).cloned();
    }
    let armor = [
        vmap.get(&39).cloned(),
        vmap.get(&38).cloned(),
        vmap.get(&37).cloned(),
        vmap.get(&36).cloned(),
    ];
    let vanilla_offhand = vmap.get(&40).cloned();

    let (mode, toolbelt, offhand, extra, ender_chest, craft_in, cursor, elytra) = {
        let mut map = server_slots_map().lock().unwrap();
        let entry = map.entry(uuid.clone()).or_default();
        if entry.offhand.len() < offhand_count {
            entry.offhand.resize(offhand_count, None);
        }
        if entry.extra.len() < extra_rows * 9 {
            entry.extra.resize(extra_rows * 9, None);
        }
        if entry.ender_chest.len() < ender_rows * 9 {
            entry.ender_chest.resize(ender_rows * 9, None);
        }
        for i in 0..27u32 {
            if (i as usize) < entry.ender_chest.len() {
                entry.ender_chest[i as usize] = emap.get(&i).cloned();
            }
        }
        if active_offhand < entry.offhand.len() {
            entry.offhand[active_offhand] = vanilla_offhand;
        }
        (
            entry.mode,
            entry.toolbelt.clone(),
            entry.offhand[..offhand_count].to_vec(),
            entry.extra[..(extra_rows * 9)].to_vec(),
            entry.ender_chest[..(ender_rows * 9)].to_vec(),
            entry.craft_in.clone(),
            entry.cursor.clone(),
            entry.elytra.clone(),
        )
    };

    let craft_out = compute_craft_match(host, player, &craft_in).map(|m| m.output);

    FullScreenSnapshot {
        is_creative,
        mode,
        open_ender_request,
        selected_slot,
        active_offhand,
        offhand_count,
        extra_rows,
        ender_rows,
        main,
        armor,
        offhand,
        toolbelt,
        extra,
        craft_in,
        craft_out,
        ender_chest,
        cursor,
        elytra,
        firework_boost_ticks: 0,
    }
}

pub fn evacuate_excess_server_slots(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    offhand_count: usize,
    extra_rows: usize,
) {
    let uuid = player_key(player);
    let mut overflow = Vec::new();
    {
        let mut map = server_slots_map().lock().unwrap();
        let entry = map.entry(uuid.clone()).or_default();
        if entry.offhand.len() < offhand_count {
            entry.offhand.resize(offhand_count, None);
        } else if entry.offhand.len() > offhand_count {
            for item in entry.offhand.drain(offhand_count..).flatten() {
                overflow.push(item);
            }
        }
        let target_extra = extra_rows * 9;
        if entry.extra.len() < target_extra {
            entry.extra.resize(target_extra, None);
        } else if entry.extra.len() > target_extra {
            for item in entry.extra.drain(target_extra..).flatten() {
                overflow.push(item);
            }
        }
        sync_server_slots_to_model(host, &uuid, entry);
    }
    for item in overflow {
        let _ = player.give_stack(host, item);
    }
}

/// Attempts to insert `incoming` into the player's Deep Pockets `extra` slots (matching stacks first, then empty).
/// Returns `(inserted_count, remaining_stack)`.
pub fn try_pickup_into_extra_slots(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    mut incoming: ApiItemStack,
) -> (u32, Option<ApiItemStack>) {
    let uuid = player_key(player);
    let extra_rows = {
        let mut st = crate::get_state().lock().unwrap();
        st.get_or_create_inventory(&uuid).extra_rows.len() / 9
    };
    if extra_rows == 0 {
        return (0, Some(incoming));
    }
    let max_stack = host.max_stack_size(incoming.item()).unwrap_or(64);
    let start_count = incoming.count();
    let mut inserted = 0u32;

    {
        let mut map = server_slots_map().lock().unwrap();
        let entry = map.entry(uuid.clone()).or_default();
        let target_len = extra_rows * 9;
        if entry.extra.len() < target_len {
            entry.extra.resize(target_len, None);
        }
        // Pass 1: stack onto matching items
        for slot in entry.extra.iter_mut().take(target_len) {
            if let Some(existing) = slot.as_mut() {
                if existing.same_item(&incoming)
                    && existing.same_components(&incoming)
                    && existing.count() < max_stack
                {
                    let room = max_stack - existing.count();
                    let take = room.min(incoming.count());
                    let _ = existing.set_count(existing.count() + take);
                    inserted += take;
                    let rem = incoming.count() - take;
                    if rem == 0 {
                        sync_server_slots_to_model(host, &uuid, entry);
                        return (inserted, None);
                    }
                    let _ = incoming.set_count(rem);
                }
            }
        }
        // Pass 2: empty slots
        for slot in entry.extra.iter_mut().take(target_len) {
            if slot.is_none() {
                let place_count = incoming.count().min(max_stack);
                let mut placed = incoming.clone();
                let _ = placed.set_count(place_count);
                *slot = Some(placed);
                inserted += place_count;
                let rem = incoming.count() - place_count;
                if rem == 0 {
                    sync_server_slots_to_model(host, &uuid, entry);
                    return (inserted, None);
                }
                let _ = incoming.set_count(rem);
            }
        }
        if inserted > 0 {
            sync_server_slots_to_model(host, &uuid, entry);
        }
    }
    let rem_opt = if inserted >= start_count {
        None
    } else {
        Some(incoming)
    };
    (inserted, rem_opt)
}

pub fn send_full_sync(host: &mut dyn ServerHost, player: PlayerRef) {
    let snap = build_snapshot(host, player, false);
    let bytes = snap.encode(host);
    let id = PayloadId(CHANNEL_SCREEN_SYNC.into());
    let _ = player.send_payload(host, &id, PayloadBytes(bytes));
}

pub fn open_ender_chest_screen(host: &mut dyn ServerHost, player: PlayerRef) {
    let uuid = player_key(player);
    server_slots_map()
        .lock()
        .unwrap()
        .entry(uuid)
        .or_default()
        .mode = ScreenMode::EnderChest;
    let snap = build_snapshot(host, player, true);
    let bytes = snap.encode(host);
    let id = PayloadId(CHANNEL_SCREEN_SYNC.into());
    let _ = player.send_payload(host, &id, PayloadBytes(bytes));
}

pub struct ScreenActionHandler;
impl IPayloadHandler for ScreenActionHandler {
    fn handle(
        &self,
        payload: PayloadBytes,
        context: &mut dyn IPayloadContext,
    ) -> Result<(), PayloadHandleError> {
        let Ok(action) = ScreenAction::decode(&payload.0) else {
            return Err(PayloadHandleError::Failed("invalid screen action".into()));
        };
        let Ok(player) = context.player() else {
            return Ok(());
        };
        if let Ok(host) = context.server_host() {
            handle_screen_action(host, player, action);
        }
        Ok(())
    }
}

pub fn send_firework_boost_sync(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    boost_ticks: u32,
) {
    let mut snap = build_snapshot(host, player, false);
    snap.firework_boost_ticks = boost_ticks;
    let id = PayloadId(CHANNEL_SCREEN_SYNC.into());
    let bytes = snap.encode(host);
    let _ = player.send_payload(host, &id, PayloadBytes(bytes));
}

pub struct ScreenSyncHandler;
impl IPayloadHandler for ScreenSyncHandler {
    fn handle(
        &self,
        payload: PayloadBytes,
        context: &mut dyn IPayloadContext,
    ) -> Result<(), PayloadHandleError> {
        if let Ok(host) = context.client_host() {
            if let Ok(snap) = FullScreenSnapshot::decode(host, &payload.0) {
                if snap.open_ender_request {
                    OPEN_ENDER_CLIENT.store(true, Ordering::Relaxed);
                }
                if snap.firework_boost_ticks > 0 {
                    crate::CLIENT_FIREWORK_BOOST_TICKS
                        .store(snap.firework_boost_ticks, Ordering::Relaxed);
                }
                *client_snapshot().lock().unwrap() = snap;
            }
        }
        Ok(())
    }
}

fn read_slot(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    uuid: &str,
    active_offhand: usize,
    slot: UnifiedSlotId,
) -> Option<ApiItemStack> {
    match slot {
        UnifiedSlotId::Main(i) => read_vanilla_map(host, player).get(&(i as u32)).cloned(),
        UnifiedSlotId::Armor(i) => {
            let vslot = 39u32.saturating_sub(i as u32);
            read_vanilla_map(host, player).get(&vslot).cloned()
        }
        UnifiedSlotId::Offhand(i) => {
            if i as usize == active_offhand {
                read_vanilla_map(host, player).get(&40).cloned()
            } else {
                server_slots_map()
                    .lock()
                    .unwrap()
                    .get(uuid)
                    .and_then(|s| s.offhand.get(i as usize).cloned().flatten())
            }
        }
        UnifiedSlotId::Toolbelt(i) => server_slots_map()
            .lock()
            .unwrap()
            .get(uuid)
            .and_then(|s| s.toolbelt.get(i as usize).cloned().flatten()),
        UnifiedSlotId::Extra(i) => server_slots_map()
            .lock()
            .unwrap()
            .get(uuid)
            .and_then(|s| s.extra.get(i as usize).cloned().flatten()),
        UnifiedSlotId::CraftInput(i) => server_slots_map()
            .lock()
            .unwrap()
            .get(uuid)
            .and_then(|s| s.craft_in.get(i as usize).cloned().flatten()),
        UnifiedSlotId::CraftOutput => {
            let craft_in = server_slots_map()
                .lock()
                .unwrap()
                .get(uuid)
                .map(|s| s.craft_in.clone())
                .unwrap_or_default();
            compute_craft_match(host, player, &craft_in).map(|m| m.output)
        }
        UnifiedSlotId::EnderChest(i) => {
            if (i as u32) < 27 {
                read_vanilla_ender_map(host, player)
                    .get(&(i as u32))
                    .cloned()
            } else {
                server_slots_map()
                    .lock()
                    .unwrap()
                    .get(uuid)
                    .and_then(|s| s.ender_chest.get(i as usize).cloned().flatten())
            }
        }
        UnifiedSlotId::Elytra => server_slots_map()
            .lock()
            .unwrap()
            .get(uuid)
            .and_then(|s| s.elytra.clone()),
    }
}

fn write_slot(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    uuid: &str,
    active_offhand: usize,
    slot: UnifiedSlotId,
    stack: Option<ApiItemStack>,
) {
    match slot {
        UnifiedSlotId::Main(i) => {
            let _ = player.set_inventory_slot(host, i as u32, stack);
        }
        UnifiedSlotId::Armor(i) => {
            let vslot = 39u32.saturating_sub(i as u32);
            let _ = player.set_inventory_slot(host, vslot, stack);
            if i == 1 {
                sync_chestplate_glider(host, player, uuid);
            }
        }
        UnifiedSlotId::Offhand(i) => {
            let mut map = server_slots_map().lock().unwrap();
            let entry = map.entry(uuid.to_string()).or_default();
            if entry.offhand.len() <= i as usize {
                entry.offhand.resize(i as usize + 1, None);
            }
            entry.offhand[i as usize] = stack.clone();
            sync_server_slots_to_model(host, uuid, entry);
            drop(map);
            if i as usize == active_offhand {
                let _ = player.set_inventory_slot(host, 40, stack);
            }
        }
        UnifiedSlotId::Toolbelt(i) => {
            if (i as usize) < 5 {
                let mut map = server_slots_map().lock().unwrap();
                let entry = map.entry(uuid.to_string()).or_default();
                entry.toolbelt[i as usize] = stack;
                sync_server_slots_to_model(host, uuid, entry);
            }
        }
        UnifiedSlotId::Extra(i) => {
            let mut map = server_slots_map().lock().unwrap();
            let entry = map.entry(uuid.to_string()).or_default();
            if entry.extra.len() <= i as usize {
                entry.extra.resize(i as usize + 1, None);
            }
            entry.extra[i as usize] = stack;
            sync_server_slots_to_model(host, uuid, entry);
        }
        UnifiedSlotId::CraftInput(i) => {
            if (i as usize) < 4 {
                let mut map = server_slots_map().lock().unwrap();
                let entry = map.entry(uuid.to_string()).or_default();
                entry.craft_in[i as usize] = stack;
            }
        }
        UnifiedSlotId::CraftOutput => {}
        UnifiedSlotId::EnderChest(i) => {
            let mut map = server_slots_map().lock().unwrap();
            let entry = map.entry(uuid.to_string()).or_default();
            if entry.ender_chest.len() <= i as usize {
                entry.ender_chest.resize(i as usize + 1, None);
            }
            entry.ender_chest[i as usize] = stack.clone();
            sync_server_slots_to_model(host, uuid, entry);
            drop(map);
            if (i as u32) < 27 {
                let _ = player.set_ender_chest_slot(host, i as u32, stack);
            }
        }
        UnifiedSlotId::Elytra => {
            {
                let mut map = server_slots_map().lock().unwrap();
                let entry = map.entry(uuid.to_string()).or_default();
                entry.elytra = stack;
            }
            sync_chestplate_glider(host, player, uuid);
        }
    }
}

pub fn sync_chestplate_glider(host: &mut dyn ServerHost, player: PlayerRef, uuid: &str) {
    let has_valid_elytra = server_slots_map()
        .lock()
        .unwrap()
        .get(uuid)
        .and_then(|e| e.elytra.as_ref())
        .is_some_and(|st| {
            let max_dmg = host.max_damage(st.item()).unwrap_or(0);
            st.item().id(host) == "minecraft:elytra"
                && (max_dmg == 0 || st.damage() + 1 < max_dmg)
        });
    let vanilla = read_vanilla_map(host, player);
    // Strip minecraft:glider from any non-elytra chestplate in main inventory/offhand
    for (slot_idx, st) in vanilla.iter() {
        if *slot_idx == 38 {
            continue;
        }
        if st.item().id(host) != "minecraft:elytra"
            && st
                .components()
                .is_some_and(|c| c.as_nbt().get(&"minecraft:glider".into()).is_some())
        {
            let mut clean = st.clone();
            let mut patch = mod_api::Compound::new();
            patch.insert(
                "!minecraft:glider".into(),
                mod_api::Tag::Compound(mod_api::Compound::new()),
            );
            let _ = clean.apply_components(&patch);
            let _ = player.set_inventory_slot(host, *slot_idx, Some(clean));
        }
    }
    if let Some(mut chest) = vanilla.get(&38).cloned() {
        if chest.item().id(host) == "minecraft:elytra" {
            return;
        }
        let has_glider = chest
            .components()
            .is_some_and(|c| c.as_nbt().get(&"minecraft:glider".into()).is_some());
        if has_valid_elytra && !has_glider {
            let mut patch = mod_api::Compound::new();
            patch.insert(
                "minecraft:glider".into(),
                mod_api::Tag::Compound(mod_api::Compound::new()),
            );
            let _ = chest.apply_components(&patch);
            let _ = player.set_inventory_slot(host, 38, Some(chest));
        } else if !has_valid_elytra && has_glider {
            let mut patch = mod_api::Compound::new();
            patch.insert(
                "!minecraft:glider".into(),
                mod_api::Tag::Compound(mod_api::Compound::new()),
            );
            let _ = chest.apply_components(&patch);
            let _ = player.set_inventory_slot(host, 38, Some(chest));
        }
    }
}

fn try_merge_or_place_into(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    uuid: &str,
    active_offhand: usize,
    targets: &[UnifiedSlotId],
    incoming: &mut ApiItemStack,
) -> bool {
    let max_stack = host.max_stack_size(incoming.item()).unwrap_or(64);
    for &t in targets {
        if !can_accept_slot(host, t, incoming) {
            continue;
        }
        if let Some(mut existing) = read_slot(host, player, uuid, active_offhand, t) {
            if existing.same_item(incoming)
                && existing.same_components(incoming)
                && existing.count() < max_stack
            {
                let room = max_stack - existing.count();
                let take = room.min(incoming.count());
                let _ = existing.set_count(existing.count() + take);
                write_slot(host, player, uuid, active_offhand, t, Some(existing));
                let rem = incoming.count() - take;
                if rem == 0 {
                    return true;
                }
                let _ = incoming.set_count(rem);
            }
        }
    }
    for &t in targets {
        if !can_accept_slot(host, t, incoming) {
            continue;
        }
        if read_slot(host, player, uuid, active_offhand, t).is_none() {
            let place_count = incoming.count().min(max_stack);
            let mut placed = incoming.clone();
            let _ = placed.set_count(place_count);
            write_slot(host, player, uuid, active_offhand, t, Some(placed));
            let rem = incoming.count() - place_count;
            if rem == 0 {
                return true;
            }
            let _ = incoming.set_count(rem);
        }
    }
    false
}

fn consume_one_craft(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    uuid: &str,
) -> Option<ApiItemStack> {
    let craft_in = server_slots_map()
        .lock()
        .unwrap()
        .get(uuid)
        .map(|s| s.craft_in.clone())
        .unwrap_or_default();
    let matched = compute_craft_match(host, player, &craft_in)?;
    let mut map = server_slots_map().lock().unwrap();
    let entry = map.entry(uuid.to_string()).or_default();
    for i in 0..4 {
        if let Some(rem) = matched.remainders.get(i).and_then(Option::as_ref) {
            entry.craft_in[i] = Some(rem.clone());
        } else if let Some(cur) = &mut entry.craft_in[i] {
            if cur.count() > 1 {
                let _ = cur.set_count(cur.count() - 1);
            } else {
                entry.craft_in[i] = None;
            }
        }
    }
    Some(matched.output)
}

pub fn handle_screen_action(host: &mut dyn ServerHost, player: PlayerRef, action: ScreenAction) {
    let uuid = player_key(player);
    let (active_offhand, offhand_count, extra_rows, ender_rows) = {
        let mut st = crate::get_state().lock().unwrap();
        let ender_rows = st.config.ender_chest_rows.clamp(1, 9);
        let inv = st.get_or_create_inventory(&uuid);
        (
            inv.active_offhand_index,
            inv.offhand_slots.len().max(1),
            inv.extra_rows.len() / 9,
            ender_rows,
        )
    };

    match action {
        ScreenAction::Open => {
            crate::sync_player_armor_and_capacities(host, player.uuid());
            server_slots_map()
                .lock()
                .unwrap()
                .entry(uuid)
                .or_default()
                .mode = ScreenMode::Inventory;
            send_full_sync(host, player);
        }
        ScreenAction::OpenEnderChest => {
            open_ender_chest_screen(host, player);
        }
        ScreenAction::Close => {
            let (cursor, craft_in) = {
                let mut map = server_slots_map().lock().unwrap();
                let entry = map.entry(uuid.clone()).or_default();
                entry.mode = ScreenMode::Inventory;
                let c = entry.cursor.take();
                let g = std::mem::take(&mut entry.craft_in);
                (c, g)
            };
            if let Some(c) = cursor {
                let _ = player.give_stack(host, c);
            }
            for item in craft_in.into_iter().flatten() {
                let _ = player.give_stack(host, item);
            }
            send_full_sync(host, player);
        }
        ScreenAction::CycleOffhand { delta } => {
            let current_vanilla_offhand = read_vanilla_map(host, player).get(&40).cloned();
            let skip_empty = crate::get_state()
                .lock()
                .unwrap()
                .config
                .skip_empty_offhand_slots;
            let old_idx = {
                let mut st = crate::get_state().lock().unwrap();
                let inv = st.get_or_create_inventory(&uuid);
                inv.active_offhand_index
            };
            let (new_idx, next_vanilla_offhand) = {
                let mut map = server_slots_map().lock().unwrap();
                let entry = map.entry(uuid.clone()).or_default();
                if entry.offhand.len() < offhand_count {
                    entry.offhand.resize(offhand_count, None);
                }
                if old_idx < entry.offhand.len() {
                    entry.offhand[old_idx] = current_vanilla_offhand;
                }
                let len = entry.offhand.len().max(1);
                let step = if delta >= 0 { 1isize } else { -1isize };
                let new_i = if skip_empty {
                    let mut chosen = old_idx.min(len - 1);
                    for offset in 1..=len {
                        let cand = ((old_idx as isize + step * offset as isize).rem_euclid(len as isize)) as usize;
                        if entry.offhand.get(cand).and_then(Option::as_ref).is_some() {
                            chosen = cand;
                            break;
                        }
                    }
                    chosen
                } else {
                    ((old_idx as isize + delta as isize).rem_euclid(len as isize)) as usize
                };
                {
                    let mut st = crate::get_state().lock().unwrap();
                    let inv = st.get_or_create_inventory(&uuid);
                    inv.active_offhand_index = new_i;
                }
                let next_stack = entry.offhand.get(new_i).cloned().flatten();
                sync_server_slots_to_model(host, &uuid, entry);
                (new_i, next_stack)
            };
            let _ = new_idx;
            let _ = player.set_inventory_slot(host, 40, next_vanilla_offhand);
            send_full_sync(host, player);
        }
        ScreenAction::FireworkBoost => {
            crate::try_firework_boost(host, player);
        }
        ScreenAction::ContainerQuickMove { slot } => {
            let active_off = {
                let mut st = crate::get_state().lock().unwrap();
                st.get_or_create_inventory(&uuid).active_offhand_index
            };
            if let Some(mut stack) = read_slot(host, player, &uuid, active_off, slot) {
                let targets: Vec<UnifiedSlotId> = (9..36)
                    .chain(0..9)
                    .map(|i| UnifiedSlotId::Main(i as u8))
                    .collect();
                let emptied =
                    try_merge_or_place_into(host, player, &uuid, active_off, &targets, &mut stack);
                if emptied {
                    write_slot(host, player, &uuid, active_off, slot, None);
                } else {
                    write_slot(host, player, &uuid, active_off, slot, Some(stack));
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::CreativePlace { slot, stack_bytes } => {
            if player.is_creative(host).unwrap_or(false) {
                let mut pos = 0;
                if let Ok(Some(stack)) = decode_api_stack(host, &stack_bytes, &mut pos) {
                    if can_accept_slot(host, slot, &stack) {
                        let active_off = {
                            let mut st = crate::get_state().lock().unwrap();
                            st.get_or_create_inventory(&uuid).active_offhand_index
                        };
                        write_slot(host, player, &uuid, active_off, slot, Some(stack));
                    }
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::MoveMainToSlot { main_slot, target } => {
            let active_off = {
                let mut st = crate::get_state().lock().unwrap();
                st.get_or_create_inventory(&uuid).active_offhand_index
            };
            let src_id = UnifiedSlotId::Main(main_slot);
            if let Some(mut incoming) = read_slot(host, player, &uuid, active_off, src_id) {
                if can_accept_slot(host, target, &incoming) {
                    let emptied = try_merge_or_place_into(
                        host,
                        player,
                        &uuid,
                        active_off,
                        &[target],
                        &mut incoming,
                    );
                    if emptied {
                        write_slot(host, player, &uuid, active_off, src_id, None);
                    } else {
                        write_slot(host, player, &uuid, active_off, src_id, Some(incoming));
                    }
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::MoveSlotToMain {
            src,
            main_slot,
            half,
        } => {
            let active_off = {
                let mut st = crate::get_state().lock().unwrap();
                st.get_or_create_inventory(&uuid).active_offhand_index
            };
            let dst_id = UnifiedSlotId::Main(main_slot);
            if read_slot(host, player, &uuid, active_off, dst_id).is_none() {
                if let Some(mut stack) = read_slot(host, player, &uuid, active_off, src) {
                    if half && stack.count() > 1 {
                        let take = (stack.count() + 1) / 2;
                        let rem = stack.count() - take;
                        let mut picked = stack.clone();
                        let _ = picked.set_count(take);
                        let _ = stack.set_count(rem);
                        write_slot(host, player, &uuid, active_off, src, Some(stack));
                        write_slot(host, player, &uuid, active_off, dst_id, Some(picked));
                    } else {
                        write_slot(host, player, &uuid, active_off, src, None);
                        write_slot(host, player, &uuid, active_off, dst_id, Some(stack));
                    }
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::FillRecipe { grid_items, shift } => {
            // Return existing craft_in items to inventory first
            let old_grid = {
                let mut map = server_slots_map().lock().unwrap();
                let entry = map.entry(uuid.clone()).or_default();
                std::mem::take(&mut entry.craft_in)
            };
            for item in old_grid.into_iter().flatten() {
                let _ = player.give_stack(host, item);
            }
            let max_sets: u32 = if shift { 64 } else { 1 };
            for _set in 0..max_sets {
                // Check if all required items are available for one more set
                let mut avail: HashMap<String, u32> = HashMap::new();
                for m in 0..36u8 {
                    if let Some(s) = read_slot(host, player, &uuid, active_offhand, UnifiedSlotId::Main(m)) {
                        *avail.entry(s.item().id(host)).or_insert(0) += s.count();
                    }
                }
                for e in 0..(extra_rows * 9) {
                    if let Some(s) = read_slot(host, player, &uuid, active_offhand, UnifiedSlotId::Extra(e as u8)) {
                        *avail.entry(s.item().id(host)).or_insert(0) += s.count();
                    }
                }
                let mut can_one = true;
                for (slot_idx, wanted_opt) in grid_items.iter().take(4).enumerate() {
                    let Some(wanted_id) = wanted_opt else {
                        continue;
                    };
                    if let Some(cur) = read_slot(
                        host,
                        player,
                        &uuid,
                        active_offhand,
                        UnifiedSlotId::CraftInput(slot_idx as u8),
                    ) {
                        if cur.item().id(host) != *wanted_id
                            || cur.count() >= host.max_stack_size(cur.item()).unwrap_or(64)
                        {
                            can_one = false;
                            break;
                        }
                    }
                    let cnt = avail.get_mut(wanted_id);
                    if cnt.as_ref().is_none_or(|c| **c == 0) {
                        can_one = false;
                        break;
                    }
                    *cnt.unwrap() -= 1;
                }
                if !can_one {
                    break;
                }
                for (slot_idx, wanted_opt) in grid_items.iter().take(4).enumerate() {
                    let Some(wanted_id) = wanted_opt else {
                        continue;
                    };
                    let mut found: Option<(UnifiedSlotId, ApiItemStack)> = None;
                    for m in 0..36u8 {
                        let sid = UnifiedSlotId::Main(m);
                        if let Some(s) = read_slot(host, player, &uuid, active_offhand, sid) {
                            if s.item().id(host) == *wanted_id {
                                found = Some((sid, s));
                                break;
                            }
                        }
                    }
                    if found.is_none() {
                        for e in 0..(extra_rows * 9) {
                            let sid = UnifiedSlotId::Extra(e as u8);
                            if let Some(s) = read_slot(host, player, &uuid, active_offhand, sid) {
                                if s.item().id(host) == *wanted_id {
                                    found = Some((sid, s));
                                    break;
                                }
                            }
                        }
                    }
                    if let Some((src_id, mut src_stack)) = found {
                        if src_stack.count() > 1 {
                            let _ = src_stack.set_count(src_stack.count() - 1);
                            write_slot(host, player, &uuid, active_offhand, src_id, Some(src_stack.clone()));
                        } else {
                            write_slot(host, player, &uuid, active_offhand, src_id, None);
                        }
                        let cslot = UnifiedSlotId::CraftInput(slot_idx as u8);
                        if let Some(mut existing) =
                            read_slot(host, player, &uuid, active_offhand, cslot)
                        {
                            let _ = existing.set_count(existing.count() + 1);
                            write_slot(host, player, &uuid, active_offhand, cslot, Some(existing));
                        } else {
                            let mut one = src_stack;
                            let _ = one.set_count(1);
                            write_slot(host, player, &uuid, active_offhand, cslot, Some(one));
                        }
                    }
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::DoubleClick { slot } => {
            if slot != UnifiedSlotId::CraftOutput {
                let mut cursor = server_slots_map()
                    .lock()
                    .unwrap()
                    .get(&uuid)
                    .and_then(|s| s.cursor.clone());
                if cursor.is_none() {
                    if let Some(s) = read_slot(host, player, &uuid, active_offhand, slot) {
                        write_slot(host, player, &uuid, active_offhand, slot, None);
                        cursor = Some(s);
                    }
                }
                if let Some(mut cur) = cursor {
                    let max_s = host.max_stack_size(cur.item()).unwrap_or(64);
                    if cur.count() < max_s {
                        let mode = server_slots_map()
                            .lock()
                            .unwrap()
                            .get(&uuid)
                            .map(|s| s.mode)
                            .unwrap_or_default();
                        let mut pool: Vec<UnifiedSlotId> =
                            (0..36).map(|i| UnifiedSlotId::Main(i as u8)).collect();
                        if mode == ScreenMode::EnderChest {
                            for e in 0..(ender_rows * 9) {
                                pool.push(UnifiedSlotId::EnderChest(e as u8));
                            }
                        } else {
                            for e in 0..(extra_rows * 9) {
                                pool.push(UnifiedSlotId::Extra(e as u8));
                            }
                            for o in 0..offhand_count {
                                pool.push(UnifiedSlotId::Offhand(o as u8));
                            }
                        }
                        // Two passes: partial stacks first, then full stacks
                        for partial_only in [true, false] {
                            for &sid in &pool {
                                if cur.count() >= max_s {
                                    break;
                                }
                                if let Some(mut s) =
                                    read_slot(host, player, &uuid, active_offhand, sid)
                                {
                                    if s.same_item(&cur) && s.same_components(&cur) {
                                        if partial_only && s.count() >= max_s {
                                            continue;
                                        }
                                        let room = max_s - cur.count();
                                        let take = room.min(s.count());
                                        let _ = cur.set_count(cur.count() + take);
                                        let rem = s.count() - take;
                                        let next = if rem == 0 {
                                            None
                                        } else {
                                            let _ = s.set_count(rem);
                                            Some(s)
                                        };
                                        write_slot(host, player, &uuid, active_offhand, sid, next);
                                    }
                                }
                            }
                        }
                    }
                    server_slots_map()
                        .lock()
                        .unwrap()
                        .entry(uuid.clone())
                        .or_default()
                        .cursor = Some(cur);
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::LeftClick { slot, shift } => {
            if slot == UnifiedSlotId::CraftOutput {
                if shift {
                    let mut crafted = 0;
                    while crafted < 64 {
                        let Some(out) = consume_one_craft(host, player, &uuid) else {
                            break;
                        };
                        let mut targets: Vec<UnifiedSlotId> =
                            (0..36).map(|i| UnifiedSlotId::Main(i as u8)).collect();
                        for i in 0..(extra_rows * 9) {
                            targets.push(UnifiedSlotId::Extra(i as u8));
                        }
                        let mut stack = out;
                        if !try_merge_or_place_into(
                            host,
                            player,
                            &uuid,
                            active_offhand,
                            &targets,
                            &mut stack,
                        ) {
                            let _ = player.give_stack(host, stack);
                            break;
                        }
                        crafted += 1;
                    }
                } else {
                    let cursor = server_slots_map()
                        .lock()
                        .unwrap()
                        .get(&uuid)
                        .and_then(|s| s.cursor.clone());
                    let craft_in = server_slots_map()
                        .lock()
                        .unwrap()
                        .get(&uuid)
                        .map(|s| s.craft_in.clone())
                        .unwrap_or_default();
                    if let Some(matched) = compute_craft_match(host, player, &craft_in) {
                        let can_take = match &cursor {
                            None => true,
                            Some(c) => {
                                let max_s = host.max_stack_size(c.item()).unwrap_or(64);
                                c.same_item(&matched.output)
                                    && c.same_components(&matched.output)
                                    && c.count() + matched.output.count() <= max_s
                            }
                        };
                        if can_take {
                            if let Some(out) = consume_one_craft(host, player, &uuid) {
                                let mut map = server_slots_map().lock().unwrap();
                                let entry = map.entry(uuid.clone()).or_default();
                                match &mut entry.cursor {
                                    None => entry.cursor = Some(out),
                                    Some(c) => {
                                        let _ = c.set_count(c.count() + out.count());
                                    }
                                }
                            }
                        }
                    }
                }
                send_full_sync(host, player);
                return;
            }

            if shift {
                let Some(mut stack) = read_slot(host, player, &uuid, active_offhand, slot) else {
                    send_full_sync(host, player);
                    return;
                };
                let mode = server_slots_map()
                    .lock()
                    .unwrap()
                    .get(&uuid)
                    .map(|s| s.mode)
                    .unwrap_or_default();
                let mut targets = Vec::new();
                if mode == ScreenMode::EnderChest {
                    match slot {
                        UnifiedSlotId::EnderChest(_) => {
                            for m in 0..36u8 {
                                targets.push(UnifiedSlotId::Main(m));
                            }
                        }
                        UnifiedSlotId::Main(_) => {
                            for e in 0..(ender_rows * 9) {
                                targets.push(UnifiedSlotId::EnderChest(e as u8));
                            }
                        }
                        _ => {}
                    }
                } else {
                    match slot {
                        UnifiedSlotId::Main(i) => {
                            for a in 0..4u8 {
                                targets.push(UnifiedSlotId::Armor(a));
                            }
                            targets.push(UnifiedSlotId::Elytra);
                            for t in 0..5u8 {
                                targets.push(UnifiedSlotId::Toolbelt(t));
                            }
                            for e in 0..(extra_rows * 9) {
                                targets.push(UnifiedSlotId::Extra(e as u8));
                            }
                            if i < 9 {
                                for m in 9..36u8 {
                                    targets.push(UnifiedSlotId::Main(m));
                                }
                            } else {
                                for m in 0..9u8 {
                                    targets.push(UnifiedSlotId::Main(m));
                                }
                            }
                        }
                        UnifiedSlotId::Extra(_) => {
                            for m in 0..36u8 {
                                targets.push(UnifiedSlotId::Main(m));
                            }
                        }
                        _ => {
                            for m in 0..36u8 {
                                targets.push(UnifiedSlotId::Main(m));
                            }
                            for e in 0..(extra_rows * 9) {
                                targets.push(UnifiedSlotId::Extra(e as u8));
                            }
                        }
                    }
                }
                let all_moved = try_merge_or_place_into(
                    host,
                    player,
                    &uuid,
                    active_offhand,
                    &targets,
                    &mut stack,
                );
                let remainder = if all_moved { None } else { Some(stack) };
                write_slot(host, player, &uuid, active_offhand, slot, remainder);
                crate::sync_player_armor_and_capacities(host, player.uuid());
                send_full_sync(host, player);
                return;
            }

            let cursor = server_slots_map()
                .lock()
                .unwrap()
                .get(&uuid)
                .and_then(|s| s.cursor.clone());
            let slot_item = read_slot(host, player, &uuid, active_offhand, slot);

            match (cursor, slot_item) {
                (None, None) => {}
                (None, Some(s)) => {
                    write_slot(host, player, &uuid, active_offhand, slot, None);
                    server_slots_map()
                        .lock()
                        .unwrap()
                        .entry(uuid.clone())
                        .or_default()
                        .cursor = Some(s);
                }
                (Some(c), None) => {
                    if can_accept_slot(host, slot, &c) {
                        write_slot(host, player, &uuid, active_offhand, slot, Some(c));
                        server_slots_map()
                            .lock()
                            .unwrap()
                            .entry(uuid.clone())
                            .or_default()
                            .cursor = None;
                    }
                }
                (Some(mut c), Some(mut s)) => {
                    if can_accept_slot(host, slot, &c) {
                        if s.same_item(&c) && s.same_components(&c) {
                            let max_s = host.max_stack_size(s.item()).unwrap_or(64);
                            let room = max_s.saturating_sub(s.count());
                            let take = room.min(c.count());
                            if take > 0 {
                                let _ = s.set_count(s.count() + take);
                                write_slot(host, player, &uuid, active_offhand, slot, Some(s));
                                let rem = c.count() - take;
                                let next_cursor = if rem == 0 {
                                    None
                                } else {
                                    let _ = c.set_count(rem);
                                    Some(c)
                                };
                                server_slots_map()
                                    .lock()
                                    .unwrap()
                                    .entry(uuid.clone())
                                    .or_default()
                                    .cursor = next_cursor;
                            }
                        } else {
                            write_slot(host, player, &uuid, active_offhand, slot, Some(c));
                            server_slots_map()
                                .lock()
                                .unwrap()
                                .entry(uuid.clone())
                                .or_default()
                                .cursor = Some(s);
                        }
                    }
                }
            }
            if matches!(slot, UnifiedSlotId::Armor(1) | UnifiedSlotId::Armor(2)) {
                crate::sync_player_armor_and_capacities(host, player.uuid());
            }
            send_full_sync(host, player);
        }
        ScreenAction::RightClick { slot } => {
            if slot == UnifiedSlotId::CraftOutput {
                handle_screen_action(
                    host,
                    player,
                    ScreenAction::LeftClick {
                        slot,
                        shift: false,
                    },
                );
                return;
            }
            let cursor = server_slots_map()
                .lock()
                .unwrap()
                .get(&uuid)
                .and_then(|s| s.cursor.clone());
            let slot_item = read_slot(host, player, &uuid, active_offhand, slot);
            match (cursor, slot_item) {
                (None, Some(mut s)) => {
                    let half = (s.count() + 1) / 2;
                    let rem = s.count() - half;
                    let mut picked = s.clone();
                    let _ = picked.set_count(half);
                    let next_slot = if rem == 0 {
                        None
                    } else {
                        let _ = s.set_count(rem);
                        Some(s)
                    };
                    write_slot(host, player, &uuid, active_offhand, slot, next_slot);
                    server_slots_map()
                        .lock()
                        .unwrap()
                        .entry(uuid.clone())
                        .or_default()
                        .cursor = Some(picked);
                }
                (Some(mut c), None) => {
                    if can_accept_slot(host, slot, &c) {
                        let mut one = c.clone();
                        let _ = one.set_count(1);
                        write_slot(host, player, &uuid, active_offhand, slot, Some(one));
                        let rem = c.count() - 1;
                        let next_c = if rem == 0 {
                            None
                        } else {
                            let _ = c.set_count(rem);
                            Some(c)
                        };
                        server_slots_map()
                            .lock()
                            .unwrap()
                            .entry(uuid.clone())
                            .or_default()
                            .cursor = next_c;
                    }
                }
                (Some(mut c), Some(mut s)) => {
                    if can_accept_slot(host, slot, &c) && s.same_item(&c) && s.same_components(&c)
                    {
                        let max_s = host.max_stack_size(s.item()).unwrap_or(64);
                        if s.count() < max_s {
                            let _ = s.set_count(s.count() + 1);
                            write_slot(host, player, &uuid, active_offhand, slot, Some(s));
                            let rem = c.count() - 1;
                            let next_c = if rem == 0 {
                                None
                            } else {
                                let _ = c.set_count(rem);
                                Some(c)
                            };
                            server_slots_map()
                                .lock()
                                .unwrap()
                                .entry(uuid.clone())
                                .or_default()
                                .cursor = next_c;
                        }
                    }
                }
                (None, None) => {}
            }
            send_full_sync(host, player);
        }
        ScreenAction::NumberKey { slot, hotbar } => {
            if hotbar < 9 && slot != UnifiedSlotId::CraftOutput {
                let target_hotbar = UnifiedSlotId::Main(hotbar);
                let a = read_slot(host, player, &uuid, active_offhand, slot);
                let b = read_slot(host, player, &uuid, active_offhand, target_hotbar);
                let ok_a = b.as_ref().is_none_or(|s| can_accept_slot(host, slot, s));
                let ok_b = a
                    .as_ref()
                    .is_none_or(|s| can_accept_slot(host, target_hotbar, s));
                if ok_a && ok_b {
                    write_slot(host, player, &uuid, active_offhand, slot, b);
                    write_slot(host, player, &uuid, active_offhand, target_hotbar, a);
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::Drop { slot, whole_stack } => {
            if let Some(slot_id) = slot {
                if slot_id != UnifiedSlotId::CraftOutput {
                    if let Some(mut s) = read_slot(host, player, &uuid, active_offhand, slot_id) {
                        if whole_stack || s.count() == 1 {
                            write_slot(host, player, &uuid, active_offhand, slot_id, None);
                            if let Ok(Some(unthrown)) = player.drop_stack(host, s, true) {
                                write_slot(
                                    host,
                                    player,
                                    &uuid,
                                    active_offhand,
                                    slot_id,
                                    Some(unthrown),
                                );
                            }
                        } else {
                            let mut drop_one = s.clone();
                            let _ = drop_one.set_count(1);
                            let _ = s.set_count(s.count() - 1);
                            write_slot(host, player, &uuid, active_offhand, slot_id, Some(s));
                            let _ = player.drop_stack(host, drop_one, true);
                        }
                    }
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::DragDistribute { button, slots } => {
            if !slots.is_empty() {
                let cursor = server_slots_map()
                    .lock()
                    .unwrap()
                    .get(&uuid)
                    .and_then(|s| s.cursor.clone());
                if let Some(mut cur) = cursor {
                    let per_slot = if button == 1 {
                        1
                    } else {
                        (cur.count() / slots.len() as u32).max(1)
                    };
                    let max_s = host.max_stack_size(cur.item()).unwrap_or(64);
                    let mut remaining = cur.count();
                    for sid in slots {
                        if remaining == 0 {
                            break;
                        }
                        if !can_accept_slot(host, sid, &cur) {
                            continue;
                        }
                        let existing = read_slot(host, player, &uuid, active_offhand, sid);
                        match existing {
                            None => {
                                let put = per_slot.min(remaining).min(max_s);
                                if put > 0 {
                                    let mut placed = cur.clone();
                                    let _ = placed.set_count(put);
                                    write_slot(host, player, &uuid, active_offhand, sid, Some(placed));
                                    remaining -= put;
                                }
                            }
                            Some(mut ex) if ex.same_item(&cur) && ex.same_components(&cur) => {
                                let room = max_s.saturating_sub(ex.count());
                                let put = per_slot.min(remaining).min(room);
                                if put > 0 {
                                    let _ = ex.set_count(ex.count() + put);
                                    write_slot(host, player, &uuid, active_offhand, sid, Some(ex));
                                    remaining -= put;
                                }
                            }
                            _ => {}
                        }
                    }
                    let next_cur = if remaining == 0 {
                        None
                    } else {
                        let _ = cur.set_count(remaining);
                        Some(cur)
                    };
                    server_slots_map()
                        .lock()
                        .unwrap()
                        .entry(uuid.clone())
                        .or_default()
                        .cursor = next_cur;
                }
            }
            send_full_sync(host, player);
        }
        ScreenAction::ShiftDoubleClick { slot } => {
            let sample = read_slot(host, player, &uuid, active_offhand, slot).or_else(|| {
                server_slots_map()
                    .lock()
                    .unwrap()
                    .get(&uuid)
                    .and_then(|s| s.cursor.clone())
            });
            if let Some(target_item) = sample {
                let mode = server_slots_map()
                    .lock()
                    .unwrap()
                    .get(&uuid)
                    .map(|s| s.mode)
                    .unwrap_or_default();
                let (sources, destinations): (Vec<UnifiedSlotId>, Vec<UnifiedSlotId>) =
                    if mode == ScreenMode::EnderChest {
                        match slot {
                            UnifiedSlotId::EnderChest(_) => (
                                (0..(ender_rows * 9))
                                    .map(|i| UnifiedSlotId::EnderChest(i as u8))
                                    .collect(),
                                (0..36).map(|i| UnifiedSlotId::Main(i as u8)).collect(),
                            ),
                            _ => (
                                (0..36).map(|i| UnifiedSlotId::Main(i as u8)).collect(),
                                (0..(ender_rows * 9))
                                    .map(|i| UnifiedSlotId::EnderChest(i as u8))
                                    .collect(),
                            ),
                        }
                    } else {
                        match slot {
                            UnifiedSlotId::Extra(_) => (
                                (0..(extra_rows * 9))
                                    .map(|i| UnifiedSlotId::Extra(i as u8))
                                    .collect(),
                                (0..36).map(|i| UnifiedSlotId::Main(i as u8)).collect(),
                            ),
                            _ => (
                                (0..36).map(|i| UnifiedSlotId::Main(i as u8)).collect(),
                                (0..(extra_rows * 9))
                                    .map(|i| UnifiedSlotId::Extra(i as u8))
                                    .collect(),
                            ),
                        }
                    };
                for src in sources {
                    if let Some(mut stack) = read_slot(host, player, &uuid, active_offhand, src) {
                        if stack.same_item(&target_item) && stack.same_components(&target_item) {
                            let all_moved = try_merge_or_place_into(
                                host,
                                player,
                                &uuid,
                                active_offhand,
                                &destinations,
                                &mut stack,
                            );
                            let rem = if all_moved { None } else { Some(stack) };
                            write_slot(host, player, &uuid, active_offhand, src, rem);
                        }
                    }
                }
            }
            send_full_sync(host, player);
        }
    }
}

pub fn active_layout_for_snapshot(snap: &FullScreenSnapshot) -> UnifiedLayout {
    if snap.mode == ScreenMode::EnderChest {
        compute_ender_chest_layout(snap.ender_rows.max(1))
    } else {
        compute_unified_layout(snap.extra_rows, snap.offhand_count.max(1))
    }
}

struct RecipeEntry {
    result: ApiItemStack,
    grid_ids: Vec<Option<String>>,
    craftable: bool,
}

fn recipe_matches_tab(id: &str, tab: usize) -> bool {
    if tab == 0 {
        return true;
    }
    let bare = id.strip_prefix("minecraft:").unwrap_or(id);
    match tab {
        1 => {
            bare.contains("plank")
                || bare.contains("wood")
                || bare.contains("stone")
                || bare.contains("brick")
                || bare.contains("wool")
                || bare.contains("carpet")
                || bare.contains("terracotta")
                || bare.contains("concrete")
                || bare.contains("glass")
                || bare.contains("slab")
                || bare.contains("stairs")
                || bare.contains("block")
                || bare.contains("fence")
                || bare.contains("wall")
        }
        2 => {
            bare.contains("bread")
                || bare.contains("stew")
                || bare.contains("soup")
                || bare.contains("sugar")
                || bare.contains("pie")
                || bare.contains("cookie")
                || bare.contains("cake")
                || bare.contains("apple")
                || bare.contains("melon")
                || bare.contains("dye")
                || bare.contains("paper")
                || bare.contains("book")
                || bare.contains("stick")
                || bare.contains("torch")
                || bare.contains("chest")
        }
        3 => {
            bare.contains("redstone")
                || bare.contains("lever")
                || bare.contains("button")
                || bare.contains("pressure_plate")
                || bare.contains("piston")
                || bare.contains("dispenser")
                || bare.contains("dropper")
                || bare.contains("repeater")
                || bare.contains("comparator")
                || bare.contains("rail")
                || bare.contains("sword")
                || bare.contains("pickaxe")
                || bare.contains("axe")
                || bare.contains("shovel")
                || bare.contains("hoe")
                || bare.contains("shield")
                || bare.contains("helmet")
                || bare.contains("chestplate")
                || bare.contains("leggings")
                || bare.contains("boots")
        }
        _ => true,
    }
}

/// Collects 2x2-compatible crafting recipes filtered by active tab, search string, and craftable toggle.
fn collect_2x2_recipes(host: &dyn ClientHost, snap: &FullScreenSnapshot) -> Vec<RecipeEntry> {
    let mut available: HashMap<String, u32> = HashMap::new();
    for s in snap.main.iter().chain(snap.extra.iter()).flatten() {
        *available.entry(s.item().id(host)).or_insert(0) += s.count();
    }

    let tab = RECIPE_BOOK_TAB.load(Ordering::Relaxed);
    let craftable_only = RECIPE_BOOK_CRAFTABLE_ONLY.load(Ordering::Relaxed);
    let search = recipe_book_search_lock().lock().unwrap().to_lowercase();

    let recipes = host.recipes_of_type("minecraft:crafting");
    let unlocked_populated = UNLOCKED_POPULATED.load(Ordering::Relaxed);
    let unlocked_set = UNLOCKED_RECIPES
        .get_or_init(|| Mutex::new(std::collections::HashSet::new()))
        .lock()
        .unwrap()
        .clone();
    let mut out = Vec::new();
    for r in recipes.iter() {
        if r.ingredients.is_empty() || r.results.is_empty() {
            continue;
        }
        if unlocked_populated && !unlocked_set.contains(&r.id) {
            continue;
        }
        // Check 2x2 dimensions for shaped recipes (exclude 3x3 / 1x3 / 3x1 like shovel, stairs, chest)
        let mut shaped_slots: Option<Vec<usize>> = None;
        if let Some(pattern) = r.json.get("pattern").and_then(|v| v.as_array()) {
            let height = pattern.len();
            let width = pattern
                .iter()
                .filter_map(|row| row.as_str().map(str::len))
                .max()
                .unwrap_or(0);
            if width == 0 || width > 2 || height == 0 || height > 2 {
                continue;
            }
            // Build mapping from non-space pattern positions to r.ingredients index
            let mut slots_2x2 = vec![usize::MAX; 4];
            let mut ing_idx = 0usize;
            for (row_i, row_val) in pattern.iter().enumerate() {
                if let Some(row_str) = row_val.as_str() {
                    for (col_i, ch) in row_str.chars().enumerate() {
                        if ch != ' ' && col_i < 2 && row_i < 2 {
                            slots_2x2[row_i * 2 + col_i] = ing_idx;
                            ing_idx += 1;
                        }
                    }
                }
            }
            shaped_slots = Some(slots_2x2);
        } else if r.ingredients.len() > 4 {
            continue;
        }

        let result = &r.results[0];
        let res_id = result.item().id(host);
        if !recipe_matches_tab(&res_id, tab) {
            continue;
        }
        if !search.is_empty() && !res_id.to_lowercase().contains(&search) {
            continue;
        }

        let mut temp_avail = available.clone();
        let mut grid_ids = vec![None; 4];
        let mut craftable = true;

        if let Some(slots_2x2) = shaped_slots {
            for (pos_2x2, &ing_i) in slots_2x2.iter().enumerate() {
                if ing_i == usize::MAX {
                    continue;
                }
                let Some(ing) = r.ingredients.get(ing_i) else {
                    craftable = false;
                    break;
                };
                if ing.items.is_empty() {
                    continue;
                }
                let picked = ing
                    .order
                    .iter()
                    .chain(ing.items.iter())
                    .find(|id| temp_avail.get(*id).copied().unwrap_or(0) > 0)
                    .cloned();
                if let Some(id) = picked {
                    *temp_avail.get_mut(&id).unwrap() -= 1;
                    grid_ids[pos_2x2] = Some(id);
                } else {
                    craftable = false;
                    grid_ids[pos_2x2] =
                        ing.order.first().or_else(|| ing.items.iter().next()).cloned();
                }
            }
        } else {
            for (i, ing) in r.ingredients.iter().take(4).enumerate() {
                if ing.items.is_empty() {
                    continue;
                }
                let picked = ing
                    .order
                    .iter()
                    .chain(ing.items.iter())
                    .find(|id| temp_avail.get(*id).copied().unwrap_or(0) > 0)
                    .cloned();
                if let Some(id) = picked {
                    *temp_avail.get_mut(&id).unwrap() -= 1;
                    grid_ids[i] = Some(id);
                } else {
                    craftable = false;
                    grid_ids[i] =
                        ing.order.first().or_else(|| ing.items.iter().next()).cloned();
                }
            }
        }

        if craftable_only && !craftable {
            continue;
        }
        out.push(RecipeEntry {
            result: result.clone(),
            grid_ids,
            craftable,
        });
    }
    // Sort craftable first
    out.sort_by(|a, b| b.craftable.cmp(&a.craftable));
    out
}

/// Handles clicks inside the recipe book panel if open; returns `Some(action)` if a recipe was clicked or `Some(None)` if UI consumed click.
pub fn handle_recipe_book_click(
    host: &dyn ClientHost,
    snap: &FullScreenSnapshot,
    origin: [f32; 2],
    layout: &UnifiedLayout,
    mouse: [f32; 2],
    shift: bool,
) -> Option<Option<ScreenAction>> {
    if snap.mode != ScreenMode::Inventory {
        return None;
    }
    let [rx, ry, rw, rh] = layout.recipe_book_btn;
    let bx = origin[0] + rx;
    let by = origin[1] + ry;
    if mouse[0] >= bx && mouse[0] < bx + rw && mouse[1] >= by && mouse[1] < by + rh {
        toggle_recipe_book();
        RECIPE_BOOK_SEARCH_FOCUSED.store(false, Ordering::Relaxed);
        return Some(None);
    }
    if !is_recipe_book_open() {
        return None;
    }
    let panel_w = 147.0;
    let panel_h = 166.0;
    let panel_x = origin[0] - panel_w - 2.0;
    let panel_y = origin[1];

    // Check 4 left tabs
    for tab_idx in 0..4usize {
        let tx = panel_x - 30.0;
        let ty = panel_y + 3.0 + tab_idx as f32 * 27.0;
        if mouse[0] >= tx && mouse[0] < tx + 35.0 && mouse[1] >= ty && mouse[1] < ty + 27.0 {
            RECIPE_BOOK_TAB.store(tab_idx, Ordering::Relaxed);
            RECIPE_BOOK_PAGE.store(0, Ordering::Relaxed);
            RECIPE_BOOK_SEARCH_FOCUSED.store(false, Ordering::Relaxed);
            return Some(None);
        }
    }

    if mouse[0] < panel_x
        || mouse[0] >= panel_x + panel_w
        || mouse[1] < panel_y
        || mouse[1] >= panel_y + panel_h
    {
        RECIPE_BOOK_SEARCH_FOCUSED.store(false, Ordering::Relaxed);
        return None;
    }

    // Search box
    if mouse[0] >= panel_x + 25.0
        && mouse[0] < panel_x + 106.0
        && mouse[1] >= panel_y + 13.0
        && mouse[1] < panel_y + 27.0
    {
        RECIPE_BOOK_SEARCH_FOCUSED.store(true, Ordering::Relaxed);
        return Some(None);
    }
    RECIPE_BOOK_SEARCH_FOCUSED.store(false, Ordering::Relaxed);

    // Craftable filter toggle
    if mouse[0] >= panel_x + 110.0
        && mouse[0] < panel_x + 136.0
        && mouse[1] >= panel_y + 12.0
        && mouse[1] < panel_y + 28.0
    {
        let cur = RECIPE_BOOK_CRAFTABLE_ONLY.load(Ordering::Relaxed);
        RECIPE_BOOK_CRAFTABLE_ONLY.store(!cur, Ordering::Relaxed);
        RECIPE_BOOK_PAGE.store(0, Ordering::Relaxed);
        return Some(None);
    }

    // Check prev/next page buttons
    let footer_y = panel_y + 137.0;
    if mouse[1] >= footer_y && mouse[1] < footer_y + 17.0 {
        if mouse[0] >= panel_x + 93.0 && mouse[0] < panel_x + 105.0 {
            let p = RECIPE_BOOK_PAGE.load(Ordering::Relaxed);
            if p > 0 {
                RECIPE_BOOK_PAGE.store(p - 1, Ordering::Relaxed);
            }
            return Some(None);
        }
        if mouse[0] >= panel_x + 120.0 && mouse[0] < panel_x + 132.0 {
            let p = RECIPE_BOOK_PAGE.load(Ordering::Relaxed);
            RECIPE_BOOK_PAGE.store(p + 1, Ordering::Relaxed);
            return Some(None);
        }
    }

    let recipes = collect_2x2_recipes(host, snap);
    let page = RECIPE_BOOK_PAGE.load(Ordering::Relaxed);
    let start = page * 20;
    for (i, entry) in recipes.iter().skip(start).take(20).enumerate() {
        let col = i % 5;
        let row = i / 5;
        let sx = panel_x + 11.0 + col as f32 * 25.0;
        let sy = panel_y + 31.0 + row as f32 * 25.0;
        if mouse[0] >= sx && mouse[0] < sx + 25.0 && mouse[1] >= sy && mouse[1] < sy + 25.0 {
            if entry.craftable {
                return Some(Some(ScreenAction::FillRecipe {
                    grid_items: entry.grid_ids.clone(),
                    shift,
                }));
            }
            return Some(None);
        }
    }
    Some(None)
}

fn draw_window_panel(draw: &mut dyn DrawContext, x: f32, y: f32, w: f32, h: f32, dark_mode: bool) {
    let (bg, hi, lo, border) = if dark_mode {
        (0xFF4F_4F4F, 0xFF66_6666, 0xFF22_2222, 0xFF00_0000)
    } else {
        (0xFFC6_C6C6, 0xFFFF_FFFF, 0xFF55_5555, 0xFF00_0000)
    };
    // Vanilla rounded-corner container silhouette (2px corner bevel)
    draw.fill([x + 2.0, y, w - 4.0, h], border);
    draw.fill([x, y + 2.0, w, h - 4.0], border);
    draw.fill([x + 1.0, y + 1.0, w - 2.0, h - 2.0], border);

    // Inner highlight (top & left, 1px inset from rounded border)
    draw.fill([x + 2.0, y + 1.0, w - 5.0, 2.0], hi);
    draw.fill([x + 1.0, y + 2.0, 2.0, h - 5.0], hi);

    // Inner shadow (bottom & right, 1px inset from rounded border)
    draw.fill([x + 3.0, y + h - 3.0, w - 5.0, 2.0], lo);
    draw.fill([x + w - 3.0, y + 3.0, 2.0, h - 5.0], lo);

    // Main panel interior
    draw.fill([x + 3.0, y + 3.0, w - 6.0, h - 6.0], bg);
    draw.fill([x + 2.0, y + 2.0, 1.0, 1.0], hi);
    draw.fill([x + w - 3.0, y + h - 3.0, 1.0, 1.0], lo);
}

fn draw_item_tooltip(
    host: &dyn ClientHost,
    draw: &mut dyn DrawContext,
    item: &ApiItemStack,
    mouse: [f32; 2],
) {
    let advanced = host.gui_options().advanced_item_tooltips;
    if let Some(elements) = mod_api::ItemResource::of(item).tooltip(host, advanced, false) {
        let lines: Vec<Text> = elements
            .into_iter()
            .filter_map(|el| match el {
                TooltipElement::Text(mut t, col) => {
                    if col != u32::MAX && t.style.color.is_none() {
                        let [r, g, b, _a] = col.to_le_bytes();
                        t.style.color = Some(format!("#{r:02x}{g:02x}{b:02x}"));
                    }
                    Some(t)
                }
                _ => None,
            })
            .collect();
        if !lines.is_empty() {
            let _ = draw.gui_tooltip(&lines, mouse);
            return;
        }
    }
    let _ = draw.gui_tooltip(&[Text::literal(item.item().id(host))], mouse);
}

fn compute_drag_preview(
    host: &dyn ClientHost,
    snap: &FullScreenSnapshot,
    drag: Option<&(u8, Vec<UnifiedSlotId>)>,
) -> (HashMap<UnifiedSlotId, ApiItemStack>, Option<ApiItemStack>) {
    let mut previews = HashMap::new();
    let Some((button, slots)) = drag else {
        return (previews, snap.cursor.clone());
    };
    if slots.len() <= 1 {
        return (previews, snap.cursor.clone());
    }
    let Some(cur) = &snap.cursor else {
        return (previews, None);
    };
    let total = cur.count();
    let n = slots.len() as u32;
    let per_slot = if *button == 1 {
        1
    } else {
        (total / n).max(1)
    };
    let max_s = host.max_stack_size(cur.item()).unwrap_or(64);
    let mut remaining = total;
    for &sid in slots {
        if remaining == 0 {
            break;
        }
        let existing_count = snap.get_slot(sid).map_or(0, |s| s.count());
        let room = max_s.saturating_sub(existing_count);
        let add = per_slot.min(remaining).min(room);
        if add > 0 {
            let mut st = cur.clone();
            let _ = st.set_count(existing_count + add);
            previews.insert(sid, st);
            remaining -= add;
        }
    }
    let rem_cursor = if remaining > 0 {
        let mut st = cur.clone();
        let _ = st.set_count(remaining);
        Some(st)
    } else {
        None
    };
    (previews, rem_cursor)
}

pub fn elytra_slot_hit_test(origin: [f32; 2], mx: f32, my: f32) -> bool {
    if !ELYTRA_POPUP_OPEN.load(Ordering::Relaxed) {
        return false;
    }
    let ex = origin[0] - 10.0;
    let ey = origin[1] + 26.0;
    mx >= ex && mx < ex + 18.0 && my >= ey && my < ey + 18.0
}

/// Renders the unified Inventorial screen when `host.mod_screen() == Some(MOD_SCREEN_ID)`.
pub fn render_unified_screen(
    host: &dyn ClientHost,
    draw: &mut dyn DrawContext,
    mouse: [f32; 2],
    dark_mode: bool,
) {
    let snap = client_snapshot().lock().unwrap().clone();
    let [sw, sh] = draw.size();
    let layout = active_layout_for_snapshot(&snap);
    let book_open = snap.mode == ScreenMode::Inventory && is_recipe_book_open();
    let origin = layout.origin_with_book(sw, sh, book_open);
    let [ox, oy] = origin;

    // 1. Dimmed screen backdrop
    draw.fill([0.0, 0.0, sw, sh], 0x8010_1010);

    // 2. Main container window
    draw_window_panel(draw, ox, oy, layout.width, layout.height, dark_mode);

    let label_col = if dark_mode { 0xFFD0_D0D0 } else { 0xFF40_4040 };

    if snap.mode == ScreenMode::EnderChest {
        let _ = draw.text("Ender Chest", [ox + 8.0, oy + 6.0], label_col, false);
        let _ = draw.text(
            "Inventory",
            [ox + layout.craft_arrow_pos[0], oy + layout.craft_arrow_pos[1]],
            label_col,
            false,
        );
    } else {
        // 3. Paper-doll beveled slot outline + black box shifted 1px right & 1px down
        let pd = layout.paper_doll_rect;
        let pd_x = ox + pd[0];
        let pd_y = oy + pd[1];
        let pd_w = pd[2] - pd[0];
        let pd_h = pd[3] - pd[1];
        let (pd_hi, pd_lo, pd_corner) = if dark_mode {
            (0xFF66_6666, 0xFF16_1616, 0xFF38_3838)
        } else {
            (0xFFFF_FFFF, 0xFF37_3737, 0xFF8B_8B8B)
        };
        // Bottom/right highlight, top/left shadow (matching vanilla container slot bevel)
        draw.fill([pd_x, pd_y, pd_w, pd_h], pd_hi);
        draw.fill([pd_x, pd_y, pd_w - 1.0, pd_h - 1.0], pd_lo);
        // Bottom-left and top-right 1x1 corner pixels match vanilla slot grey
        draw.fill([pd_x, pd_y + pd_h - 1.0, 1.0, 1.0], pd_corner);
        draw.fill([pd_x + pd_w - 1.0, pd_y, 1.0, 1.0], pd_corner);
        draw.fill([pd_x + 1.0, pd_y + 1.0, pd_w - 2.0, pd_h - 2.0], 0xFF00_0000);
        if let Some(player_ent) = host.camera_entity() {
            let _ = draw.entity(
                &GuiEntity::existing(player_ent),
                GuiEntityView::new(
                    [pd_x + 1.0, pd_y + 1.0, pd_x + pd_w - 1.0, pd_y + pd_h - 1.0],
                    30.0,
                    0.0625,
                    GuiEntityLook::At(mouse),
                ),
            );
        }

        // 4. Crafting label, arrow, and Recipe Book toggle button
        let _ = draw.text(
            "Crafting",
            [ox + layout.craft_label_pos[0], oy + layout.craft_label_pos[1]],
            label_col,
            false,
        );
        let arrow_col = if dark_mode { 0xFF38_3838 } else { 0xFF8B_8B8B };
        let ax = ox + layout.craft_arrow_pos[0] + 1.0;
        let ay = oy + layout.craft_arrow_pos[1] + 2.0;
        // Vanilla 14x11 thick crafting arrow (8x3 shaft + 6px tapered head)
        draw.fill([ax, ay + 4.0, 8.0, 3.0], arrow_col);
        for step in 0..6 {
            let h = 11.0 - step as f32 * 2.0;
            draw.fill([ax + 8.0 + step as f32, ay + step as f32, 1.0, h], arrow_col);
        }
        let [rx, ry, rw, rh] = layout.recipe_book_btn;
        let bx = ox + rx;
        let by = oy + ry;
        let btn_hovered =
            mouse[0] >= bx && mouse[0] < bx + rw && mouse[1] >= by && mouse[1] < by + rh;
        let btn_sprite = if btn_hovered {
            "gui/sprites/recipe_book/button_highlighted"
        } else {
            "gui/sprites/recipe_book/button"
        };
        draw.sprite(btn_sprite, [0.0, 0.0, 20.0, 18.0], [bx, by, rw, rh], None);
    }

    // 5. Render every slot background, silhouette (armor/toolbelt only), item + count/durability, and hover/drag highlight
    let base_hovered = layout.hit_test(origin, mouse[0], mouse[1]);
    let popup_was_open = ELYTRA_POPUP_OPEN.load(Ordering::Relaxed);
    let in_popup_area = popup_was_open
        && mouse[0] >= ox - 14.0
        && mouse[0] <= ox + 26.0
        && mouse[1] >= oy + 22.0
        && mouse[1] <= oy + 48.0;
    let show_elytra_popup = snap.mode == ScreenMode::Inventory
        && (base_hovered == Some(UnifiedSlotId::Armor(1)) || in_popup_area);
    ELYTRA_POPUP_OPEN.store(show_elytra_popup, Ordering::Relaxed);
    let hovered = if show_elytra_popup && elytra_slot_hit_test(origin, mouse[0], mouse[1]) {
        Some(UnifiedSlotId::Elytra)
    } else {
        base_hovered
    };
    if let Some(h_id) = hovered {
        if let Some(drag) = drag_state_lock().lock().unwrap().as_mut() {
            if h_id != UnifiedSlotId::CraftOutput && !drag.slots.contains(&h_id) {
                if let Some(cur) = &snap.cursor {
                    let existing = snap.get_slot(h_id);
                    let accepts = existing.is_none_or(|s| s.same_item(cur) && s.same_components(cur));
                    if accepts {
                        drag.slots.push(h_id);
                    }
                }
            }
        }
    }
    let drag_info = drag_state_lock()
        .lock()
        .unwrap()
        .as_ref()
        .map(|d| (d.button, d.slots.clone()));
    let drag_slots = drag_info
        .as_ref()
        .map(|(_, s)| s.clone())
        .unwrap_or_default();
    let (drag_previews, preview_cursor) =
        compute_drag_preview(host, &snap, drag_info.as_ref());
    for slot_pos in &layout.slots {
        let sx = ox + slot_pos.x;
        let sy = oy + slot_pos.y;
        draw.sprite(
            "gui/sprites/container/slot",
            [0.0, 0.0, 18.0, 18.0],
            [sx, sy, 18.0, 18.0],
            None,
        );

        let stack = drag_previews
            .get(&slot_pos.id)
            .or_else(|| snap.get_slot(slot_pos.id));

        if stack.is_none() {
            match slot_pos.id {
                UnifiedSlotId::Armor(i) => {
                    let icon = ["helmet", "chestplate", "leggings", "boots"][i as usize];
                    draw.sprite(
                        &format!("gui/sprites/container/slot/{icon}"),
                        [0.0, 0.0, 16.0, 16.0],
                        [sx + 1.0, sy + 1.0, 16.0, 16.0],
                        None,
                    );
                }
                UnifiedSlotId::Toolbelt(i) => {
                    if let Some(tt) = ToolType::from_slot_index(i as usize) {
                        let sprite_name = tt
                            .silhouette_texture_path(dark_mode)
                            .replace("textures/", "")
                            .trim_end_matches(".png")
                            .to_string();
                        draw.sprite(
                            &sprite_name,
                            [0.0, 0.0, 16.0, 16.0],
                            [sx + 1.0, sy + 1.0, 16.0, 16.0],
                            None,
                        );
                    }
                }
                _ => {}
            }
        }

        if let Some(item) = stack {
            draw.gui_draw_item_with(item, [sx + 1.0, sy + 1.0], None);
        }

        if hovered == Some(slot_pos.id) || (drag_slots.len() > 1 && drag_slots.contains(&slot_pos.id))
        {
            draw.fill([sx + 1.0, sy + 1.0, 16.0, 16.0], 0x80FF_FFFF);
        }
    }

    // 6. Optional Vanilla-style Recipe Book panel on the left
    let mut recipe_tooltip_item: Option<ApiItemStack> = None;
    if book_open {
        let panel_w = 147.0;
        let panel_h = 166.0;
        let panel_x = ox - panel_w - 2.0;
        let panel_y = oy;

        // Unselected category tabs drawn behind the recipe book panel
        let active_tab = RECIPE_BOOK_TAB.load(Ordering::Relaxed);
        let tab_icons = [
            "minecraft:compass",
            "minecraft:bricks",
            "minecraft:apple",
            "minecraft:redstone",
        ];
        let draw_tab = |draw: &mut dyn DrawContext, idx: usize, icon_id: &str, selected: bool| {
            let tx = panel_x - if selected { 32.0 } else { 30.0 };
            let ty = panel_y + 3.0 + idx as f32 * 27.0;
            let sprite = if selected {
                "gui/sprites/recipe_book/tab_selected"
            } else {
                "gui/sprites/recipe_book/tab"
            };
            draw.sprite(sprite, [0.0, 0.0, 35.0, 27.0], [tx, ty, 35.0, 27.0], None);
            if let Ok(ident) = Identifier::parse(icon_id) {
                if let Some(key) = host.item_key(&ident) {
                    if let Some(icon_stack) = ApiItemStack::new(key, 1) {
                        draw.gui_draw_item_with(&icon_stack, [tx + 9.0, ty + 5.0], None);
                    }
                }
            }
        };
        for (idx, icon_id) in tab_icons.iter().enumerate() {
            if idx != active_tab {
                draw_tab(draw, idx, icon_id, false);
            }
        }

        draw_window_panel(draw, panel_x, panel_y, panel_w, panel_h, dark_mode);
        // Dark recipe-book interior
        draw.fill([panel_x + 7.0, panel_y + 7.0, 133.0, 152.0], 0xFF37_3737);

        // Selected category tab drawn on top of the recipe book panel border
        if let Some(icon_id) = tab_icons.get(active_tab) {
            draw_tab(draw, active_tab, icon_id, true);
        }

        // Search box + magnifying glass
        draw.fill([panel_x + 25.0, panel_y + 13.0, 81.0, 14.0], 0xFF00_0000);
        let search_focused = RECIPE_BOOK_SEARCH_FOCUSED.load(Ordering::Relaxed);
        let outline_col = if search_focused {
            0xFFFF_FFFF
        } else {
            0xFFA0_A0A0
        };
        draw.fill([panel_x + 25.0, panel_y + 13.0, 81.0, 1.0], outline_col);
        draw.fill([panel_x + 25.0, panel_y + 26.0, 81.0, 1.0], outline_col);
        draw.fill([panel_x + 25.0, panel_y + 13.0, 1.0, 14.0], outline_col);
        draw.fill([panel_x + 105.0, panel_y + 13.0, 1.0, 14.0], outline_col);
        let search_val = recipe_book_search_lock().lock().unwrap().clone();
        if search_val.is_empty() && !search_focused {
            let _ = draw.text("Search...", [panel_x + 28.0, panel_y + 16.0], 0xFFAA_AAAA, true);
        } else {
            let disp = if search_focused {
                format!("{search_val}_")
            } else {
                search_val
            };
            let _ = draw.text(&disp, [panel_x + 28.0, panel_y + 16.0], 0xFFFF_FFFF, true);
        }
        draw.sprite(
            "gui/sprites/icon/search",
            [0.0, 0.0, 12.0, 12.0],
            [panel_x + 11.0, panel_y + 14.0, 12.0, 12.0],
            None,
        );

        // Craftable filter toggle
        let craftable_only = RECIPE_BOOK_CRAFTABLE_ONLY.load(Ordering::Relaxed);
        let filter_hover = mouse[0] >= panel_x + 110.0
            && mouse[0] < panel_x + 136.0
            && mouse[1] >= panel_y + 12.0
            && mouse[1] < panel_y + 28.0;
        let filter_sprite = match (craftable_only, filter_hover) {
            (true, true) => "gui/sprites/recipe_book/filter_enabled_highlighted",
            (true, false) => "gui/sprites/recipe_book/filter_enabled",
            (false, true) => "gui/sprites/recipe_book/filter_disabled_highlighted",
            (false, false) => "gui/sprites/recipe_book/filter_disabled",
        };
        draw.sprite(
            filter_sprite,
            [0.0, 0.0, 26.0, 16.0],
            [panel_x + 110.0, panel_y + 12.0, 26.0, 16.0],
            None,
        );

        let recipes = collect_2x2_recipes(host, &snap);
        let max_pages = (recipes.len() + 19) / 20;
        let page = RECIPE_BOOK_PAGE
            .load(Ordering::Relaxed)
            .min(max_pages.saturating_sub(1));
        RECIPE_BOOK_PAGE.store(page, Ordering::Relaxed);

        for (i, entry) in recipes.iter().skip(page * 20).take(20).enumerate() {
            let col = i % 5;
            let row = i / 5;
            let sx = panel_x + 11.0 + col as f32 * 25.0;
            let sy = panel_y + 31.0 + row as f32 * 25.0;
            let slot_sprite = if entry.craftable {
                "gui/sprites/recipe_book/slot_craftable"
            } else {
                "gui/sprites/recipe_book/slot_uncraftable"
            };
            draw.sprite(slot_sprite, [0.0, 0.0, 25.0, 25.0], [sx, sy, 25.0, 25.0], None);
            draw.gui_draw_item_with(&entry.result, [sx + 4.0, sy + 4.0], None);
            if mouse[0] >= sx && mouse[0] < sx + 25.0 && mouse[1] >= sy && mouse[1] < sy + 25.0 {
                draw.fill([sx + 2.0, sy + 2.0, 21.0, 21.0], 0x40FF_FFFF);
                recipe_tooltip_item = Some(entry.result.clone());
            }
        }
        let footer_y = panel_y + 137.0;
        let page_str = format!("{}/{}", page + 1, max_pages.max(1));
        let _ = draw.text(&page_str, [panel_x + 62.0, footer_y + 4.0], 0xFFFF_FFFF, true);
        draw.sprite(
            "gui/sprites/recipe_book/page_backward",
            [0.0, 0.0, 12.0, 17.0],
            [panel_x + 93.0, footer_y, 12.0, 17.0],
            None,
        );
        draw.sprite(
            "gui/sprites/recipe_book/page_forward",
            [0.0, 0.0, 12.0, 17.0],
            [panel_x + 120.0, footer_y, 12.0, 17.0],
            None,
        );
    }

    // 5b. Connected Chestplate + Elytra hover box extending left from Chestplate slot
    if show_elytra_popup {
        let px = ox - 14.0;
        let py = oy + 22.0;
        draw_window_panel(draw, px, py, 44.0, 26.0, dark_mode);
        let ex = ox - 10.0;
        let ey = oy + 26.0;
        draw.sprite(
            "gui/sprites/container/slot",
            [0.0, 0.0, 18.0, 18.0],
            [ex, ey, 18.0, 18.0],
            None,
        );
        if let Some(el_stack) = &snap.elytra {
            draw.gui_draw_item_with(el_stack, [ex + 1.0, ey + 1.0], None);
        } else {
            let icon = if dark_mode {
                "inventorial:gui/sprites/dark/slot_elytra"
            } else {
                "inventorial:gui/sprites/slot_elytra"
            };
            draw.sprite(
                icon,
                [0.0, 0.0, 16.0, 16.0],
                [ex + 1.0, ey + 1.0, 16.0, 16.0],
                None,
            );
        }
        if hovered == Some(UnifiedSlotId::Elytra) {
            draw.fill([ex + 1.0, ey + 1.0, 16.0, 16.0], 0x40FF_FFFF);
        }

        // Re-draw Chestplate slot on the connected panel so both slots sit side by side
        let cx = ox + 8.0;
        let cy = oy + 26.0;
        draw.sprite(
            "gui/sprites/container/slot",
            [0.0, 0.0, 18.0, 18.0],
            [cx, cy, 18.0, 18.0],
            None,
        );
        if let Some(chest_stack) = snap.get_slot(UnifiedSlotId::Armor(1)) {
            draw.gui_draw_item_with(chest_stack, [cx + 1.0, cy + 1.0], None);
        } else {
            draw.sprite(
                "gui/sprites/container/slot/chestplate",
                [0.0, 0.0, 16.0, 16.0],
                [cx + 1.0, cy + 1.0, 16.0, 16.0],
                None,
            );
        }
        if hovered == Some(UnifiedSlotId::Armor(1)) {
            draw.fill([cx + 1.0, cy + 1.0, 16.0, 16.0], 0x80FF_FFFF);
        }
    }

    // 7. Cursor item stack or hovered tooltip
    if let Some(cursor_item) = &preview_cursor {
        draw.gui_draw_item_with(cursor_item, [mouse[0] - 8.0, mouse[1] - 8.0], None);
    } else if snap.cursor.is_none() {
        if let Some(h_id) = hovered {
            if let Some(item) = snap.get_slot(h_id) {
                draw_item_tooltip(host, draw, item, mouse);
            }
        } else if let Some(ritem) = recipe_tooltip_item {
            draw_item_tooltip(host, draw, &ritem, mouse);
        }
    }
}

struct ContainerExtensionLayout {
    panels: Vec<[f32; 4]>,
    slots: Vec<(UnifiedSlotId, f32, f32)>,
}

fn compute_container_extension_slots(
    screen_id: &str,
    screen_slots_len: usize,
    rect: [f32; 4],
    gui_size: [f32; 2],
    snap: &FullScreenSnapshot,
) -> ContainerExtensionLayout {
    let mut layout = ContainerExtensionLayout {
        panels: Vec::new(),
        slots: Vec::new(),
    };
    let is_creative = screen_id == "creative";
    let is_creative_survival = is_creative && matches!(screen_slots_len, 41 | 46);
    if is_creative && !is_creative_survival {
        return layout;
    }

    let [rx, ry, rw, _rh] = rect;
    let [_gw, _gh] = gui_size;

    // 1. Deep Pockets: left side in Creative Survival tab, right side in all other container GUIs
    let dp_count = snap.extra.len();
    if dp_count > 0 {
        let rows = dp_count.min(6);
        let cols = (dp_count + rows - 1) / rows;
        let pw = 10.0 + (cols as f32) * 18.0;
        let ph = 10.0 + (rows as f32) * 18.0;
        let px = if is_creative_survival {
            (rx - pw - 2.0).max(2.0)
        } else {
            rx + rw + 2.0
        };
        let py = ry;
        layout.panels.push([px, py, pw, ph]);
        for i in 0..dp_count {
            let col = (i / rows) as f32;
            let row = (i % rows) as f32;
            layout.slots.push((
                UnifiedSlotId::Extra(i as u8),
                px + 5.0 + col * 18.0,
                py + 5.0 + row * 18.0,
            ));
        }
    }

    // 2. Creative Survival Inventory tab: Toolbelt on the right, Offhand to the right of Toolbelt
    if is_creative_survival {
        let tb_x = rx + rw + 2.0;
        let tb_y = ry;
        let tb_w = 26.0;
        layout.panels.push([tb_x, tb_y, tb_w, 98.0]);
        for i in 0..5 {
            layout.slots.push((
                UnifiedSlotId::Toolbelt(i as u8),
                tb_x + 4.0,
                tb_y + 4.0 + (i as f32) * 18.0,
            ));
        }

        let off_count = snap.offhand.len();
        if off_count > 0 {
            let rows = off_count.min(4);
            let cols = (off_count + rows - 1) / rows;
            let ow = 8.0 + (cols as f32) * 18.0;
            let oh = 8.0 + (rows as f32) * 18.0;
            let ox = tb_x + tb_w + 2.0;
            let oy = ry;
            layout.panels.push([ox, oy, ow, oh]);
            for i in 0..off_count {
                let col = (i / rows) as f32;
                let row = (i % rows) as f32;
                layout.slots.push((
                    UnifiedSlotId::Offhand(i as u8),
                    ox + 4.0 + col * 18.0,
                    oy + 4.0 + row * 18.0,
                ));
            }
        }
    }

    layout
}

pub fn draw_container_extension_foreground(
    host: &dyn ClientHost,
    event: &mut mod_api::ScreenRenderForegroundEvent<'_>,
) {
    if host.mod_screen().is_some() {
        return;
    }
    if event.screen.id != "creative" && event.screen.menu.is_none() {
        return;
    }
    let snap = client_snapshot().lock().unwrap().clone();
    let gui_size = event.draw.size();
    let layout = compute_container_extension_slots(
        &event.screen.id,
        event.screen.slots,
        event.screen.rect,
        gui_size,
        &snap,
    );
    if layout.slots.is_empty() {
        return;
    }

    let dark = crate::effective_dark_mode();
    for [px, py, pw, ph] in layout.panels {
        draw_window_panel(event.draw, px, py, pw, ph, dark);
    }

    let mouse = event.screen.mouse;
    let mut hovered_slot: Option<UnifiedSlotId> = None;
    for &(slot_id, sx, sy) in &layout.slots {
        if mouse[0] >= sx && mouse[0] < sx + 18.0 && mouse[1] >= sy && mouse[1] < sy + 18.0 {
            hovered_slot = Some(slot_id);
            break;
        }
    }
    // Also detect hover over player's main/hotbar slots during an active drag
    if hovered_slot.is_none() && snap.cursor.is_some() {
        let [rx, ry, _, rh] = event.screen.rect;
        let is_creative = event.screen.id == "creative";
        for slot in 0..36usize {
            let (sx, sy) = if is_creative {
                if slot < 9 {
                    (rx + 8.0 + slot as f32 * 18.0, ry + 111.0)
                } else {
                    (
                        rx + 8.0 + ((slot - 9) % 9) as f32 * 18.0,
                        ry + 53.0 + ((slot - 9) / 9) as f32 * 18.0,
                    )
                }
            } else if slot < 9 {
                (rx + 8.0 + slot as f32 * 18.0, ry + rh - 24.0)
            } else {
                (
                    rx + 8.0 + ((slot - 9) % 9) as f32 * 18.0,
                    ry + rh - 82.0 + ((slot - 9) / 9) as f32 * 18.0,
                )
            };
            if mouse[0] >= sx && mouse[0] < sx + 18.0 && mouse[1] >= sy && mouse[1] < sy + 18.0 {
                hovered_slot = Some(UnifiedSlotId::Main(slot as u8));
                break;
            }
        }
    }
    if let Some(h_id) = hovered_slot {
        if let Some(drag) = drag_state_lock().lock().unwrap().as_mut() {
            if !drag.slots.contains(&h_id) {
                if let Some(cur) = &snap.cursor {
                    let existing = snap.get_slot(h_id);
                    let accepts =
                        existing.is_none_or(|s| s.same_item(cur) && s.same_components(cur));
                    if accepts {
                        drag.slots.push(h_id);
                    }
                }
            }
        }
    }
    let drag_info = drag_state_lock()
        .lock()
        .unwrap()
        .as_ref()
        .map(|d| (d.button, d.slots.clone()));
    let drag_slots = drag_info
        .as_ref()
        .map(|(_, s)| s.clone())
        .unwrap_or_default();
    let (drag_previews, _preview_cursor) =
        compute_drag_preview(host, &snap, drag_info.as_ref());

    for &(slot_id, sx, sy) in &layout.slots {
        event.draw.sprite(
            "gui/sprites/container/slot",
            [0.0, 0.0, 18.0, 18.0],
            [sx, sy, 18.0, 18.0],
            None,
        );
        if let Some(stack) = drag_previews
            .get(&slot_id)
            .or_else(|| snap.get_slot(slot_id))
        {
            event.draw.gui_draw_item_with(stack, [sx + 1.0, sy + 1.0], None);
        } else {
            match slot_id {
                UnifiedSlotId::Toolbelt(i) => {
                    let icon = match i {
                        0 => Some("sword"),
                        1 => Some("axe"),
                        2 => Some("pickaxe"),
                        3 => Some("shovel"),
                        4 => Some("hoe"),
                        _ => None,
                    };
                    if let Some(ic) = icon {
                        event.draw.sprite(
                            &format!("gui/sprites/container/slot/{ic}"),
                            [0.0, 0.0, 16.0, 16.0],
                            [sx + 1.0, sy + 1.0, 16.0, 16.0],
                            None,
                        );
                    }
                }
                UnifiedSlotId::Offhand(i) if i as usize == snap.active_offhand => {
                    event.draw.sprite(
                        "gui/sprites/container/slot/shield",
                        [0.0, 0.0, 16.0, 16.0],
                        [sx + 1.0, sy + 1.0, 16.0, 16.0],
                        None,
                    );
                }
                _ => {}
            }
        }
        if hovered_slot == Some(slot_id)
            || (drag_slots.len() > 1 && drag_slots.contains(&slot_id))
        {
            event.draw.fill([sx + 1.0, sy + 1.0, 16.0, 16.0], 0x40FF_FFFF);
        }
    }

    // Draw drag previews over main inventory slots if dragging across them
    if drag_slots.len() > 1 {
        let [rx, ry, _, rh] = event.screen.rect;
        let is_creative = event.screen.id == "creative";
        for (uid, pstack) in &drag_previews {
            if let UnifiedSlotId::Main(slot) = *uid {
                let s = slot as usize;
                let (sx, sy) = if is_creative {
                    if s < 9 {
                        (rx + 8.0 + s as f32 * 18.0, ry + 111.0)
                    } else {
                        (
                            rx + 8.0 + ((s - 9) % 9) as f32 * 18.0,
                            ry + 53.0 + ((s - 9) / 9) as f32 * 18.0,
                        )
                    }
                } else if s < 9 {
                    (rx + 8.0 + s as f32 * 18.0, ry + rh - 24.0)
                } else {
                    (
                        rx + 8.0 + ((s - 9) % 9) as f32 * 18.0,
                        ry + rh - 82.0 + ((s - 9) / 9) as f32 * 18.0,
                    )
                };
                event.draw.gui_draw_item_with(pstack, [sx + 1.0, sy + 1.0], None);
                event.draw.fill([sx + 1.0, sy + 1.0, 16.0, 16.0], 0x40FF_FFFF);
            }
        }
    }
}

pub fn draw_container_extension_post(
    host: &dyn ClientHost,
    event: &mut mod_api::ScreenRenderPostEvent<'_>,
) {
    if host.mod_screen().is_some() {
        return;
    }
    if event.screen.id != "creative" && event.screen.menu.is_none() {
        return;
    }
    let snap = client_snapshot().lock().unwrap().clone();
    let gui_size = event.draw.size();
    let layout = compute_container_extension_slots(
        &event.screen.id,
        event.screen.slots,
        event.screen.rect,
        gui_size,
        &snap,
    );
    if layout.slots.is_empty() {
        return;
    }
    let mouse = event.screen.mouse;
    let drag_info = drag_state_lock()
        .lock()
        .unwrap()
        .as_ref()
        .map(|d| (d.button, d.slots.clone()));
    let (_drag_previews, preview_cursor) =
        compute_drag_preview(host, &snap, drag_info.as_ref());

    if let Some(cursor_item) = &preview_cursor {
        event
            .draw
            .gui_draw_item_with(cursor_item, [mouse[0] - 8.0, mouse[1] - 8.0], None);
    } else if snap.cursor.is_none() {
        let vanilla_carried = host
            .local_player()
            .and_then(|p| p.container_menu(host))
            .and_then(|m| m.carried(host))
            .flatten();
        if vanilla_carried.is_none() {
            for &(slot_id, sx, sy) in &layout.slots {
                if mouse[0] >= sx && mouse[0] < sx + 18.0 && mouse[1] >= sy && mouse[1] < sy + 18.0 {
                    if let Some(item) = snap.get_slot(slot_id) {
                        draw_item_tooltip(host, event.draw, item, mouse);
                    }
                    break;
                }
            }
        }
    }
}

pub fn is_over_container_extension(
    host: &dyn ClientHost,
    screen_id: &str,
    screen_slots: usize,
    has_menu: bool,
    rect: [f32; 4],
    mx: f32,
    my: f32,
) -> bool {
    if host.mod_screen().is_some() {
        return false;
    }
    if screen_id != "creative" && !has_menu {
        return false;
    }
    let snap = client_snapshot().lock().unwrap().clone();
    let gui_size = crate::get_last_gui_size();
    let layout = compute_container_extension_slots(screen_id, screen_slots, rect, gui_size, &snap);
    layout
        .panels
        .iter()
        .any(|[px, py, pw, ph]| mx >= *px && mx < *px + *pw && my >= *py && my < *py + *ph)
}

pub fn handle_container_extension_click(
    host: &dyn ClientHost,
    event: &mut mod_api::ScreenMouseButtonPressedPreEvent<'_>,
) -> bool {
    if host.mod_screen().is_some() {
        return false;
    }
    if event.screen.id != "creative" && event.screen.menu.is_none() {
        return false;
    }
    let snap = client_snapshot().lock().unwrap().clone();
    let gui_size = crate::get_last_gui_size();
    let layout = compute_container_extension_slots(
        &event.screen.id,
        event.screen.slots,
        event.screen.rect,
        gui_size,
        &snap,
    );
    let [mx, my] = event.screen.mouse;
    let shift = (event.modifiers & 0x0001) != 0;
    for (slot_id, sx, sy) in layout.slots {
        if mx >= sx && mx < sx + 18.0 && my >= sy && my < sy + 18.0 {
            if shift {
                let is_double = crate::check_double_click(slot_id);
                if event.button == 0 && is_double {
                    crate::queue_screen_action(ScreenAction::ShiftDoubleClick { slot: slot_id });
                } else {
                    crate::queue_screen_action(ScreenAction::ContainerQuickMove { slot: slot_id });
                }
            } else if event.screen.id != "creative" {
                let menu_opt = host.local_player().and_then(|p| p.container_menu(host));
                if let Some(menu) = menu_opt {
                    let vanilla_carried = menu.carried(host).flatten();
                    if vanilla_carried.is_some() {
                        *crate::PENDING_VANILLA_TO_EXTRA.lock().unwrap() = Some(slot_id);
                        return true;
                    } else if snap.cursor.is_none() && snap.get_slot(slot_id).is_some() {
                        let count = menu.slot_count(host).unwrap_or(0);
                        for s in 0..count {
                            if let Some(mod_api::MenuSlotBacking::Player(inv_idx)) =
                                menu.slot_backing(host, s)
                            {
                                if inv_idx < 36 && menu.slot(host, s) == Some(None) {
                                    crate::queue_screen_action(ScreenAction::MoveSlotToMain {
                                        src: slot_id,
                                        main_slot: inv_idx as u8,
                                        half: event.button == 1,
                                    });
                                    *crate::DELAYED_MENU_PICKUP.lock().unwrap() = Some((2, s));
                                    return true;
                                }
                            }
                        }
                    }
                }
                // Fallback (singleplayer where container_menu is None): use snap.cursor directly
                let is_double = crate::check_double_click(slot_id);
                if event.button == 0 && is_double {
                    crate::queue_screen_action(ScreenAction::DoubleClick { slot: slot_id });
                } else if snap.cursor.is_some() && (event.button == 0 || event.button == 1) {
                    *drag_state_lock().lock().unwrap() = Some(DragState {
                        button: event.button as u8,
                        slots: vec![slot_id],
                    });
                } else if event.button == 1 {
                    crate::queue_screen_action(ScreenAction::RightClick { slot: slot_id });
                } else if event.button == 0 {
                    crate::queue_screen_action(ScreenAction::LeftClick {
                        slot: slot_id,
                        shift: false,
                    });
                }
            } else if snap.cursor.is_none() {
                let vanilla_carried = host
                    .local_player()
                    .and_then(|p| p.container_menu(host))
                    .and_then(|m| m.carried(host))
                    .flatten();
                if let Some(carried) = vanilla_carried {
                    let mut stack_bytes = Vec::new();
                    encode_api_stack(host, &Some(carried), &mut stack_bytes);
                    crate::queue_screen_action(ScreenAction::CreativePlace {
                        slot: slot_id,
                        stack_bytes,
                    });
                } else {
                    let is_double = crate::check_double_click(slot_id);
                    if event.button == 0 && is_double {
                        crate::queue_screen_action(ScreenAction::DoubleClick { slot: slot_id });
                    } else if event.button == 1 {
                        crate::queue_screen_action(ScreenAction::RightClick { slot: slot_id });
                    } else if event.button == 0 {
                        crate::queue_screen_action(ScreenAction::LeftClick {
                            slot: slot_id,
                            shift: false,
                        });
                    }
                }
            } else {
                let is_double = crate::check_double_click(slot_id);
                if event.button == 0 && is_double {
                    crate::queue_screen_action(ScreenAction::DoubleClick { slot: slot_id });
                } else if snap.cursor.is_some() && (event.button == 0 || event.button == 1) {
                    *drag_state_lock().lock().unwrap() = Some(DragState {
                        button: event.button as u8,
                        slots: vec![slot_id],
                    });
                } else if event.button == 1 {
                    crate::queue_screen_action(ScreenAction::RightClick { slot: slot_id });
                } else if event.button == 0 {
                    crate::queue_screen_action(ScreenAction::LeftClick {
                        slot: slot_id,
                        shift: false,
                    });
                }
            }
            return true;
        }
    }
    let is_creative_survival =
        event.screen.id == "creative" && matches!(event.screen.slots, 41 | 46);
    if is_creative_survival || (event.screen.id != "creative" && snap.cursor.is_some()) {
        let [rx, ry, _, rh] = event.screen.rect;
        let vanilla_carried = host
            .local_player()
            .and_then(|p| p.container_menu(host))
            .and_then(|m| m.carried(host))
            .flatten();
        let ctrl_or_alt = (event.modifiers & 0x0006) != 0;
        let allow_main_intercept = if is_creative_survival {
            vanilla_carried.is_none() || snap.cursor.is_some() || shift
        } else {
            (snap.cursor.is_some() && !shift) || (ctrl_or_alt && snap.cursor.is_none())
        };
        if allow_main_intercept {
            for slot in 0..36usize {
                let (sx, sy) = if is_creative_survival {
                    if slot < 9 {
                        (rx + 8.0 + slot as f32 * 18.0, ry + 111.0)
                    } else {
                        (
                            rx + 8.0 + ((slot - 9) % 9) as f32 * 18.0,
                            ry + 53.0 + ((slot - 9) / 9) as f32 * 18.0,
                        )
                    }
                } else if slot < 9 {
                    (rx + 8.0 + slot as f32 * 18.0, ry + rh - 24.0)
                } else {
                    (
                        rx + 8.0 + ((slot - 9) % 9) as f32 * 18.0,
                        ry + rh - 82.0 + ((slot - 9) / 9) as f32 * 18.0,
                    )
                };
                if mx >= sx && mx < sx + 18.0 && my >= sy && my < sy + 18.0 {
                    let uid = UnifiedSlotId::Main(slot as u8);
                    let effective_shift = shift || (!is_creative_survival && ctrl_or_alt);
                    let is_double = crate::check_double_click(uid);
                    if event.button == 0 && effective_shift && is_double {
                        crate::queue_screen_action(ScreenAction::ShiftDoubleClick { slot: uid });
                    } else if event.button == 0 && !effective_shift && is_double {
                        crate::queue_screen_action(ScreenAction::DoubleClick { slot: uid });
                    } else if !effective_shift
                        && snap.cursor.is_some()
                        && (event.button == 0 || event.button == 1)
                    {
                        *drag_state_lock().lock().unwrap() = Some(DragState {
                            button: event.button as u8,
                            slots: vec![uid],
                        });
                    } else if event.button == 1 && !effective_shift {
                        crate::queue_screen_action(ScreenAction::RightClick { slot: uid });
                    } else {
                        crate::queue_screen_action(ScreenAction::LeftClick {
                            slot: uid,
                            shift: effective_shift,
                        });
                    }
                    return true;
                }
            }
        }
    }
    layout
        .panels
        .iter()
        .any(|[px, py, pw, ph]| mx >= *px && mx < *px + *pw && my >= *py && my < *py + *ph)
}
