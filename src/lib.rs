//! # Inventorial
//!
//! A comprehensive inventory expansion mod for Horizon.
//!
//! Features:
//! 1. Dedicated tool slots (Sword, Axe, Pickaxe, Shovel, Hoe) with outline silhouettes,
//!    strict tool type validation, and seamless non-destructive auto-switching.
//! 2. Cycling offhand slots (default 4, rebindable Ctrl + scroll wheel, wrap-around,
//!    HUD preview with darker previous/next item indicators).
//! 3. New enchantments:
//!    - Deep Pockets (up to 4 levels, adds 1 extra row above inventory per level).
//!    - Big Hands (up to 3 levels, adds 4 offhand slots per level).
//!    - Safe item evacuation on downgrade/un-equip (items are never deleted).
//! 4. Dynamic inventory screen scaling:
//!    - Dynamic bounding box calculation and GUI scaling clamp to fit any screen resolution without clipping.

pub mod config;
use mod_api::{
    HostBlockStates, ItemPickedUpEvent, PlayerHarvestCheckEvent, RightClickBlockEvent,
    ScreenCharTypedPreEvent, ScreenMouseButtonReleasedPreEvent, TriState,
};
pub mod enchantments;
pub mod input;
pub mod model;
pub mod screen_controller;
pub mod network;
pub mod slots;
pub mod storage;
pub mod ui;

use config::InventorialConfig;
use enchantments::{BigHandsEnchantment, DeepPocketsEnchantment};
use input::{InputManager, KeyAction, KeybindSettings};
use model::{ItemStack, ModInventoryData, PlayerAction, ToolType};
use network::{ClickType, ModPacket, PacketHandler};
use slots::{AutoSwitchTracker, ExtraPocketsManager};
use storage::SaveManager;
use ui::{
    CustomSlotView, HotbarOffhandSlotDisplay, HotbarOverlayRenderer, HotbarToolSlotDisplay,
    InventoryLayoutMetrics, InventoryScreenLayout, ScreenScaler,
};

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Global state manager for Inventorial.
pub struct InventorialState {
    pub config: InventorialConfig,
    pub inventories: HashMap<String, ModInventoryData>,
    pub auto_switchers: HashMap<String, AutoSwitchTracker>,
    pub input_manager: InputManager,
    pub save_manager: Option<SaveManager>,
}

impl InventorialState {
    pub fn new() -> Self {
        let config = InventorialConfig::default();
        let key_settings = KeybindSettings {
            modifier_key: config.cycle_modifier_key,
            cycle_next_key: None,
            cycle_prev_key: None,
            enable_mouse_wheel_cycling: true,
        };

        Self {
            config,
            inventories: HashMap::new(),
            auto_switchers: HashMap::new(),
            input_manager: InputManager::new(key_settings),
            save_manager: None,
        }
    }

    pub fn get_or_create_inventory(&mut self, player_uuid: &str) -> &mut ModInventoryData {
        self.inventories
            .entry(player_uuid.to_string())
            .or_insert_with(ModInventoryData::default)
    }

    pub fn get_or_create_switcher(&mut self, player_uuid: &str) -> &mut AutoSwitchTracker {
        self.auto_switchers
            .entry(player_uuid.to_string())
            .or_insert_with(AutoSwitchTracker::new)
    }
}

impl Default for InventorialState {
    fn default() -> Self {
        Self::new()
    }
}

static STATE: OnceLock<Arc<Mutex<InventorialState>>> = OnceLock::new();

pub fn get_state() -> &'static Arc<Mutex<InventorialState>> {
    STATE.get_or_init(|| Arc::new(Mutex::new(InventorialState::new())))
}

use mod_api::{
    self, ActiveEffect, AttackEntityEvent, BreakBlockEvent, BuildCreativeModeTabContentsEvent,
    ClientHost, ClientResourcesReloadedEvent, ClientTickPreEvent, Compound, Flow,
    GuiLayerPreEvent, Hooks, Host, HostSide, Identifier, InputKeyEvent,
    KeyAction as ApiKeyAction, KeyConflictContext, KeyMappingPreEvent, KeyRegistration,
    LivingDeathEvent, LivingEquipmentChangeEvent, LivingGetProjectileEvent, MenuClick,
    MenuSlotBacking, MouseButton, MouseScrollingEvent, OnEquipEvent, PayloadBytes, PayloadId,
    PlayerBreakSpeedEvent, PlayerInventoryChangedEvent, PlayerLoggedInEvent, PlayerLoggedOutEvent,
    PlayerRef, PlayerRespawnedEvent, PlayerXpChangeEvent, Registrar, RegistrationContext,
    RightClickItemEvent, ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent,
    ScreenOpening, ScreenOpeningEvent, ScreenRenderForegroundEvent, ScreenRenderPostEvent,
    ServerHost, ServerStoppingEvent, ServerTickEvent, SettingChangedEvent, SettingSpec, Tag,
    UseItemOnBlockEvent, WindowResizeEvent, WorldSavedEvent,
};

/// Remaining client ticks to display the temporary right-hand tool slot on the HUD.
static TOOL_HUD_TICKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// 1-based index into toolbelt for the active preview (0 = none).
static ACTIVE_PREVIEW_TOOL_IDX: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static PENDING_SCREEN_ACTIONS: OnceLock<Mutex<Vec<screen_controller::ScreenAction>>> =
    OnceLock::new();
static CLOSE_MOD_SCREEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LAST_GUI_SIZE: OnceLock<Mutex<[f32; 2]>> = OnceLock::new();
static LAST_SLOT_CLICK: OnceLock<
    Mutex<Option<(std::time::Instant, ui::inventory_screen::UnifiedSlotId)>>,
> = OnceLock::new();

pub fn check_double_click(slot: ui::inventory_screen::UnifiedSlotId) -> bool {
    let mut last = LAST_SLOT_CLICK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap();
    let now = std::time::Instant::now();
    if let Some((prev_t, prev_slot)) = *last {
        if prev_slot == slot && now.duration_since(prev_t).as_millis() <= 300 {
            *last = None;
            return true;
        }
    }
    *last = Some((now, slot));
    false
}
static SEGMENTED_SECTION: Mutex<Option<usize>> = Mutex::new(None);
static PENDING_SEGMENTED_SLOT: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);
static FIREWORK_BOOST_TICKS: OnceLock<Mutex<HashMap<u128, u32>>> = OnceLock::new();
pub static CLIENT_FIREWORK_BOOST_TICKS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
static DARK_MODE_AUTO_DETECTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static NEEDS_DARK_MODE_CHECK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
pub static PENDING_VANILLA_TO_EXTRA: Mutex<Option<ui::inventory_screen::UnifiedSlotId>> =
    Mutex::new(None);
pub static DELAYED_MENU_PICKUP: Mutex<Option<(u8, usize)>> = Mutex::new(None);
pub static DELAYED_MOVE_TO_EXTRA: Mutex<Option<(u8, u8, ui::inventory_screen::UnifiedSlotId)>> =
    Mutex::new(None);
static SETTINGS_SYNCED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static WORLD_DIR: OnceLock<Mutex<Option<std::path::PathBuf>>> = OnceLock::new();

pub fn effective_dark_mode() -> bool {
    let setting = get_state().lock().unwrap().config.dark_mode_setting;
    match setting {
        config::DarkModeSetting::Auto => {
            DARK_MODE_AUTO_DETECTED.load(std::sync::atomic::Ordering::Relaxed)
        }
        config::DarkModeSetting::On => true,
        config::DarkModeSetting::Off => false,
    }
}

fn detect_dark_mode_pack(instance_dir: &std::path::Path) -> bool {
    let options_path = instance_dir.join("options.txt");
    let Ok(text) = std::fs::read_to_string(&options_path) else {
        return false;
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("resourcePacks:") {
            let lower = rest.to_lowercase();
            // Look for enabled dark-mode resource packs in options.txt
            if lower.contains("dark") {
                return true;
            }
        }
    }
    false
}

fn sync_all_settings_from_host(host: &dyn ClientHost) {
    let dark_detected = detect_dark_mode_pack(&host.instance_dir());
    DARK_MODE_AUTO_DETECTED.store(dark_detected, std::sync::atomic::Ordering::Relaxed);
    let mut state = get_state().lock().unwrap();
    if let Some(v) = host.setting_bool("auto_switch_enabled") {
        state.config.auto_switch_enabled = v;
    }
    if let Some(v) = host.setting_bool("totem_any_offhand_slot") {
        state.config.totem_any_offhand_slot = v;
    }
    if let Some(v) = host.setting_f64("ender_chest_rows") {
        state.config.ender_chest_rows = v.round().clamp(1.0, 9.0) as usize;
    }
    if let Some(idx) = host.setting_choice("force_dark_mode") {
        state.config.dark_mode_setting = match idx {
            1 => config::DarkModeSetting::On,
            2 => config::DarkModeSetting::Off,
            _ => config::DarkModeSetting::Auto,
        };
    }
    if let Some(v) = host.setting_bool("empty_hand_till_and_path") {
        state.config.empty_hand_till_and_path = v;
    }
    if let Some(v) = host.setting_bool("can_throw_unloyal_trident") {
        state.config.can_throw_unloyal_trident = v;
    }
    if let Some(v) = host.setting_bool("skip_empty_offhand_slots") {
        state.config.skip_empty_offhand_slots = v;
    }
    if let Some(v) = host.setting_bool("infinity_requires_no_arrows") {
        state.config.infinity_requires_no_arrows = v;
    }
    if let Some(v) = host.setting_bool("swapped_hands") {
        state.config.swapped_hands = v;
    }
    if let Some(idx) = host.setting_choice("segmented_hotbar") {
        state.config.segmented_hotbar = match idx {
            1 => config::SegmentedHotbarMode::VisualOnly,
            2 => config::SegmentedHotbarMode::On,
            3 => config::SegmentedHotbarMode::FunctionOnly,
            _ => config::SegmentedHotbarMode::Off,
        };
    }
}

fn tool_tier_speed(item_id: &str) -> f32 {
    let bare = item_id.strip_prefix("minecraft:").unwrap_or(item_id);
    if bare.starts_with("golden_") {
        12.0
    } else if bare.starts_with("netherite_") {
        9.0
    } else if bare.starts_with("diamond_") {
        8.0
    } else if bare.starts_with("iron_") {
        6.0
    } else if bare.starts_with("stone_") {
        4.0
    } else if bare.starts_with("wooden_") {
        2.0
    } else {
        4.0
    }
}

fn weapon_bonus_damage(host: &dyn Host, stack: &mod_api::ItemStack) -> f32 {
    let id = stack.item().id(host);
    let bare = id.strip_prefix("minecraft:").unwrap_or(&id);
    let base = if bare.ends_with("_sword") {
        if bare.starts_with("netherite_") {
            7.0
        } else if bare.starts_with("diamond_") {
            6.0
        } else if bare.starts_with("iron_") {
            5.0
        } else if bare.starts_with("stone_") {
            4.0
        } else {
            3.0
        }
    } else if bare.ends_with("_axe") {
        if bare.starts_with("netherite_") || bare.starts_with("diamond_") || bare.starts_with("iron_")
        {
            8.0
        } else if bare.starts_with("stone_") {
            8.0
        } else {
            6.0
        }
    } else if bare == "trident" || bare.ends_with("_trident") {
        8.0
    } else {
        0.0
    };
    let model = from_api_stack(host, stack);
    let sharp = model.enchantment_level("minecraft:sharpness");
    if sharp > 0 {
        base + 0.5 * sharp as f32 + 0.5
    } else {
        base
    }
}

fn damage_toolbelt_item(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    tool_idx: usize,
) {
    if player.is_creative(host).unwrap_or(false) {
        return;
    }
    let uuid = screen_controller::player_key(player);
    let mut changed = false;
    {
        let mut map = screen_controller::server_slots_map().lock().unwrap();
        let entry = map.entry(uuid.clone()).or_default();
        if let Some(mut tool) = entry.toolbelt[tool_idx].take() {
            let max_dmg = tool
                .components()
                .and_then(|c| c.as_nbt().get(&"minecraft:max_damage".into()))
                .and_then(|t| match t {
                    Tag::Int(v) => Some(*v as u32),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    let id = tool.item().id(host);
                    let bare = id.strip_prefix("minecraft:").unwrap_or(&id);
                    if bare.starts_with("netherite_") {
                        2031
                    } else if bare.starts_with("diamond_") {
                        1561
                    } else if bare.starts_with("iron_") {
                        250
                    } else if bare.starts_with("stone_") {
                        131
                    } else if bare.starts_with("golden_") {
                        32
                    } else {
                        59
                    }
                });
            let next_dmg = tool.damage().saturating_add(1);
            if next_dmg < max_dmg {
                tool.set_damage(next_dmg);
                entry.toolbelt[tool_idx] = Some(tool);
            }
            changed = true;
            screen_controller::sync_server_slots_to_model(host, &uuid, entry);
        }
    }
    if changed {
        screen_controller::send_full_sync(host, player);
    }
}

fn player_held_is_tool(host: &dyn ServerHost, player: PlayerRef) -> bool {
    let selected = player.selected_slot(host).unwrap_or(0) as u32;
    let held = player
        .inventory_slots(host)
        .unwrap_or_default()
        .into_iter()
        .find(|(slot, _)| *slot == selected)
        .map(|(_, s)| s);
    held.is_some_and(|s| ItemStack::new(s.item().id(host), 1).tool_type().is_some())
}

fn best_toolbelt_tool_for_block_slice(
    host: &dyn Host,
    toolbelt: &[Option<mod_api::ItemStack>; 5],
    state: mod_api::BlockState,
) -> Option<(usize, mod_api::ItemStack, f32)> {
    let block_name = host.block_state_string(state)?;
    let classified = classify_block_tool(&block_name);
    let mut best: Option<(usize, mod_api::ItemStack, f32)> = None;
    for (idx, tool_opt) in toolbelt.iter().enumerate() {
        let Some(tool) = tool_opt else {
            continue;
        };
        let is_correct = host
            .block_states()
            .is_correct_tool_for_drops(state, tool)
            == Some(true);
        let matches_class = classified == ToolType::from_slot_index(idx);
        if !is_correct && !matches_class {
            continue;
        }
        let id = tool.item().id(host);
        let mut speed = tool_tier_speed(&id);
        let eff = from_api_stack(host, tool).enchantment_level("minecraft:efficiency");
        if eff > 0 {
            speed += (eff * eff + 1) as f32;
        }
        if best.as_ref().is_none_or(|(_, _, s)| speed > *s) {
            best = Some((idx, tool.clone(), speed));
        }
    }
    best
}

fn best_toolbelt_tool_for_block(
    host: &dyn ServerHost,
    player: PlayerRef,
    state: mod_api::BlockState,
) -> Option<(usize, mod_api::ItemStack, f32)> {
    let uuid = screen_controller::player_key(player);
    let map = screen_controller::server_slots_map().lock().unwrap();
    let entry = map.get(&uuid)?;
    best_toolbelt_tool_for_block_slice(host, &entry.toolbelt, state)
}

fn classify_block_tool(block_id: &str) -> Option<ToolType> {
    let bare = block_id
        .split('[')
        .next()
        .unwrap_or(block_id)
        .strip_prefix("minecraft:")
        .unwrap_or(block_id);
    if bare.contains("log")
        || bare.contains("wood")
        || bare.contains("plank")
        || bare.contains("fence")
        || bare.contains("chest") && !bare.contains("ender_chest")
        || bare.contains("barrel")
        || bare.contains("crafting_table")
        || bare.contains("bookshelf")
        || bare.contains("melon")
        || bare.contains("pumpkin")
        || bare.contains("bamboo")
        || bare.contains("sign")
        || bare.contains("door") && !bare.contains("iron_")
        || bare.contains("gate")
        || bare.contains("ladder")
        || bare.contains("scaffolding")
        || bare.contains("lectern")
        || bare.contains("composter")
        || bare.contains("beehive")
        || bare.contains("bee_nest")
    {
        return Some(ToolType::Axe);
    }
    if bare.contains("dirt")
        || bare.contains("grass_block")
        || bare.contains("podzol")
        || bare.contains("mycelium")
        || bare.contains("sand") && !bare.contains("sandstone")
        || bare.contains("gravel")
        || bare.contains("clay")
        || bare.contains("snow")
        || bare.contains("soul_soil")
        || bare.contains("mud")
        || bare.contains("farmland")
        || bare.contains("concrete_powder")
    {
        return Some(ToolType::Shovel);
    }
    if bare.contains("leaves")
        || bare.contains("leaf")
        || bare.contains("hay")
        || bare.contains("moss")
        || bare.contains("sculk")
        || bare.contains("sponge")
        || bare.contains("wart")
        || bare.contains("shroomlight")
        || bare.contains("target")
        || bare.contains("kelp")
        || bare.contains("azalea")
    {
        return Some(ToolType::Hoe);
    }
    if bare.contains("stone")
        || bare.contains("deepslate")
        || bare.contains("obsidian")
        || bare.contains("ore")
        || bare.contains("brick")
        || bare.contains("iron")
        || bare.contains("gold")
        || bare.contains("diamond")
        || bare.contains("emerald")
        || bare.contains("copper")
        || bare.contains("netherite")
        || bare.contains("coal")
        || bare.contains("redstone")
        || bare.contains("lapis")
        || bare.contains("quartz")
        || bare.contains("prismarine")
        || bare.contains("basalt")
        || bare.contains("blackstone")
        || bare.contains("andesite")
        || bare.contains("diorite")
        || bare.contains("granite")
        || bare.contains("tuff")
        || bare.contains("calcite")
        || bare.contains("terracotta")
        || bare.contains("concrete")
        || bare.contains("anvil")
        || bare.contains("furnace")
        || bare.contains("hopper")
        || bare.contains("dispenser")
        || bare.contains("dropper")
        || bare.contains("rail")
        || bare.contains("ice")
        || bare.contains("amethyst")
        || bare.contains("netherrack")
        || bare.contains("ender_chest")
        || bare.contains("shulker")
    {
        return Some(ToolType::Pickaxe);
    }
    None
}

fn queue_screen_action(action: screen_controller::ScreenAction) {
    PENDING_SCREEN_ACTIONS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap()
        .push(action);
}

fn set_last_gui_size(w: f32, h: f32) {
    *LAST_GUI_SIZE
        .get_or_init(|| Mutex::new([427.0, 240.0]))
        .lock()
        .unwrap() = [w, h];
}

fn get_last_gui_size() -> [f32; 2] {
    *LAST_GUI_SIZE
        .get_or_init(|| Mutex::new([427.0, 240.0]))
        .lock()
        .unwrap()
}

fn from_api_stack(host: &dyn Host, api: &mod_api::ItemStack) -> ItemStack {
    let id = api.item().id(host);
    let count = api.count().min(u8::MAX as u32) as u8;
    let mut stack = ItemStack::new(id, count);
    let mut read_ench_map = |map: &Compound| {
        for (k, v) in map.iter() {
            let lvl = match v {
                Tag::Int(n) => *n,
                Tag::Short(n) => *n as i32,
                Tag::Byte(n) => *n as i32,
                _ => 0,
            };
            if lvl > 0 {
                let key_str = k.to_string_lossy();
                stack.enchantments.insert(key_str, lvl as u32);
            }
        }
    };
    if let Some(comps) = api.components() {
        let nbt = comps.as_nbt();
        let ench_tag = nbt
            .get(&"minecraft:enchantments".into())
            .or_else(|| nbt.get(&"enchantments".into()));
        if let Some(Tag::Compound(map)) = ench_tag {
            if let Some(Tag::Compound(levels)) = map.get(&"levels".into()) {
                read_ench_map(levels);
            } else {
                read_ench_map(map);
            }
        }
    }
    stack
}

pub fn try_firework_boost(host: &mut dyn ServerHost, player: PlayerRef) {
    let Some(living) = host.living(player.entity()) else {
        return;
    };
    if !living.is_gliding(host).unwrap_or(false) {
        return;
    }
    let uuid = screen_controller::player_key(player);
    let is_creative = player.is_creative(host).unwrap_or(false);
    let active_off = {
        let mut st = get_state().lock().unwrap();
        st.get_or_create_inventory(&uuid).active_offhand_index
    };
    let mut boosted_flight: Option<u32> = None;
    {
        let mut map = screen_controller::server_slots_map().lock().unwrap();
        let entry = map.entry(uuid.clone()).or_default();
        if let Some(slots) = player.inventory_slots(host) {
            let v_off = slots.into_iter().find(|(s, _)| *s == 40).map(|(_, st)| st);
            if active_off < entry.offhand.len() {
                entry.offhand[active_off] = v_off;
            }
        }
        for (i, slot_opt) in entry.offhand.iter_mut().enumerate() {
            if let Some(st) = slot_opt {
                if st.item().id(host) == "minecraft:firework_rocket" && st.count() > 0 {
                    // Check flight duration if present; skip rockets with explosions if desired
                    let mut flight = 1u32;
                    let mut has_explosions = false;
                    if let Some(comps) = st.components() {
                        let nbt = comps.as_nbt();
                        if let Some(Tag::Compound(fw)) = nbt.get(&"minecraft:fireworks".into()) {
                            if let Some(Tag::Byte(f)) = fw.get(&"flight_duration".into()) {
                                flight = (*f).max(1) as u32;
                            } else if let Some(Tag::Int(f)) = fw.get(&"flight_duration".into()) {
                                flight = (*f).max(1) as u32;
                            }
                            if let Some(Tag::List(exp)) = fw.get(&"explosions".into()) {
                                if !exp.is_empty() {
                                    has_explosions = true;
                                }
                            }
                        }
                    }
                    if has_explosions {
                        continue;
                    }
                    if !is_creative {
                        let next_count = st.count().saturating_sub(1);
                        if next_count == 0 {
                            *slot_opt = None;
                        } else {
                            let _ = st.set_count(next_count);
                        }
                        if i == active_off {
                            let _ = player.set_inventory_slot(
                                host,
                                40,
                                entry.offhand.get(active_off).cloned().flatten(),
                            );
                        }
                    }
                    boosted_flight = Some(flight);
                    break;
                }
            }
        }
        if boosted_flight.is_some() {
            screen_controller::sync_server_slots_to_model(host, &uuid, entry);
        }
    }
    if let Some(flight) = boosted_flight {
        let ticks = 10 + flight * 10;
        FIREWORK_BOOST_TICKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(player.uuid(), ticks);
        if let (Some(level), Some(pos)) = (player.level(host), player.entity().position(host)) {
            let _ = level.play_sound(
                host,
                "minecraft:entity.firework_rocket.launch",
                pos,
                mod_api::SoundSource::Ambient,
                3.0,
                1.0,
            );
        }
        let _ = living.swing(host, mod_api::Hand::Main, true);
        screen_controller::send_firework_boost_sync(host, player, ticks);
    }
}

fn to_api_stack(host: &dyn Host, stack: &ItemStack) -> Option<mod_api::ItemStack> {
    if stack.is_empty() {
        return None;
    }
    let ident = Identifier::parse(&stack.id).ok()?;
    let key = host.item_key(&ident)?;
    let mut api = mod_api::ItemStack::new(key, stack.count as u32)?;
    if !stack.enchantments.is_empty() {
        let mut ench_compound = Compound::new();
        for (k, v) in &stack.enchantments {
            ench_compound.insert(k.as_str().into(), Tag::Int(*v as i32));
        }
        let mut patch = Compound::new();
        patch.insert("minecraft:enchantments".into(), Tag::Compound(ench_compound));
        let _ = api.apply_components(&patch);
    }
    Some(api)
}

/// Scans equipped armor (slot 37 = leggings, slot 38 = chestplate) on the server,
/// recalculates Deep Pockets & Big Hands capacities, evacuates overflow via `player.give_stack`,
/// and mirrors the inventory state for local client rendering.
pub fn sync_player_armor_and_capacities(host: &mut dyn ServerHost, uuid: u128) {
    let Some(player) = host.player(uuid) else {
        return;
    };
    let Some(slots) = player.inventory_slots(host) else {
        return;
    };

    let mut legs: Option<ItemStack> = None;
    let mut chest: Option<ItemStack> = None;
    let mut offhand_item: Option<ItemStack> = None;

    for (slot_idx, api_stack) in &slots {
        match *slot_idx {
            37 => legs = Some(from_api_stack(host, api_stack)),
            38 => chest = Some(from_api_stack(host, api_stack)),
            40 => offhand_item = Some(from_api_stack(host, api_stack)),
            _ => {}
        }
    }

    let armor = [None, legs, chest, None];
    let uuid_key = format!("{:032x}", uuid);

    let (offhand_count, extra_rows) = {
        let mut state = get_state().lock().unwrap();
        let config = state.config.clone();
        let dp_lvl = DeepPocketsEnchantment::calculate_level(&armor, &config);
        let bh_lvl = BigHandsEnchantment::calculate_level(&armor, &config);

        let inv = state.get_or_create_inventory(&uuid_key);
        let _ = inv.update_capacities(dp_lvl, bh_lvl, &config);

        // Keep active offhand slot synchronized with vanilla slot 40
        let active_idx = inv.active_offhand_index;
        if let Some(slot) = inv.offhand_slots.get_mut(active_idx) {
            *slot = offhand_item;
        }

        // Mirror to local_player for client UI rendering in singleplayer
        let snapshot = inv.clone();
        state.inventories.insert("local_player".to_string(), snapshot.clone());
        (snapshot.offhand_slots.len().max(1), snapshot.extra_rows.len() / 9)
    };

    screen_controller::evacuate_excess_server_slots(host, player, offhand_count, extra_rows);
}

#[derive(Default)]
pub struct InventorialMod;

#[mod_api::horizon_mod]
impl Hooks for InventorialMod {
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        let _ = r.register_setting(SettingSpec::toggle(
            "auto_switch_enabled",
            "setting.inventorial.auto_switch_enabled",
            true,
        ));
        let _ = r.register_setting(SettingSpec::toggle(
            "totem_any_offhand_slot",
            "setting.inventorial.totem_any_offhand_slot",
            true,
        ));
        let _ = r.register_setting(SettingSpec::range(
            "ender_chest_rows",
            "setting.inventorial.ender_chest_rows",
            3.0,
            9.0,
            1.0,
            6.0,
        ));
        let _ = r.register_setting(SettingSpec::choice(
            "force_dark_mode",
            "setting.inventorial.force_dark_mode",
            [
                "setting.inventorial.force_dark_mode.auto",
                "setting.inventorial.force_dark_mode.on",
                "setting.inventorial.force_dark_mode.off",
            ],
            0,
        ));
        let _ = r.register_setting(SettingSpec::toggle(
            "empty_hand_till_and_path",
            "setting.inventorial.empty_hand_till_and_path",
            true,
        ));
        let _ = r.register_setting(SettingSpec::toggle(
            "can_throw_unloyal_trident",
            "setting.inventorial.can_throw_unloyal_trident",
            false,
        ));
        let _ = r.register_setting(SettingSpec::toggle(
            "skip_empty_offhand_slots",
            "setting.inventorial.skip_empty_offhand_slots",
            false,
        ));
        let _ = r.register_setting(SettingSpec::choice(
            "segmented_hotbar",
            "setting.inventorial.segmented_hotbar",
            [
                "setting.inventorial.segmented_hotbar.off",
                "setting.inventorial.segmented_hotbar.visual_only",
                "setting.inventorial.segmented_hotbar.on",
                "setting.inventorial.segmented_hotbar.function_only",
            ],
            0,
        ));
        let _ = r.register_setting(SettingSpec::toggle(
            "infinity_requires_no_arrows",
            "setting.inventorial.infinity_requires_no_arrows",
            true,
        ));
        let _ = r.register_setting(SettingSpec::toggle(
            "swapped_hands",
            "setting.inventorial.swapped_hands",
            false,
        ));
        r.register_key_mapping(
            KeyRegistration::new("firework_boost", "key.keyboard.v")
                .label("key.inventorial.rocket_boost")
                .category("key.inventorial.category")
                .context(KeyConflictContext::InGame),
        );

        let mut payloads = r.payload_handlers();
        let mut view = payloads.registrar("1");
        view.play_to_server(
            PayloadId(screen_controller::CHANNEL_SCREEN_ACTION.into()),
            Arc::new(screen_controller::ScreenActionHandler),
        );
        view.play_to_client(
            PayloadId(screen_controller::CHANNEL_SCREEN_SYNC.into()),
            Some(Arc::new(screen_controller::ScreenSyncHandler)),
        );
    }

    fn setting_changed(&self, host: &mut dyn ClientHost, event: &mut SettingChangedEvent<'_>) {
        let mut state = get_state().lock().unwrap();
        match event.key {
            "auto_switch_enabled" => {
                if let Some(v) = host.setting_bool("auto_switch_enabled") {
                    state.config.auto_switch_enabled = v;
                }
            }
            "totem_any_offhand_slot" => {
                if let Some(v) = host.setting_bool("totem_any_offhand_slot") {
                    state.config.totem_any_offhand_slot = v;
                }
            }
            "ender_chest_rows" => {
                if let Some(v) = host.setting_f64("ender_chest_rows") {
                    state.config.ender_chest_rows = v.round().clamp(1.0, 9.0) as usize;
                }
            }
            "force_dark_mode" => {
                if let Some(idx) = host.setting_choice("force_dark_mode") {
                    state.config.dark_mode_setting = match idx {
                        1 => config::DarkModeSetting::On,
                        2 => config::DarkModeSetting::Off,
                        _ => config::DarkModeSetting::Auto,
                    };
                }
            }
            "empty_hand_till_and_path" => {
                if let Some(v) = host.setting_bool("empty_hand_till_and_path") {
                    state.config.empty_hand_till_and_path = v;
                }
            }
            "can_throw_unloyal_trident" => {
                if let Some(v) = host.setting_bool("can_throw_unloyal_trident") {
                    state.config.can_throw_unloyal_trident = v;
                }
            }
            "skip_empty_offhand_slots" => {
                if let Some(v) = host.setting_bool("skip_empty_offhand_slots") {
                    state.config.skip_empty_offhand_slots = v;
                }
            }
            "segmented_hotbar" => {
                if let Some(idx) = host.setting_choice("segmented_hotbar") {
                    state.config.segmented_hotbar = match idx {
                        1 => config::SegmentedHotbarMode::VisualOnly,
                        2 => config::SegmentedHotbarMode::On,
                        3 => config::SegmentedHotbarMode::FunctionOnly,
                        _ => config::SegmentedHotbarMode::Off,
                    };
                    *SEGMENTED_SECTION.lock().unwrap() = None;
                }
            }
            "infinity_requires_no_arrows" => {
                if let Some(v) = host.setting_bool("infinity_requires_no_arrows") {
                    state.config.infinity_requires_no_arrows = v;
                }
            }
            "swapped_hands" => {
                if let Some(v) = host.setting_bool("swapped_hands") {
                    state.config.swapped_hands = v;
                }
            }
            _ => {}
        }
    }

    fn build_creative_mode_tab_contents(
        &self,
        host: &mut dyn ClientHost,
        event: &mut BuildCreativeModeTabContentsEvent<'_>,
    ) {
        if event.tab == "minecraft:ingredients" {
            let Ok(ident) = Identifier::parse("minecraft:enchanted_book") else {
                return;
            };
            let Some(book_key) = host.item_key(&ident) else {
                return;
            };
            for (ench_id, max_lvl) in [
                ("inventorial:deep_pockets", 4),
                ("inventorial:big_hands", 3),
            ] {
                for lvl in 1..=max_lvl {
                    let Some(mut api) = mod_api::ItemStack::new(book_key, 1) else {
                        continue;
                    };
                    let mut ench_compound = Compound::new();
                    ench_compound.insert(ench_id.into(), Tag::Int(lvl));
                    let mut patch = Compound::new();
                    patch.insert("minecraft:stored_enchantments".into(), Tag::Compound(ench_compound));
                    let _ = api.apply_components(&patch);
                    event.items.push(api);
                }
            }
        }
    }

    fn client_resources_reloaded(
        &self,
        _host: &mut dyn ClientHost,
        _event: &mut ClientResourcesReloadedEvent,
    ) {
        NEEDS_DARK_MODE_CHECK.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn player_logged_in(&self, host: &mut dyn ServerHost, event: &mut PlayerLoggedInEvent<'_>) {
        if let Some(wdir) = host.world_dir() {
            *WORLD_DIR.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(wdir.clone());
            if let Some(p) = host.player(event.player) {
                let uuid = screen_controller::player_key(p);
                on_player_join(&wdir, &uuid);
                screen_controller::load_server_slots_from_model(host, &uuid);
                screen_controller::sync_chestplate_glider(host, p, &uuid);
            }
        }
        sync_player_armor_and_capacities(host, event.player);
        if let Some(p) = host.player(event.player) {
            screen_controller::send_full_sync(host, p);
        }
    }

    fn player_logged_out(&self, host: &mut dyn ServerHost, _event: &mut PlayerLoggedOutEvent<'_>) {
        if let Some(wdir) = host
            .world_dir()
            .or_else(|| WORLD_DIR.get().and_then(|m| m.lock().unwrap().clone()))
        {
            let _ = on_world_save(&wdir);
        }
    }

    fn world_saved(&self, host: &mut dyn ServerHost, _event: &mut WorldSavedEvent) {
        if let Some(wdir) = host
            .world_dir()
            .or_else(|| WORLD_DIR.get().and_then(|m| m.lock().unwrap().clone()))
        {
            let _ = on_world_save(&wdir);
        }
    }

    fn server_stopping(&self, host: &mut dyn ServerHost, _event: &mut ServerStoppingEvent) {
        if let Some(wdir) = host
            .world_dir()
            .or_else(|| WORLD_DIR.get().and_then(|m| m.lock().unwrap().clone()))
        {
            let _ = on_world_save(&wdir);
        }
    }

    fn player_respawned(&self, host: &mut dyn ServerHost, event: &mut PlayerRespawnedEvent<'_>) {
        sync_player_armor_and_capacities(host, event.player);
        if let Some(p) = host.player(event.player) {
            screen_controller::send_full_sync(host, p);
        }
    }

    fn living_equipment_change(
        &self,
        host: &mut dyn ServerHost,
        event: &mut LivingEquipmentChangeEvent<'_>,
    ) {
        if event.kind == "minecraft:player" {
            let players = host.players().server_players();
            for player in players {
                let uuid = screen_controller::player_key(player);
                screen_controller::sync_chestplate_glider(host, player, &uuid);
                sync_player_armor_and_capacities(host, player.uuid());
                screen_controller::send_full_sync(host, player);
            }
        }
    }

    fn on_equip(&self, host: &mut dyn ServerHost, _event: &mut OnEquipEvent<'_>) {
        let players = host.players().server_players();
        for player in players {
            sync_player_armor_and_capacities(host, player.uuid());
            screen_controller::send_full_sync(host, player);
        }
    }

    fn player_inventory_changed(
        &self,
        host: &mut dyn ServerHost,
        event: &mut PlayerInventoryChangedEvent<'_>,
    ) {
        sync_player_armor_and_capacities(host, event.player);
        if let Some(p) = host.player(event.player) {
            screen_controller::send_full_sync(host, p);
        }
    }

    fn living_death(&self, host: &mut dyn ServerHost, event: &mut LivingDeathEvent<'_>) -> Flow {
        if event.kind == "minecraft:player" {
            let totem_enabled = get_state().lock().unwrap().config.totem_any_offhand_slot;
            if !totem_enabled {
                return Flow::Continue;
            }
            let player_opt = host
                .players()
                .server_players()
                .into_iter()
                .find(|p| p.entity().id() == event.entity);
            if let Some(player) = player_opt {
                let uuid = screen_controller::player_key(player);
                // Check & consume totem from server_slots_map (including active vanilla offhand)
                let consumed_idx: Option<usize> = {
                    let active_off = {
                        let mut st = get_state().lock().unwrap();
                        st.get_or_create_inventory(&uuid).active_offhand_index
                    };
                    let mut map = screen_controller::server_slots_map().lock().unwrap();
                    let entry = map.entry(uuid.clone()).or_default();
                    if let Some(slots) = player.inventory_slots(host) {
                        let v_off = slots.into_iter().find(|(s, _)| *s == 40).map(|(_, st)| st);
                        if active_off < entry.offhand.len() {
                            entry.offhand[active_off] = v_off;
                        }
                    }
                    let mut found = None;
                    for (i, slot_opt) in entry.offhand.iter_mut().enumerate() {
                        if let Some(st) = slot_opt {
                            let bare = st.item().id(host);
                            if bare == "minecraft:totem_of_undying" && st.count() > 0 {
                                let next_count = st.count().saturating_sub(1);
                                if next_count == 0 {
                                    *slot_opt = None;
                                } else {
                                    let _ = st.set_count(next_count);
                                }
                                found = Some(i);
                                break;
                            }
                        }
                    }
                    if let Some(idx) = found {
                        if idx == active_off {
                            let _ = player.set_inventory_slot(
                                host,
                                40,
                                entry.offhand.get(active_off).cloned().flatten(),
                            );
                        }
                        screen_controller::sync_server_slots_to_model(host, &uuid, entry);
                    }
                    found
                };
                if consumed_idx.is_some() {
                    if let Some(living) = host.living(player.entity()) {
                        let _ = living.set_health(host, 1.0);
                        let _ = living.remove_all_effects(host);
                        if let Some(regen) = host.effect_key("minecraft:regeneration") {
                            let _ = living.add_effect(
                                host,
                                &ActiveEffect::new(regen, 900, 1, false, true, true),
                            );
                        }
                        if let Some(abs) = host.effect_key("minecraft:absorption") {
                            let _ = living.add_effect(
                                host,
                                &ActiveEffect::new(abs, 100, 1, false, true, true),
                            );
                        }
                        if let Some(fire) = host.effect_key("minecraft:fire_resistance") {
                            let _ = living.add_effect(
                                host,
                                &ActiveEffect::new(fire, 800, 0, false, true, true),
                            );
                        }
                    }
                    let _ = player.entity().broadcast_event(host, 35);
                    screen_controller::send_full_sync(host, player);
                    return Flow::Cancel;
                }
            }
        }
        Flow::Continue
    }

    fn key_mapping_pre(
        &self,
        _host: &mut dyn ClientHost,
        event: &mut KeyMappingPreEvent<'_>,
    ) -> Flow {
        let seg_mode = get_state().lock().unwrap().config.segmented_hotbar;
        if !matches!(
            seg_mode,
            config::SegmentedHotbarMode::On | config::SegmentedHotbarMode::FunctionOnly
        ) {
            return Flow::Continue;
        }
        if event.pressed {
            if let Some(digit) = event.mapping.strip_prefix("key.hotbar.") {
                if let Ok(num) = digit.parse::<usize>() {
                    if (1..=9).contains(&num) {
                        if num <= 3 {
                            let mut sec_guard = SEGMENTED_SECTION.lock().unwrap();
                            if let Some(sec) = sec_guard.take() {
                                let target = (sec * 3 + (num - 1)).min(8);
                                PENDING_SEGMENTED_SLOT
                                    .store(target as i8, std::sync::atomic::Ordering::Relaxed);
                                screen_controller::client_snapshot()
                                    .lock()
                                    .unwrap()
                                    .selected_slot = target;
                            } else {
                                *sec_guard = Some(num - 1);
                            }
                        }
                        return Flow::Cancel;
                    }
                }
            }
        }
        Flow::Continue
    }

    fn input_key(&self, _host: &mut dyn ClientHost, event: &mut InputKeyEvent<'_>) {
        let action = match event.action {
            ApiKeyAction::Release => KeyAction::Release,
            ApiKeyAction::Press => KeyAction::Press,
            ApiKeyAction::Repeat => KeyAction::Repeat,
        };
        if event.key == "key.keyboard.left.control" || event.key == "key.keyboard.right.control" {
            on_key_event("local_player", 341, action);
        }
        if _host.playing() && _host.mod_screen().is_none() && event.action == ApiKeyAction::Press {
            let seg_mode = get_state().lock().unwrap().config.segmented_hotbar;
            if !matches!(
                seg_mode,
                config::SegmentedHotbarMode::On | config::SegmentedHotbarMode::FunctionOnly
            ) {
                if let Some(digit) = event.key.strip_prefix("key.keyboard.") {
                    if let Ok(num) = digit.parse::<usize>() {
                        if (1..=9).contains(&num) {
                            screen_controller::client_snapshot()
                                .lock()
                                .unwrap()
                                .selected_slot = num - 1;
                        }
                    }
                }
            }
        }
    }

    fn mouse_scrolling(&self, host: &mut dyn ClientHost, event: &mut MouseScrollingEvent) -> Flow {
        // GLFW Ctrl modifier bit is 0x0002
        let ctrl_held = (event.modifiers & 0x0002) != 0
            || host.is_key_down("key.keyboard.left.control")
            || host.is_key_down("key.keyboard.right.control");
        if host.playing() && ctrl_held && event.scroll[1] != 0.0 {
            let delta = if event.scroll[1] > 0.0 { -1 } else { 1 };
            queue_screen_action(screen_controller::ScreenAction::CycleOffhand { delta });
            return Flow::Cancel;
        }
        if host.playing() && host.mod_screen().is_none() && event.scroll[1] != 0.0 {
            *SEGMENTED_SECTION.lock().unwrap() = None;
            let mut snap = screen_controller::client_snapshot().lock().unwrap();
            let delta: isize = if event.scroll[1] > 0.0 { -1 } else { 1 };
            snap.selected_slot = (snap.selected_slot as isize + delta).rem_euclid(9) as usize;
        }
        Flow::Continue
    }

    fn client_tick_pre(&self, host: &mut dyn ClientHost, _event: &mut ClientTickPreEvent) {
        if !SETTINGS_SYNCED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            sync_all_settings_from_host(host);
            NEEDS_DARK_MODE_CHECK.store(false, std::sync::atomic::Ordering::Relaxed);
        } else if NEEDS_DARK_MODE_CHECK.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let dark = detect_dark_mode_pack(&host.instance_dir());
            DARK_MODE_AUTO_DETECTED.store(dark, std::sync::atomic::Ordering::Relaxed);
        }
        let pending_slot = PENDING_SEGMENTED_SLOT.swap(-1, std::sync::atomic::Ordering::Relaxed);
        if pending_slot >= 0 {
            let _ = host.select_hotbar_slot(pending_slot as u8);
            screen_controller::client_snapshot()
                .lock()
                .unwrap()
                .selected_slot = pending_slot as usize;
        }
        let boost_ticks = CLIENT_FIREWORK_BOOST_TICKS.load(std::sync::atomic::Ordering::Relaxed);
        if boost_ticks > 0 {
            if let Some(player) = host.local_player() {
                let entity = player.entity();
                if host
                    .living(entity)
                    .and_then(|l| l.is_gliding(host))
                    .unwrap_or(false)
                {
                    if let (Some(rot), Some(vel)) = (entity.rotation(host), entity.velocity(host)) {
                        let yaw = (rot[0] as f64).to_radians();
                        let pitch = (rot[1] as f64).to_radians();
                        let cos_p = pitch.cos();
                        let lx = -yaw.sin() * cos_p;
                        let ly = -pitch.sin();
                        let lz = yaw.cos() * cos_p;
                        let kick = [
                            (lx * 0.1 + (lx * 1.5 - vel[0]) * 0.5).clamp(-4.0, 4.0) as f32,
                            (ly * 0.1 + (ly * 1.5 - vel[1]) * 0.5).clamp(-4.0, 4.0) as f32,
                            (lz * 0.1 + (lz * 1.5 - vel[2]) * 0.5).clamp(-4.0, 4.0) as f32,
                        ];
                        host.push_player(kick);
                    }
                    CLIENT_FIREWORK_BOOST_TICKS
                        .store(boost_ticks - 1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    CLIENT_FIREWORK_BOOST_TICKS.store(0, std::sync::atomic::Ordering::Relaxed);
                }
            } else {
                CLIENT_FIREWORK_BOOST_TICKS.store(0, std::sync::atomic::Ordering::Relaxed);
            }
        }
        if host.playing() && host.mod_screen().is_none() {
            if host.consume_click("inventorial", "firework_boost") {
                queue_screen_action(screen_controller::ScreenAction::FireworkBoost);
            }
        }
        if screen_controller::take_open_ender_client() {
            host.mod_screen_open(screen_controller::MOD_SCREEN_ID);
        }
        if host.mod_screen() == Some(screen_controller::MOD_SCREEN_ID) {
            screen_controller::refresh_unlocked_recipes(host);
        }
        if CLOSE_MOD_SCREEN.swap(false, std::sync::atomic::Ordering::Relaxed) {
            host.mod_screen_close(screen_controller::MOD_SCREEN_ID);
        }
        if let Some(target_slot) = PENDING_VANILLA_TO_EXTRA.lock().unwrap().take() {
            if let Some(menu) = host.local_player().and_then(|p| p.container_menu(host)) {
                let count = menu.slot_count(host).unwrap_or(0);
                for s in 0..count {
                    if let Some(MenuSlotBacking::Player(inv_idx)) = menu.slot_backing(host, s) {
                        if inv_idx < 36 && menu.slot(host, s) == Some(None) {
                            if menu.click(
                                host,
                                &[MenuClick::Pickup {
                                    slot: Some(s),
                                    button: MouseButton::Left,
                                }],
                            ) {
                                *DELAYED_MOVE_TO_EXTRA.lock().unwrap() =
                                    Some((2, inv_idx as u8, target_slot));
                            }
                            break;
                        }
                    }
                }
            }
        }
        let mut delayed_move = DELAYED_MOVE_TO_EXTRA.lock().unwrap();
        if let Some((ticks, inv_idx, target_slot)) = *delayed_move {
            if ticks <= 1 {
                *delayed_move = None;
                queue_screen_action(screen_controller::ScreenAction::MoveMainToSlot {
                    main_slot: inv_idx,
                    target: target_slot,
                });
            } else {
                *delayed_move = Some((ticks - 1, inv_idx, target_slot));
            }
        }
        drop(delayed_move);

        let mut delayed_pickup = DELAYED_MENU_PICKUP.lock().unwrap();
        if let Some((ticks, menu_slot)) = *delayed_pickup {
            if ticks <= 1 {
                *delayed_pickup = None;
                if let Some(menu) = host.local_player().and_then(|p| p.container_menu(host)) {
                    let _ = menu.click(
                        host,
                        &[MenuClick::Pickup {
                            slot: Some(menu_slot),
                            button: MouseButton::Left,
                        }],
                    );
                }
            } else {
                *delayed_pickup = Some((ticks - 1, menu_slot));
            }
        }
        drop(delayed_pickup);
        let actions: Vec<screen_controller::ScreenAction> = PENDING_SCREEN_ACTIONS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        for action in actions {
            let id = PayloadId(screen_controller::CHANNEL_SCREEN_ACTION.into());
            let _ = host.send_payload_to_server(&id, PayloadBytes(action.encode()));
        }

        let prev = TOOL_HUD_TICKS.load(std::sync::atomic::Ordering::Relaxed);
        if prev > 0 {
            let next = prev - 1;
            TOOL_HUD_TICKS.store(next, std::sync::atomic::Ordering::Relaxed);
            if next == 0 {
                on_action_end("local_player");
            }
        }
    }

    fn window_resize(&self, _host: &mut dyn ClientHost, event: &mut WindowResizeEvent) {
        set_last_gui_size(event.gui_width as f32, event.gui_height as f32);
        let _ = get_inventory_layout(
            "local_player",
            event.gui_width as f32,
            event.gui_height as f32,
            1.0,
        );
    }

    fn register_gui_layers(
        &self,
        _cx: &mod_api::RegistrationContext,
        event: &mut mod_api::RegisterGuiLayersEvent<'_>,
    ) {
        event.layers.push(mod_api::GuiLayer {
            id: "inventorial:hotbar".into(),
            placement: mod_api::GuiLayerPlacement::Above("minecraft:hotbar".into()),
        });
    }

    fn gui_layer_pre(&self, host: &dyn ClientHost, event: &mut GuiLayerPreEvent<'_>) -> Flow {
        if host.camera_entity().is_some() && event.layer == "minecraft:hotbar" {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn gui_layer_draw(&self, host: &dyn ClientHost, event: &mut mod_api::GuiLayerDrawEvent<'_>) {
        if event.layer != "inventorial:hotbar" || host.camera_entity().is_none() {
            return;
        }
        let snap = screen_controller::client_snapshot().lock().unwrap().clone();
        let [w, h] = event.draw.size();
        let hotbar_center_x = w / 2.0;

        let seg_mode = get_state().lock().unwrap().config.segmented_hotbar;
        let visual_seg = matches!(
            seg_mode,
            config::SegmentedHotbarMode::VisualOnly | config::SegmentedHotbarMode::On
        );
        let seg_gap = if visual_seg { 4.0 } else { 0.0 };

        // 1. Re-render the 9-slot hotbar cleanly without the vanilla offhand slot
        //    When swapped_hands is enabled, the 9-slot hotbar shows Offhand slots and the side preview shows Hotbar slots.
        let swapped = get_state().lock().unwrap().config.swapped_hands;
        let bar_x = hotbar_center_x - 91.0;
        let bar_y = h - 22.0;
        let selected = if swapped {
            snap.active_offhand.min(8)
        } else {
            snap.selected_slot.min(8)
        };
        let bar_slots: &[Option<mod_api::ItemStack>] = if swapped {
            &snap.offhand
        } else {
            &snap.main
        };

        if visual_seg {
            // Draw 3 separate 3-slot segments (62px each, 4px gap between segments)
            for seg in 0..3 {
                let sx = bar_x - seg_gap + seg as f32 * (60.0 + seg_gap);
                // Left cap (21px), middle (20px), right cap (21px) = 62px
                let _ = event.draw.sprite(
                    "gui/sprites/hud/hotbar",
                    [0.0, 0.0, 21.0, 22.0],
                    [sx, bar_y, 21.0, 22.0],
                    None,
                );
                let _ = event.draw.sprite(
                    "gui/sprites/hud/hotbar",
                    [21.0, 0.0, 20.0, 22.0],
                    [sx + 21.0, bar_y, 20.0, 22.0],
                    None,
                );
                let _ = event.draw.sprite(
                    "gui/sprites/hud/hotbar",
                    [161.0, 0.0, 21.0, 22.0],
                    [sx + 41.0, bar_y, 21.0, 22.0],
                    None,
                );
            }
            let sel_seg = selected / 3;
            let sel_in_seg = selected % 3;
            let sel_x = bar_x - seg_gap + sel_seg as f32 * (60.0 + seg_gap) + sel_in_seg as f32 * 20.0 - 1.0;
            let _ = event.draw.sprite(
                "gui/sprites/hud/hotbar_selection",
                [0.0, 0.0, 24.0, 23.0],
                [sel_x, bar_y - 1.0, 24.0, 23.0],
                None,
            );
            for i in 0..9 {
                if let Some(Some(api_stack)) = bar_slots.get(i) {
                    let seg = i / 3;
                    let in_seg = i % 3;
                    let ix = bar_x - seg_gap + seg as f32 * (60.0 + seg_gap) + 3.0 + in_seg as f32 * 20.0;
                    event.draw.gui_draw_item_with(api_stack, [ix, bar_y + 3.0], None);
                }
            }
        } else {
            let _ = event.draw.sprite(
                "gui/sprites/hud/hotbar",
                [0.0, 0.0, 182.0, 22.0],
                [bar_x, bar_y, 182.0, 22.0],
                None,
            );
            let _ = event.draw.sprite(
                "gui/sprites/hud/hotbar_selection",
                [0.0, 0.0, 24.0, 23.0],
                [bar_x - 1.0 + selected as f32 * 20.0, bar_y - 1.0, 24.0, 23.0],
                None,
            );
            for i in 0..9 {
                if let Some(Some(api_stack)) = bar_slots.get(i) {
                    event
                        .draw
                        .gui_draw_item_with(api_stack, [bar_x + 3.0 + i as f32 * 20.0, bar_y + 3.0], None);
                }
            }
        }

        // Highlight active 3-slot section when first number key was pressed in Segmented Hotbar mode
        if matches!(
            seg_mode,
            config::SegmentedHotbarMode::On | config::SegmentedHotbarMode::FunctionOnly
        ) {
            if let Some(sec) = *SEGMENTED_SECTION.lock().unwrap() {
                let sec_x = if visual_seg {
                    bar_x - seg_gap + sec as f32 * (60.0 + seg_gap) - 1.0
                } else {
                    bar_x + sec as f32 * 60.0 - 1.0
                };
                let sec_w = 64.0;
                let sec_y = bar_y - 1.0;
                let sec_h = 24.0;
                let col = 0xFFFF_D700;
                event.draw.fill([sec_x, sec_y, sec_w, 1.0], col);
                event.draw.fill([sec_x, sec_y + sec_h - 1.0, sec_w, 1.0], col);
                event.draw.fill([sec_x, sec_y, 1.0, sec_h], col);
                event.draw.fill([sec_x + sec_w - 1.0, sec_y, 1.0, sec_h], col);
            }
        }

        // 2. Render side preview: Offhand slots normally (left), or Hotbar slots when swapped_hands (right)
        let preview_slots: &[Option<mod_api::ItemStack>] = if swapped {
            &snap.main[..snap.main.len().min(9)]
        } else {
            &snap.offhand
        };
        if preview_slots.iter().any(Option::is_some) {
            let len = preview_slots.len();
            let active = if swapped {
                snap.selected_slot.min(len.saturating_sub(1))
            } else {
                snap.active_offhand.min(len.saturating_sub(1))
            };
            let prev_idx = (active + len - 1) % len;
            let next_idx = (active + 1) % len;

            let slot_y = h - 23.0;
            let active_x = if swapped {
                let left_edge_x = hotbar_center_x + 91.0 + seg_gap;
                if len > 2 {
                    left_edge_x + 20.0
                } else {
                    left_edge_x
                }
            } else {
                let right_edge_x = bar_x - seg_gap - 29.0;
                if len > 2 {
                    right_edge_x - 20.0
                } else {
                    right_edge_x
                }
            };
            let bg_sprite = if swapped {
                "gui/sprites/hud/hotbar_offhand_right"
            } else {
                "gui/sprites/hud/hotbar_offhand_left"
            };
            let item_dx = if swapped { 10.0 } else { 3.0 };
            let sel_dx = if swapped { 6.0 } else { -1.0 };

            let draw_preview_slot =
                |batch: &mut dyn mod_api::DrawContext, x: f32, idx: usize, is_selected: bool| {
                    let _ = batch.sprite(
                        bg_sprite,
                        [0.0, 0.0, 29.0, 24.0],
                        [x, slot_y, 29.0, 24.0],
                        if is_selected { None } else { Some(0xBB99_9999) },
                    );
                    if is_selected {
                        let _ = batch.sprite(
                            "gui/sprites/hud/hotbar_selection",
                            [0.0, 0.0, 24.0, 23.0],
                            [x + sel_dx, slot_y, 24.0, 23.0],
                            None,
                        );
                    }
                    if let Some(Some(api_stack)) = preview_slots.get(idx) {
                        batch.gui_draw_item_with(api_stack, [x + item_dx, slot_y + 4.0], None);
                    }
                };

            // Draw prev and next FIRST so active draws on top of their touching edges
            if len > 1 {
                draw_preview_slot(event.draw, active_x - 20.0, prev_idx, false);
            }
            if len > 2 {
                draw_preview_slot(event.draw, active_x + 20.0, next_idx, false);
            }
            draw_preview_slot(event.draw, active_x, active, true);
        }

        // 3. Render temporary toolbelt preview when auto-switch is active (right normally, left when swapped_hands)
        if TOOL_HUD_TICKS.load(std::sync::atomic::Ordering::Relaxed) > 0 {
            if let Some(tool_idx) = ACTIVE_PREVIEW_TOOL_IDX
                .load(std::sync::atomic::Ordering::Relaxed)
                .checked_sub(1)
            {
                if let Some(Some(tool_stack)) = snap.toolbelt.get(tool_idx) {
                    let ty = h - 23.0;
                    if swapped {
                        let tx = bar_x - seg_gap - 29.0;
                        let _ = event.draw.sprite(
                            "gui/sprites/hud/hotbar_offhand_left",
                            [0.0, 0.0, 29.0, 24.0],
                            [tx, ty, 29.0, 24.0],
                            None,
                        );
                        event
                            .draw
                            .gui_draw_item_with(tool_stack, [tx + 3.0, ty + 4.0], None);
                    } else {
                        let tx = hotbar_center_x + 91.0 + seg_gap;
                        let _ = event.draw.sprite(
                            "gui/sprites/hud/hotbar_offhand_right",
                            [0.0, 0.0, 29.0, 24.0],
                            [tx, ty, 29.0, 24.0],
                            None,
                        );
                        event
                            .draw
                            .gui_draw_item_with(tool_stack, [tx + 10.0, ty + 4.0], None);
                    }
                }
            }
        }
    }

    fn screen_opening(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenOpeningEvent<'_>,
    ) -> Option<ScreenOpening> {
        let is_creative = host
            .local_player()
            .and_then(|p| p.is_creative(host))
            .unwrap_or_else(|| {
                screen_controller::client_snapshot()
                    .lock()
                    .unwrap()
                    .is_creative
            });
        if event.new.id == "inventory"
            && event.new.menu.is_none()
            && host.mod_screen() != Some(screen_controller::MOD_SCREEN_ID)
            && !is_creative
        {
            queue_screen_action(screen_controller::ScreenAction::Open);
            screen_controller::reset_unlocked_recipes();
            return Some(ScreenOpening::ModScreen);
        }
        None
    }

    fn screen_render_foreground(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenRenderForegroundEvent<'_>,
    ) {
        if host.mod_screen() != Some(screen_controller::MOD_SCREEN_ID) {
            let [sw, sh] = event.draw.size();
            set_last_gui_size(sw, sh);
            screen_controller::draw_container_extension_foreground(host, event);
        }
    }

    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        if host.mod_screen() == Some(screen_controller::MOD_SCREEN_ID) {
            let [sw, sh] = event.draw.size();
            set_last_gui_size(sw, sh);
            let dark_mode = effective_dark_mode();
            screen_controller::render_unified_screen(
                host,
                event.draw,
                event.screen.mouse,
                dark_mode,
            );
        } else {
            let [sw, sh] = event.draw.size();
            set_last_gui_size(sw, sh);
            screen_controller::draw_container_extension_post(host, event);
        }
    }

    fn screen_mouse_button_pressed_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        if host.mod_screen() != Some(screen_controller::MOD_SCREEN_ID) {
            if screen_controller::handle_container_extension_click(host, event) {
                return Flow::Cancel;
            }
            return Flow::Continue;
        }
        let [sw, sh] = get_last_gui_size();
        let snap = screen_controller::client_snapshot().lock().unwrap().clone();
        let layout = screen_controller::active_layout_for_snapshot(&snap);
        let book_open = snap.mode == screen_controller::ScreenMode::Inventory
            && screen_controller::is_recipe_book_open();
        let origin = layout.origin_with_book(sw, sh, book_open);
        let [mx, my] = event.screen.mouse;
        let shift = (event.modifiers & 0x0001) != 0;
        if let Some(maybe_action) = screen_controller::handle_recipe_book_click(
            host,
            &snap,
            origin,
            &layout,
            [mx, my],
            shift,
        ) {
            if let Some(action) = maybe_action {
                queue_screen_action(action);
            }
            return Flow::Cancel;
        }
        let hit_slot = if snap.mode == screen_controller::ScreenMode::Inventory
            && screen_controller::elytra_slot_hit_test(origin, mx, my)
        {
            Some(ui::inventory_screen::UnifiedSlotId::Elytra)
        } else {
            layout.hit_test(origin, mx, my)
        };
        if let Some(slot) = hit_slot {
            let is_double = check_double_click(slot);
            if event.button == 0 && shift && is_double {
                queue_screen_action(screen_controller::ScreenAction::ShiftDoubleClick { slot });
            } else if event.button == 0 && !shift && is_double {
                queue_screen_action(screen_controller::ScreenAction::DoubleClick { slot });
            } else if !shift
                && snap.cursor.is_some()
                && slot != ui::inventory_screen::UnifiedSlotId::CraftOutput
                && (event.button == 0 || event.button == 1)
            {
                *screen_controller::drag_state_lock().lock().unwrap() =
                    Some(screen_controller::DragState {
                        button: event.button as u8,
                        slots: vec![slot],
                    });
            } else if event.button == 1 {
                queue_screen_action(screen_controller::ScreenAction::RightClick { slot });
            } else if event.button == 0 {
                queue_screen_action(screen_controller::ScreenAction::LeftClick { slot, shift });
            }
        }
        Flow::Cancel
    }

    fn screen_mouse_button_released_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenMouseButtonReleasedPreEvent<'_>,
    ) -> Flow {
        if host.mod_screen() != Some(screen_controller::MOD_SCREEN_ID) {
            if let Some(d) = screen_controller::drag_state_lock().lock().unwrap().take() {
                if d.slots.len() > 1 {
                    queue_screen_action(screen_controller::ScreenAction::DragDistribute {
                        button: d.button,
                        slots: d.slots,
                    });
                } else if let Some(&slot) = d.slots.first() {
                    if d.button == 1 {
                        queue_screen_action(screen_controller::ScreenAction::RightClick { slot });
                    } else {
                        queue_screen_action(screen_controller::ScreenAction::LeftClick {
                            slot,
                            shift: false,
                        });
                    }
                }
                return Flow::Cancel;
            }
            let [mx, my] = event.screen.mouse;
            if screen_controller::is_over_container_extension(
                host,
                &event.screen.id,
                event.screen.slots,
                event.screen.menu.is_some(),
                event.screen.rect,
                mx,
                my,
            ) {
                return Flow::Cancel;
            }
            return Flow::Continue;
        }
        let drag = screen_controller::drag_state_lock().lock().unwrap().take();
        if let Some(d) = drag {
            if d.slots.len() > 1 {
                queue_screen_action(screen_controller::ScreenAction::DragDistribute {
                    button: d.button,
                    slots: d.slots,
                });
            } else if let Some(&slot) = d.slots.first() {
                if d.button == 1 {
                    queue_screen_action(screen_controller::ScreenAction::RightClick { slot });
                } else {
                    queue_screen_action(screen_controller::ScreenAction::LeftClick {
                        slot,
                        shift: false,
                    });
                }
            }
        }
        Flow::Cancel
    }

    fn screen_char_typed_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenCharTypedPreEvent<'_>,
    ) -> Flow {
        if host.mod_screen() == Some(screen_controller::MOD_SCREEN_ID)
            && screen_controller::is_recipe_search_focused()
        {
            screen_controller::on_recipe_book_char(event.c);
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn screen_key_pressed_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenKeyPressedPreEvent<'_>,
    ) -> Flow {
        if host.mod_screen() != Some(screen_controller::MOD_SCREEN_ID) {
            return Flow::Continue;
        }
        if screen_controller::is_recipe_search_focused() {
            if event.key == "key.keyboard.escape" {
                queue_screen_action(screen_controller::ScreenAction::Close);
                CLOSE_MOD_SCREEN.store(true, std::sync::atomic::Ordering::Relaxed);
                return Flow::Cancel;
            }
            if event.key == "key.keyboard.backspace" {
                screen_controller::on_recipe_book_backspace();
            }
            return Flow::Cancel;
        }
        if event.key == "key.keyboard.e" || event.key == "key.keyboard.escape" {
            queue_screen_action(screen_controller::ScreenAction::Close);
            CLOSE_MOD_SCREEN.store(true, std::sync::atomic::Ordering::Relaxed);
            return Flow::Cancel;
        }
        let [sw, sh] = get_last_gui_size();
        let snap = screen_controller::client_snapshot().lock().unwrap().clone();
        let layout = screen_controller::active_layout_for_snapshot(&snap);
        let book_open = snap.mode == screen_controller::ScreenMode::Inventory
            && screen_controller::is_recipe_book_open();
        let origin = layout.origin_with_book(sw, sh, book_open);
        let [mx, my] = event.screen.mouse;
        let hovered = layout.hit_test(origin, mx, my);

        if let Some(digit) = event.key.strip_prefix("key.keyboard.") {
            if let Ok(num) = digit.parse::<u8>() {
                if (1..=9).contains(&num) {
                    if let Some(slot) = hovered {
                        queue_screen_action(screen_controller::ScreenAction::NumberKey {
                            slot,
                            hotbar: num - 1,
                        });
                    }
                    return Flow::Cancel;
                }
            }
        }
        if event.key == "key.keyboard.q" {
            let whole_stack = (event.modifiers & 0x0002) != 0;
            queue_screen_action(screen_controller::ScreenAction::Drop {
                slot: hovered,
                whole_stack,
            });
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn right_click_block(
        &self,
        host: HostSide,
        event: &mut RightClickBlockEvent<'_>,
    ) -> Flow {
        let HostSide::Server(server_host) = host else {
            return Flow::Continue;
        };
        let ender_rows = get_state().lock().unwrap().config.ender_chest_rows;
        if ender_rows == 3 {
            return Flow::Continue;
        }
        let Some(player) = server_host.player(event.click.player) else {
            return Flow::Continue;
        };
        let Some(level) = player.level(server_host) else {
            return Flow::Continue;
        };
        let Some(state) = level.block_state(server_host, event.click.pos) else {
            return Flow::Continue;
        };
        if let Some(name) = server_host.block_state_string(state) {
            if name.starts_with("minecraft:ender_chest") {
                screen_controller::open_ender_chest_screen(server_host, player);
                return Flow::Cancel;
            }
        }
        Flow::Continue
    }

    fn use_item_on_block(
        &self,
        host: HostSide,
        event: &mut UseItemOnBlockEvent<'_>,
    ) -> Flow {
        let HostSide::Server(server_host) = host else {
            return Flow::Continue;
        };
        let Some(player) = server_host.player(event.click.player) else {
            return Flow::Continue;
        };
        let Some(level) = player.level(server_host) else {
            return Flow::Continue;
        };
        let Some(state) = level.block_state(server_host, event.click.pos) else {
            return Flow::Continue;
        };
        let Some(name) = server_host.block_state_string(state) else {
            return Flow::Continue;
        };

        let (ender_rows, empty_hand_till_and_path) = {
            let st = get_state().lock().unwrap();
            (st.config.ender_chest_rows, st.config.empty_hand_till_and_path)
        };

        if ender_rows != 3 && name.starts_with("minecraft:ender_chest") {
            screen_controller::open_ender_chest_screen(server_host, player);
            return Flow::Cancel;
        }

        let offhand_empty = player
            .inventory_slots(server_host)
            .is_none_or(|slots| !slots.iter().any(|(s, _)| *s == 40));

        if empty_hand_till_and_path
            && event.click.hand == mod_api::Hand::Main
            && event.click.stack.is_none()
            && offhand_empty
        {
            let bare = name.split('[').next().unwrap_or(&name);
            let is_tillable = matches!(
                bare,
                "minecraft:dirt"
                    | "minecraft:grass_block"
                    | "minecraft:dirt_path"
                    | "minecraft:coarse_dirt"
                    | "minecraft:rooted_dirt"
            );
            if is_tillable {
                let crouching = player.entity().is_crouching(server_host).unwrap_or(false);
                let uuid = screen_controller::player_key(player);
                if crouching {
                    let has_shovel = screen_controller::server_slots_map()
                        .lock()
                        .unwrap()
                        .get(&uuid)
                        .is_some_and(|s| s.toolbelt[ToolType::Shovel.slot_index()].is_some());
                    if has_shovel && bare != "minecraft:dirt_path" {
                        if let Some(target) = server_host.block_state_id("minecraft:dirt_path") {
                            let _ = level.set_block(
                                server_host,
                                event.click.pos,
                                target,
                                mod_api::BlockUpdateFlags::default(),
                            );
                            damage_toolbelt_item(server_host, player, ToolType::Shovel.slot_index());
                            if let Some(living) = server_host.living(player.entity()) {
                                let _ = living.swing(server_host, mod_api::Hand::Main, true);
                            }
                            return Flow::Cancel;
                        }
                    }
                } else {
                    let has_hoe = screen_controller::server_slots_map()
                        .lock()
                        .unwrap()
                        .get(&uuid)
                        .is_some_and(|s| s.toolbelt[ToolType::Hoe.slot_index()].is_some());
                    if has_hoe {
                        let target_id = if bare == "minecraft:coarse_dirt" {
                            "minecraft:dirt"
                        } else {
                            "minecraft:farmland"
                        };
                        if let Some(target) = server_host.block_state_id(target_id) {
                            let _ = level.set_block(
                                server_host,
                                event.click.pos,
                                target,
                                mod_api::BlockUpdateFlags::default(),
                            );
                            damage_toolbelt_item(server_host, player, ToolType::Hoe.slot_index());
                            if let Some(living) = server_host.living(player.entity()) {
                                let _ = living.swing(server_host, mod_api::Hand::Main, true);
                            }
                            return Flow::Cancel;
                        }
                    }
                }
            }
        }
        Flow::Continue
    }

    fn right_click_item(&self, host: HostSide<'_>, event: &mut RightClickItemEvent<'_>) -> Flow {
        let can_throw_unloyal = get_state()
            .lock()
            .unwrap()
            .config
            .can_throw_unloyal_trident;
        if !can_throw_unloyal {
            let h: &dyn Host = match host {
                HostSide::Server(s) => s,
                HostSide::Client(c) => c,
            };
            if event.click.stack.item().id(h) == "minecraft:trident" {
                let model = from_api_stack(h, event.click.stack);
                let has_loyalty = model.enchantment_level("minecraft:loyalty") > 0;
                let has_riptide = model.enchantment_level("minecraft:riptide") > 0;
                if !has_loyalty && !has_riptide {
                    return Flow::Cancel;
                }
            }
        }
        Flow::Continue
    }

    fn server_tick(&self, host: &mut dyn ServerHost, _event: &mut ServerTickEvent<'_>) {
        let map = FIREWORK_BOOST_TICKS.get_or_init(|| Mutex::new(HashMap::new()));
        let active: Vec<(u128, u32)> = {
            let mut guard = map.lock().unwrap();
            guard.retain(|_, t| *t > 0);
            guard.iter().map(|(&k, &v)| (k, v)).collect()
        };
        for (player_uuid, _) in active {
            let Some(player) = host.player(player_uuid) else {
                map.lock().unwrap().remove(&player_uuid);
                continue;
            };
            let entity = player.entity();
            let Some(living) = host.living(entity) else {
                continue;
            };
            if !living.is_gliding(host).unwrap_or(false) {
                map.lock().unwrap().remove(&player_uuid);
                continue;
            }
            if let (Some(rot), Some(vel)) = (entity.rotation(host), entity.velocity(host)) {
                let yaw = (rot[0] as f64).to_radians();
                let pitch = (rot[1] as f64).to_radians();
                let cos_p = pitch.cos();
                let lx = -yaw.sin() * cos_p;
                let ly = -pitch.sin();
                let lz = yaw.cos() * cos_p;
                let nv = [
                    vel[0] + lx * 0.1 + (lx * 1.5 - vel[0]) * 0.5,
                    vel[1] + ly * 0.1 + (ly * 1.5 - vel[1]) * 0.5,
                    vel[2] + lz * 0.1 + (lz * 1.5 - vel[2]) * 0.5,
                ];
                let _ = entity.set_velocity(host, nv);
            }
            let mut guard = map.lock().unwrap();
            if let Some(t) = guard.get_mut(&player_uuid) {
                if *t <= 1 {
                    guard.remove(&player_uuid);
                } else {
                    *t -= 1;
                }
            }
        }
    }

    fn player_xp_change(
        &self,
        host: &mut dyn ServerHost,
        event: &mut PlayerXpChangeEvent<'_>,
    ) -> Flow {
        if event.amount <= 0 {
            return Flow::Continue;
        }
        let Some(player) = host.player(event.player) else {
            return Flow::Continue;
        };
        let uuid = screen_controller::player_key(player);
        let mut remaining_xp = event.amount;
        let mut repaired_any = false;
        {
            let mut map = screen_controller::server_slots_map().lock().unwrap();
            if let Some(entry) = map.get_mut(&uuid) {
                for slot_opt in entry.toolbelt.iter_mut() {
                    if remaining_xp <= 0 {
                        break;
                    }
                    if let Some(st) = slot_opt {
                        let dmg = st.damage();
                        if dmg > 0 && from_api_stack(host, st).enchantment_level("minecraft:mending") > 0 {
                            let heal = (remaining_xp as u32 * 2).min(dmg);
                            if heal > 0 {
                                let _ = st.set_damage(dmg - heal);
                                let used_xp = ((heal + 1) / 2) as i32;
                                remaining_xp = (remaining_xp - used_xp).max(0);
                                repaired_any = true;
                            }
                        }
                    }
                }
                if repaired_any {
                    screen_controller::sync_server_slots_to_model(host, &uuid, entry);
                }
            }
        }
        if repaired_any {
            screen_controller::send_full_sync(host, player);
            if remaining_xp <= 0 {
                return Flow::Cancel;
            }
            event.amount = remaining_xp;
        }
        Flow::Continue
    }

    fn living_get_projectile(
        &self,
        host: HostSide<'_>,
        event: &mut LivingGetProjectileEvent<'_>,
    ) {
        if !get_state().lock().unwrap().config.infinity_requires_no_arrows {
            return;
        }
        if event.projectile.is_some() {
            return;
        }
        let h: &dyn Host = match host {
            HostSide::Server(s) => s,
            HostSide::Client(c) => c,
        };
        if event.weapon.item().id(h) == "minecraft:bow"
            && from_api_stack(h, event.weapon).enchantment_level("minecraft:infinity") > 0
        {
            if let Ok(ident) = Identifier::parse("minecraft:arrow") {
                if let Some(arrow_key) = h.item_key(&ident) {
                    *event.projectile = mod_api::ItemStack::new(arrow_key, 1);
                }
            }
        }
    }

    fn attack_entity(&self, host: &mut dyn ServerHost, event: &mut AttackEntityEvent<'_>) -> Flow {
        if !get_state().lock().unwrap().config.auto_switch_enabled {
            return Flow::Continue;
        }
        let players = host.players().server_players();
        let Some(player) = players.into_iter().next() else {
            return Flow::Continue;
        };
        if player_held_is_tool(host, player) {
            return Flow::Continue;
        }
        let uuid = screen_controller::player_key(player);
        let weapon = {
            let map = screen_controller::server_slots_map().lock().unwrap();
            map.get(&uuid).and_then(|e| {
                e.toolbelt[ToolType::Sword.slot_index()]
                    .as_ref()
                    .map(|s| (ToolType::Sword.slot_index(), s.clone()))
                    .or_else(|| {
                        e.toolbelt[ToolType::Axe.slot_index()]
                            .as_ref()
                            .map(|s| (ToolType::Axe.slot_index(), s.clone()))
                    })
            })
        };
        if let Some((idx, stack)) = weapon {
            TOOL_HUD_TICKS.store(20, std::sync::atomic::Ordering::Relaxed);
            ACTIVE_PREVIEW_TOOL_IDX.store(idx + 1, std::sync::atomic::Ordering::Relaxed);
            let bonus = weapon_bonus_damage(host, &stack);
            if bonus > 0.0 {
                if let Some(level) = player.level(host) {
                    let min = [event.pos[0] - 0.5, event.pos[1] - 0.5, event.pos[2] - 0.5];
                    let max = [event.pos[0] + 0.5, event.pos[1] + 0.5, event.pos[2] + 0.5];
                    if let Some(targets) = level.entities_in(host, min, max) {
                        for target in targets {
                            if target != player.entity() {
                                let _ = target.hurt(
                                    host,
                                    "minecraft:player_attack",
                                    bonus,
                                    Some(player.entity()),
                                    Some(player.entity()),
                                );
                                damage_toolbelt_item(host, player, idx);
                                break;
                            }
                        }
                    }
                }
            }
        }
        Flow::Continue
    }

    fn player_harvest_check(
        &self,
        host: HostSide<'_>,
        event: &mut PlayerHarvestCheckEvent,
    ) {
        if event.can_harvest || !get_state().lock().unwrap().config.auto_switch_enabled {
            return;
        }
        match host {
            HostSide::Server(server_host) => {
                let players = server_host.players().server_players();
                let Some(player) = players.into_iter().next() else {
                    return;
                };
                if player_held_is_tool(server_host, player) {
                    return;
                }
                let Some(level) = player.level(server_host) else {
                    return;
                };
                let Some(state) = level.block_state(server_host, event.pos) else {
                    return;
                };
                if let Some((_idx, tool, _)) =
                    best_toolbelt_tool_for_block(server_host, player, state)
                {
                    if server_host
                        .block_states()
                        .is_correct_tool_for_drops(state, &tool)
                        == Some(true)
                    {
                        event.can_harvest = true;
                    }
                }
            }
            HostSide::Client(client_host) => {
                let snap = screen_controller::client_snapshot().lock().unwrap().clone();
                let held_is_tool = snap
                    .main
                    .get(snap.selected_slot)
                    .and_then(Option::as_ref)
                    .is_some_and(|s| {
                        ItemStack::new(s.item().id(client_host), 1)
                            .tool_type()
                            .is_some()
                    });
                if held_is_tool {
                    return;
                }
                let Some(level) = client_host
                    .camera_entity()
                    .and_then(|e| e.level(client_host))
                else {
                    return;
                };
                let Some(state) = level.block_state(client_host, event.pos) else {
                    return;
                };
                if let Some((_idx, tool, _)) =
                    best_toolbelt_tool_for_block_slice(client_host, &snap.toolbelt, state)
                {
                    if client_host
                        .block_states()
                        .is_correct_tool_for_drops(state, &tool)
                        == Some(true)
                    {
                        event.can_harvest = true;
                    }
                }
            }
        }
    }

    fn player_break_speed(
        &self,
        host: HostSide<'_>,
        event: &mut PlayerBreakSpeedEvent,
    ) -> Flow {
        if !get_state().lock().unwrap().config.auto_switch_enabled {
            return Flow::Continue;
        }
        match host {
            HostSide::Server(server_host) => {
                let players = server_host.players().server_players();
                let Some(player) = players.into_iter().next() else {
                    return Flow::Continue;
                };
                if player_held_is_tool(server_host, player) {
                    return Flow::Continue;
                }
                let Some(level) = player.level(server_host) else {
                    return Flow::Continue;
                };
                let Some(state) = level.block_state(server_host, event.pos) else {
                    return Flow::Continue;
                };
                if let Some((idx, _tool, speed)) =
                    best_toolbelt_tool_for_block(server_host, player, state)
                {
                    TOOL_HUD_TICKS.store(20, std::sync::atomic::Ordering::Relaxed);
                    ACTIVE_PREVIEW_TOOL_IDX
                        .store(idx + 1, std::sync::atomic::Ordering::Relaxed);
                    event.new_speed = event.original_speed.max(speed);
                }
            }
            HostSide::Client(client_host) => {
                let snap = screen_controller::client_snapshot().lock().unwrap().clone();
                let held_is_tool = snap
                    .main
                    .get(snap.selected_slot)
                    .and_then(Option::as_ref)
                    .is_some_and(|s| {
                        ItemStack::new(s.item().id(client_host), 1)
                            .tool_type()
                            .is_some()
                    });
                if held_is_tool {
                    return Flow::Continue;
                }
                let Some(level) = client_host
                    .camera_entity()
                    .and_then(|e| e.level(client_host))
                else {
                    return Flow::Continue;
                };
                let Some(state) = level.block_state(client_host, event.pos) else {
                    return Flow::Continue;
                };
                if let Some((idx, _tool, speed)) =
                    best_toolbelt_tool_for_block_slice(client_host, &snap.toolbelt, state)
                {
                    TOOL_HUD_TICKS.store(20, std::sync::atomic::Ordering::Relaxed);
                    ACTIVE_PREVIEW_TOOL_IDX
                        .store(idx + 1, std::sync::atomic::Ordering::Relaxed);
                    event.new_speed = event.original_speed.max(speed);
                }
            }
        }
        Flow::Continue
    }

    fn item_picked_up(&self, host: &mut dyn ServerHost, event: &mut ItemPickedUpEvent<'_>) {
        if *event.can_pickup == TriState::False {
            return;
        }
        if *event.can_pickup == TriState::Default
            && event.item.pickup_delay(host).unwrap_or(1) > 0
        {
            return;
        }
        let Some(player) = host.player(event.player) else {
            return;
        };
        let Some(stack) = event.item.stack(host) else {
            return;
        };
        let max_stack = host.max_stack_size(stack.item()).unwrap_or(64);
        let vslots: HashMap<u32, mod_api::ItemStack> = player
            .inventory_slots(host)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut vanilla_room = 0u32;
        for i in 0..36u32 {
            match vslots.get(&i) {
                None => vanilla_room = vanilla_room.saturating_add(max_stack),
                Some(existing)
                    if existing.same_item(&stack) && existing.same_components(&stack) =>
                {
                    vanilla_room =
                        vanilla_room.saturating_add(max_stack.saturating_sub(existing.count()));
                }
                _ => {}
            }
            if vanilla_room >= stack.count() {
                return;
            }
        }
        let (inserted, remaining) = screen_controller::try_pickup_into_extra_slots(host, player, stack);
        if inserted > 0 {
            match remaining {
                None => {
                    let _ = event.item.entity().discard(host);
                    *event.can_pickup = TriState::False;
                }
                Some(rem) => {
                    let _ = event.item.set_stack(host, rem);
                }
            }
            screen_controller::send_full_sync(host, player);
        }
    }

    fn break_block(&self, host: &mut dyn ServerHost, event: &mut BreakBlockEvent<'_>) -> Flow {
        if !get_state().lock().unwrap().config.auto_switch_enabled {
            return Flow::Continue;
        }
        let players = host.players().server_players();
        let Some(player) = players.into_iter().next() else {
            return Flow::Continue;
        };
        if player_held_is_tool(host, player) {
            return Flow::Continue;
        }
        if let Some((idx, tool, _)) = best_toolbelt_tool_for_block(host, player, event.state) {
            TOOL_HUD_TICKS.store(20, std::sync::atomic::Ordering::Relaxed);
            ACTIVE_PREVIEW_TOOL_IDX.store(idx + 1, std::sync::atomic::Ordering::Relaxed);
            let req = host
                .block_states()
                .requires_correct_tool(event.state)
                .unwrap_or(false);
            if req
                && host
                    .block_states()
                    .is_correct_tool_for_drops(event.state, &tool)
                    == Some(true)
            {
                if let Some(level) = player.level(host) {
                    let _ = level.destroy_block(host, event.pos, true, Some(&tool), false);
                    damage_toolbelt_item(host, player, idx);
                    return Flow::Cancel;
                }
            } else {
                damage_toolbelt_item(host, player, idx);
            }
        }
        Flow::Continue
    }
}

/// Called per player tick to update enchantment levels and synchronize inventory state.
pub fn on_player_tick(
    player_uuid: &str,
    equipped_armor: &[Option<ItemStack>],
    vanilla_inventory: &mut [Option<ItemStack>],
    dropped_items_sink: &mut Vec<ItemStack>,
) {
    let mut state = get_state().lock().unwrap();
    let config = state.config.clone();

    // 1. Calculate active enchantment levels
    let deep_pockets_lvl = DeepPocketsEnchantment::calculate_level(equipped_armor, &config);
    let big_hands_lvl = BigHandsEnchantment::calculate_level(equipped_armor, &config);

    // 2. Update capacities
    let inv = state.get_or_create_inventory(player_uuid);
    let overflow = inv.update_capacities(deep_pockets_lvl, big_hands_lvl, &config);

    // 3. Safe item evacuation if capacity was reduced
    if !overflow.is_empty() {
        let unplaced = ExtraPocketsManager::evacuate_overflow(overflow, vanilla_inventory);
        for item in unplaced {
            dropped_items_sink.push(item);
        }
    }
}

/// Called when player starts an interaction/action (attacking, breaking a block, tilling).
///
/// Priority rule: If the player is already holding a matching tool in their hotbar,
/// it takes priority over the toolbelt.
/// If not holding a matching tool, activates the matching tool from dedicated slots
/// into a temporary slot visible on the right-hand side of the hotbar (without mutating the hotbar).
pub fn on_action_start(
    player_uuid: &str,
    action: PlayerAction,
    held_item: &ItemStack,
) -> Option<ItemStack> {
    let mut state = get_state().lock().unwrap();
    if !state.config.auto_switch_enabled {
        return None;
    }

    let inv = state.get_or_create_inventory(player_uuid).clone();
    let switcher = state.get_or_create_switcher(player_uuid);
    switcher.on_action_start(action, &inv, held_item)
}

/// Called when player action terminates (block broken, attack completed).
/// Hides the temporary right-hand tool slot.
pub fn on_action_end(player_uuid: &str) -> bool {
    let mut state = get_state().lock().unwrap();
    if !state.config.auto_switch_restore_on_finish {
        return false;
    }

    let switcher = state.get_or_create_switcher(player_uuid);
    switcher.on_action_end()
}

/// Called on mouse wheel scroll.
/// If Ctrl is pressed, intercepts the scroll to cycle offhand slots with wrap-around.
pub fn on_mouse_scroll(player_uuid: &str, x_offset: f64, y_offset: f64) -> Option<usize> {
    let mut state = get_state().lock().unwrap();
    if let Some(delta) = state.input_manager.on_mouse_scroll(x_offset, y_offset) {
        let inv = state.get_or_create_inventory(player_uuid);
        let new_idx = inv.cycle_offhand(delta);
        Some(new_idx)
    } else {
        None
    }
}

/// Called on keyboard key events to track modifier keys (Ctrl) or discrete cycling keys.
pub fn on_key_event(player_uuid: &str, key: i32, action: KeyAction) -> Option<usize> {
    let mut state = get_state().lock().unwrap();
    if let Some(delta) = state.input_manager.on_key_event(key, action) {
        let inv = state.get_or_create_inventory(player_uuid);
        let new_idx = inv.cycle_offhand(delta);
        Some(new_idx)
    } else {
        None
    }
}

/// Called when rendering the HUD hotbar to retrieve offhand preview displays.
pub fn get_hotbar_offhand_hud(
    player_uuid: &str,
    center_x: f32,
    center_y: f32,
    gui_scale: f32,
) -> Vec<HotbarOffhandSlotDisplay> {
    let mut state = get_state().lock().unwrap();
    let inv = state.get_or_create_inventory(player_uuid);
    HotbarOverlayRenderer::compute_offhand_display(center_x, center_y, gui_scale, inv)
}

/// Called when rendering the HUD hotbar to retrieve the temporary active tool slot on the right-hand side.
/// Returns Some(HotbarToolSlotDisplay) if a tool is currently active, None when hidden.
/// Dynamically selects dark mode textures when a Dark Mode resource pack is active.
pub fn get_hotbar_tool_slot_hud(
    player_uuid: &str,
    hotbar_center_x: f32,
    hotbar_center_y: f32,
    gui_scale: f32,
) -> Option<HotbarToolSlotDisplay> {
    let dark_mode = effective_dark_mode();
    let mut state = get_state().lock().unwrap();
    let switcher = state.get_or_create_switcher(player_uuid);
    HotbarOverlayRenderer::compute_tool_slot_display(
        hotbar_center_x,
        hotbar_center_y,
        gui_scale,
        switcher,
        dark_mode,
    )
}

/// Called when player receives lethal damage.
/// If totem_any_offhand_slot is enabled, searches all offhand slots for a Totem of Undying,
/// consumes one, and returns true to prevent death and resurrect the player.
pub fn on_player_lethal_damage(player_uuid: &str) -> bool {
    let mut state = get_state().lock().unwrap();
    if !state.config.totem_any_offhand_slot {
        return false;
    }
    let inv = state.get_or_create_inventory(player_uuid);
    inv.consume_totem_of_undying()
}

/// Helper to set Dark Mode GUI state.
pub fn set_dark_mode_active(active: bool) {
    let mut state = get_state().lock().unwrap();
    state.config.dark_mode_setting = if active {
        config::DarkModeSetting::On
    } else {
        config::DarkModeSetting::Off
    };
}

/// Called when opening or resizing the inventory screen to compute layout and dynamic scale.
pub fn get_inventory_layout(
    player_uuid: &str,
    window_width: f32,
    window_height: f32,
    gui_scale: f32,
) -> (InventoryLayoutMetrics, Vec<CustomSlotView>) {
    let mut state = get_state().lock().unwrap();
    let config = state.config.clone();
    let inv = state.get_or_create_inventory(player_uuid).clone();
    drop(state);
    let dark_mode = effective_dark_mode();

    let extra_rows = inv.extra_rows.len() / 9;
    let metrics = ScreenScaler::compute_layout(
        window_width,
        window_height,
        gui_scale,
        extra_rows,
        config.tool_slots_count,
        inv.offhand_slots.len(),
        &config,
    );

    let slots = InventoryScreenLayout::build_slots(&metrics, &inv, dark_mode);
    (metrics, slots)
}

/// Called on mouse click within the inventory screen.
pub fn on_inventory_mouse_click(
    player_uuid: &str,
    metrics: &InventoryLayoutMetrics,
    mouse_x: f32,
    mouse_y: f32,
    click_type: ClickType,
    vanilla_inventory: &mut [Option<ItemStack>],
    cursor_item: &mut Option<ItemStack>,
) -> bool {
    let mut state = get_state().lock().unwrap();
    let inv = state.get_or_create_inventory(player_uuid);

    if let Some((target, slot_index)) = InventoryScreenLayout::hit_test(metrics, inv, mouse_x, mouse_y) {
        let packet = ModPacket::SlotClick {
            slot_type: target,
            slot_index,
            click_type,
        };
        PacketHandler::handle_packet_server(packet, inv, vanilla_inventory, cursor_item);
        true
    } else {
        false
    }
}

/// Called on player death: drops items if keepInventory is false.
pub fn on_player_death(
    player_uuid: &str,
    keep_inventory: bool,
    dropped_items_sink: &mut Vec<ItemStack>,
) {
    if !keep_inventory {
        let mut state = get_state().lock().unwrap();
        let inv = state.get_or_create_inventory(player_uuid);
        let items = inv.drain_all_items();
        dropped_items_sink.extend(items);
    }
}

/// Called on player respawn: re-initializes custom slots.
pub fn on_player_respawn(player_uuid: &str, equipped_armor: &[Option<ItemStack>]) {
    let mut state = get_state().lock().unwrap();
    let config = state.config.clone();
    let deep_pockets_lvl = DeepPocketsEnchantment::calculate_level(equipped_armor, &config);
    let big_hands_lvl = BigHandsEnchantment::calculate_level(equipped_armor, &config);

    let inv = state.get_or_create_inventory(player_uuid);
    let _ = inv.update_capacities(deep_pockets_lvl, big_hands_lvl, &config);
}

/// Called when world is saved.
pub fn on_world_save(world_dir: &std::path::Path) -> Result<(), std::io::Error> {
    let state = get_state().lock().unwrap();
    let save_mgr = SaveManager::new(world_dir);
    for (uuid, inv) in &state.inventories {
        save_mgr.save_player_data(uuid, inv)?;
    }
    Ok(())
}

/// Called when player logs into world.
pub fn on_player_join(world_dir: &std::path::Path, player_uuid: &str) {
    let mut state = get_state().lock().unwrap();
    let save_mgr = SaveManager::new(world_dir);
    let loaded_inv = save_mgr.load_player_data(player_uuid);
    state.inventories.insert(player_uuid.to_string(), loaded_inv);
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToolType;

    #[test]
    fn test_tool_slot_validation() {
        let sword = ItemStack::new("minecraft:diamond_sword", 1);
        let pickaxe = ItemStack::new("minecraft:iron_pickaxe", 1);
        let dirt = ItemStack::new("minecraft:dirt", 64);

        assert!(ModInventoryData::can_place_in_tool_slot(0, &sword)); // Sword slot
        assert!(!ModInventoryData::can_place_in_tool_slot(0, &pickaxe));
        assert!(!ModInventoryData::can_place_in_tool_slot(0, &dirt));

        assert!(ModInventoryData::can_place_in_tool_slot(2, &pickaxe)); // Pickaxe slot
        assert!(!ModInventoryData::can_place_in_tool_slot(2, &sword));
    }

    #[test]
    fn test_offhand_cycling_wraparound() {
        let mut inv = ModInventoryData::new(4, 0, 6);
        assert_eq!(inv.active_offhand_index, 0);

        // Cycle next
        assert_eq!(inv.cycle_offhand(1), 1);
        assert_eq!(inv.cycle_offhand(1), 2);
        assert_eq!(inv.cycle_offhand(1), 3);
        // Wraparound to start
        assert_eq!(inv.cycle_offhand(1), 0);

        // Cycle prev with wraparound to end
        assert_eq!(inv.cycle_offhand(-1), 3);
    }

    #[test]
    fn test_auto_switch_priority_and_temporary_slot() {
        let player_uuid = "test-player-auto-switch";
        {
            let mut state = get_state().lock().unwrap();
            let inv = state.get_or_create_inventory(player_uuid);
            inv.tool_slots[ToolType::Pickaxe.slot_index()] =
                Some(ItemStack::new("minecraft:netherite_pickaxe", 1));
        }

        // Case 1: Player is already holding a pickaxe in hotbar -> HOTBAR PRIORITY!
        let held_pick = ItemStack::new("minecraft:iron_pickaxe", 1);
        let switched = on_action_start(player_uuid, PlayerAction::MiningStoneOrOre, &held_pick);
        // Does NOT switch because held tool takes priority!
        assert!(switched.is_none());
        assert!(get_hotbar_tool_slot_hud(player_uuid, 200.0, 300.0, 1.0).is_none());

        // Case 2: Player is holding an apple (not a tool) -> uses toolbelt tool in temporary slot!
        let held_apple = ItemStack::new("minecraft:apple", 5);
        let switched2 = on_action_start(player_uuid, PlayerAction::MiningStoneOrOre, &held_apple);
        assert!(switched2.is_some());
        assert_eq!(switched2.unwrap().id, "minecraft:netherite_pickaxe");

        // Temporary slot appears on the right-hand side of the hotbar
        let tool_hud = get_hotbar_tool_slot_hud(player_uuid, 200.0, 300.0, 1.0);
        assert!(tool_hud.is_some());
        let hud_display = tool_hud.unwrap();
        assert!(hud_display.visible);
        assert_eq!(hud_display.tool_item.id, "minecraft:netherite_pickaxe");
        // Right side of hotbar (center is 200, half width is 91, margin 7 -> x >= 298)
        assert!(hud_display.bounds.x >= 298.0);

        // Case 3: Action terminates -> temporary slot is hidden
        let ended = on_action_end(player_uuid);
        assert!(ended);
        assert!(get_hotbar_tool_slot_hud(player_uuid, 200.0, 300.0, 1.0).is_none());
    }

    #[test]
    fn test_enchantment_strict_armor_compatibility() {
        let config = InventorialConfig::default();

        let chestplate_with_big_hands =
            ItemStack::new("minecraft:diamond_chestplate", 1).with_enchantment("inventorial:big_hands", 2);
        let leggings_with_big_hands =
            ItemStack::new("minecraft:diamond_leggings", 1).with_enchantment("inventorial:big_hands", 2);

        let chestplate_with_deep_pockets =
            ItemStack::new("minecraft:diamond_chestplate", 1).with_enchantment("inventorial:deep_pockets", 3);
        let leggings_with_deep_pockets =
            ItemStack::new("minecraft:diamond_leggings", 1).with_enchantment("inventorial:deep_pockets", 3);

        // Big Hands: ONLY compatible with chestplate
        let armor_chest = vec![Some(chestplate_with_big_hands)];
        let armor_legs_wrong = vec![Some(leggings_with_big_hands)];
        assert_eq!(BigHandsEnchantment::calculate_level(&armor_chest, &config), 2);
        assert_eq!(BigHandsEnchantment::calculate_level(&armor_legs_wrong, &config), 0);

        // Deep Pockets: ONLY compatible with leggings
        let armor_legs = vec![Some(leggings_with_deep_pockets)];
        let armor_chest_wrong = vec![Some(chestplate_with_deep_pockets)];
        assert_eq!(DeepPocketsEnchantment::calculate_level(&armor_legs, &config), 3);
        assert_eq!(DeepPocketsEnchantment::calculate_level(&armor_chest_wrong, &config), 0);
    }

    #[test]
    fn test_totem_of_undying_in_any_offhand_slot() {
        let player_uuid = "test-player-totem";
        {
            let mut state = get_state().lock().unwrap();
            let inv = state.get_or_create_inventory(player_uuid);
            // Selected slot is 0 (holding empty or food)
            inv.active_offhand_index = 0;
            inv.offhand_slots[0] = Some(ItemStack::new("minecraft:bread", 16));

            // Totem is in offhand slot 1 (NOT currently selected)
            inv.offhand_slots[1] = Some(ItemStack::new("minecraft:totem_of_undying", 1));
        }

        // Lethal damage arrives!
        let saved = on_player_lethal_damage(player_uuid);
        assert!(saved, "Totem in non-active offhand slot must trigger to save the player!");

        // Verify totem was consumed from slot 1, while slot 0 still has bread
        let mut state = get_state().lock().unwrap();
        let inv = state.get_or_create_inventory(player_uuid);
        assert!(inv.offhand_slots[1].is_none());
        assert_eq!(inv.offhand_slots[0].as_ref().unwrap().id, "minecraft:bread");

        // Next lethal damage: no totems left -> returns false
        drop(state);
        let saved_again = on_player_lethal_damage(player_uuid);
        assert!(!saved_again, "No totems remaining, death occurs");
    }

    #[test]
    fn test_unified_layout_positions() {
        use crate::ui::inventory_screen::{compute_unified_layout, UnifiedSlotId};

        let base = compute_unified_layout(0, 4);
        let expanded = compute_unified_layout(2, 12);

        // Extra offhand columns (12 slots -> 3 cols vs 4 slots -> 2 cols) push crafting UI right
        assert!(expanded.craft_label_pos[0] > base.craft_label_pos[0]);

        // Extra rows (2 rows) push regular main inventory down
        let base_main_y = base
            .slots
            .iter()
            .find(|s| s.id == UnifiedSlotId::Main(9))
            .unwrap()
            .y;
        let exp_main_y = expanded
            .slots
            .iter()
            .find(|s| s.id == UnifiedSlotId::Main(9))
            .unwrap()
            .y;
        assert!(exp_main_y > base_main_y);

        // Toolbelt slots sit inside the main window bounds
        for s in expanded
            .slots
            .iter()
            .filter(|s| matches!(s.id, UnifiedSlotId::Toolbelt(_)))
        {
            assert!(s.x + 18.0 <= expanded.width);
            assert!(s.y + 18.0 <= expanded.height);
        }
    }

    #[test]
    fn test_ender_chest_capacity_config_and_evacuation() {
        let mut inv = ModInventoryData::default();
        let mut config = InventorialConfig::default();

        // Default: 6 rows = 54 slots
        assert_eq!(config.ender_chest_rows, 6);
        assert_eq!(inv.ender_chest.len(), 54);

        // Put an item in slot 50
        inv.ender_chest[50] = Some(ItemStack::new("minecraft:nether_star", 2));

        // Reconfigure to vanilla 3 rows (27 slots)
        config.ender_chest_rows = 3;
        let overflow = inv.update_capacities(0, 0, &config);
        assert_eq!(inv.ender_chest.len(), 27);
        // Item in slot 50 was safely evacuated into overflow
        assert_eq!(overflow.len(), 1);
        assert_eq!(overflow[0].id, "minecraft:nether_star");
        assert_eq!(overflow[0].count, 2);
    }

    #[test]
    fn test_dark_mode_resource_pack_compatibility() {
        let player_uuid = "test-player-dark-mode";

        // Light mode
        set_dark_mode_active(false);
        let (_, light_slots) = get_inventory_layout(player_uuid, 800.0, 600.0, 1.0);
        assert_eq!(
            light_slots[0].silhouette,
            Some("inventorial:textures/gui/slot_sword.png")
        );

        // Dark mode enabled (matching Dark Mode resource packs)
        set_dark_mode_active(true);
        let (_, dark_slots) = get_inventory_layout(player_uuid, 800.0, 600.0, 1.0);
        assert_eq!(
            dark_slots[0].silhouette,
            Some("inventorial:textures/gui/dark/slot_sword.png")
        );

        // Test temporary hotbar tool slot dark frame
        let held_apple = ItemStack::new("minecraft:apple", 1);
        {
            let mut state = get_state().lock().unwrap();
            let inv = state.get_or_create_inventory(player_uuid);
            inv.tool_slots[ToolType::Sword.slot_index()] =
                Some(ItemStack::new("minecraft:netherite_sword", 1));
        }
        on_action_start(player_uuid, PlayerAction::AttackingEntity, &held_apple);
        let tool_hud = get_hotbar_tool_slot_hud(player_uuid, 200.0, 300.0, 1.0).unwrap();
        assert_eq!(
            tool_hud.frame_texture,
            "inventorial:textures/gui/dark/slot_frame.png"
        );
        on_action_end(player_uuid);

        // Reset
        set_dark_mode_active(false);
    }

    #[test]
    fn test_deep_pockets_overflow_evacuation() {
        let mut inv = ModInventoryData::default();
        let config = InventorialConfig::default();

        // Level 2 -> 18 extra slots
        inv.update_capacities(2, 0, &config);
        assert_eq!(inv.extra_rows.len(), 18);

        // Put an item in slot 15
        inv.extra_rows[15] = Some(ItemStack::new("minecraft:diamond", 10));

        // Downgrade to level 1 -> 9 slots (slot 15 is removed)
        let overflow = inv.update_capacities(1, 0, &config);
        assert_eq!(inv.extra_rows.len(), 9);
        assert_eq!(overflow.len(), 1);
        assert_eq!(overflow[0].id, "minecraft:diamond");
        assert_eq!(overflow[0].count, 10);

        // Evacuate into vanilla inventory
        let mut vanilla = vec![None; 36];
        let unplaced = ExtraPocketsManager::evacuate_overflow(overflow, &mut vanilla);
        assert!(unplaced.is_empty());
        assert_eq!(vanilla[0].as_ref().unwrap().id, "minecraft:diamond");
        assert_eq!(vanilla[0].as_ref().unwrap().count, 10);
    }

    #[test]
    fn test_dynamic_scaling_no_clipping() {
        let config = InventorialConfig::default();
        // Very small window (e.g. 400x300) with 4 extra rows and 16 offhand slots
        let metrics = ScreenScaler::compute_layout(400.0, 300.0, 2.0, 4, 5, 16, &config);

        let scaled_w = metrics.total_width * metrics.effective_gui_scale;
        let scaled_h = metrics.total_height * metrics.effective_gui_scale;

        // Must fit inside window minus padding
        assert!(scaled_w <= 400.0);
        assert!(scaled_h <= 300.0);
    }

    #[test]
    fn test_death_drain_and_keep_inventory() {
        let player_uuid = "test-player-death";
        {
            let mut state = get_state().lock().unwrap();
            let inv = state.get_or_create_inventory(player_uuid);
            inv.tool_slots[0] = Some(ItemStack::new("minecraft:diamond_sword", 1));
            inv.offhand_slots[0] = Some(ItemStack::new("minecraft:shield", 1));
            inv.extra_rows = vec![Some(ItemStack::new("minecraft:gold_ingot", 64))];
        }

        let mut dropped = Vec::new();
        // keep_inventory = true -> nothing dropped
        on_player_death(player_uuid, true, &mut dropped);
        assert!(dropped.is_empty());

        // keep_inventory = false -> all dropped
        on_player_death(player_uuid, false, &mut dropped);
        assert_eq!(dropped.len(), 3);

        // Inventory is now empty
        let mut state = get_state().lock().unwrap();
        let inv = state.get_or_create_inventory(player_uuid);
        assert!(inv.tool_slots[0].is_none());
        assert!(inv.offhand_slots[0].is_none());
        assert!(inv.extra_rows[0].is_none());
    }

    #[test]
    fn test_save_and_load_persistence() {
        let temp_dir = std::env::temp_dir().join("inventorial_test_save");
        let save_mgr = SaveManager::new(&temp_dir);
        let player_uuid = "test-uuid-1234";

        let mut inv = ModInventoryData::new(4, 1, 6);
        inv.tool_slots[0] = Some(ItemStack::new("minecraft:netherite_sword", 1));
        inv.offhand_slots[1] = Some(ItemStack::new("minecraft:totem_of_undying", 1));
        inv.active_offhand_index = 1;

        assert!(save_mgr.save_player_data(player_uuid, &inv).is_ok());

        let loaded = save_mgr.load_player_data(player_uuid);
        assert_eq!(loaded.active_offhand_index, 1);
        assert_eq!(
            loaded.tool_slots[0].as_ref().unwrap().id,
            "minecraft:netherite_sword"
        );
        assert_eq!(
            loaded.offhand_slots[1].as_ref().unwrap().id,
            "minecraft:totem_of_undying"
        );

        // Loading nonexistent player falls back to default safely
        let non_existent = save_mgr.load_player_data("unknown-player");
        assert_eq!(non_existent.active_offhand_index, 0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_click_handling_and_tool_restriction() {
        let mut custom_inv = ModInventoryData::default();
        let mut vanilla_inv = vec![None; 36];
        let mut cursor = Some(ItemStack::new("minecraft:iron_sword", 1));

        // Click slot 0 (Sword) with a sword -> success
        let packet = ModPacket::SlotClick {
            slot_type: network::SlotTypeTarget::Tool,
            slot_index: 0,
            click_type: ClickType::Normal,
        };
        PacketHandler::handle_packet_server(
            packet,
            &mut custom_inv,
            &mut vanilla_inv,
            &mut cursor,
        );
        assert!(cursor.is_none());
        assert_eq!(
            custom_inv.tool_slots[0].as_ref().unwrap().id,
            "minecraft:iron_sword"
        );

        // Now cursor has a shovel, try placing into sword slot (0) -> rejected!
        cursor = Some(ItemStack::new("minecraft:diamond_shovel", 1));
        let packet = ModPacket::SlotClick {
            slot_type: network::SlotTypeTarget::Tool,
            slot_index: 0,
            click_type: ClickType::Normal,
        };
        PacketHandler::handle_packet_server(
            packet,
            &mut custom_inv,
            &mut vanilla_inv,
            &mut cursor,
        );
        // Rejected: cursor still holds shovel, slot still holds sword
        assert_eq!(cursor.as_ref().unwrap().id, "minecraft:diamond_shovel");
        assert_eq!(
            custom_inv.tool_slots[0].as_ref().unwrap().id,
            "minecraft:iron_sword"
        );
    }
}
