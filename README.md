# Inventorial

**Inventorial** is a port of the [Inventorio](https://modrinth.com/mod/inventorio) mod for the [Horizon](https://playhorizon.dev) Rust client. It extends the player's inventory with dedicated tool and elytra slots, cycling offhand slots, expandable inventory enchantments, and dynamic GUI scaling.

* Original **Inventorio** Mod: [GitHub (RubixDev/Inventorio)](https://github.com/RubixDev/Inventorio) · [Modrinth](https://modrinth.com/mod/inventorio)

---

## Features

* **Dedicated Toolbelt Slots** — Five specialized slots (Sword, Axe, Pickaxe, Shovel, Hoe) with non-destructive auto-switching and Mending XP support.
* **Dedicated Elytra Slot** — Extra utility slot on the inventory screen for an Elytra.
* **Cycling Offhand Slots & Multi-Slot Totem Support** — Expandable offhand grid (2 base slots, up to 8 with Big Hands), hotbar HUD previews, Swap Hands support, and Totems of Undying that trigger from any offhand slot.
* **Data-Driven Enchantments** — *Deep Pockets* (Leggings) and *Big Hands* (Chestplates) with zero-item-loss overflow evacuation.
* **Container GUI Extensions** — Deep Pockets slots remain accessible alongside chests, crafting tables, furnaces, and the Creative inventory.
* **Dynamic Inventory Screen Scaling** — Automatically scales the inventory UI to fit any resolution and GUI scale without clipping.
* **Configurable Ender Chest Capacity** — Expandable Ender Chest storage (default 6 rows / 54 slots).
* **Dark Mode Resource Pack Support** — Automatic detection and dedicated dark-mode textures.

### 1. Dedicated Toolbelt Slots
* **Five Dedicated Slots**: Vertically arrayed on the right side of the inventory screen for **Sword**, **Axe**, **Pickaxe**, **Shovel**, and **Hoe**.
* **Visual Silhouettes**: Empty slots render clean outline silhouettes indicating each tool type.
* **Strict Type Enforcement**: Only matching tool types can be inserted into their respective slots.
* **Non-Destructive Auto-Switching & Temporary Hotbar Slot**:
  * **Hotbar Priority**: If you are already holding a matching tool in your hotbar, that held tool takes priority over the toolbelt.
  * **Temporary Hotbar Slot**: When breaking a block or attacking with a non-matching or empty hand, the matching toolbelt tool is temporarily used via a visual slot on the right-hand side of the hotbar (symmetrical to the vanilla offhand slot on the left) without disturbing your selected hotbar slot.
  * **Auto-Hide**: The temporary slot hides once the action finishes.
* **Mending XP Support**: Picked-up experience orbs repair Mending-enchanted items in the toolbelt before adding XP levels to the player.

### 2. Dedicated Elytra Slot
* Provides a dedicated utility slot with a custom `slot_elytra` silhouette on the inventory screen for storing/equipping an Elytra.

### 3. Cycling Offhand Slots & Multi-Slot Totem Support
* **Multi-Slot Offhand**: Players have 2 offhand slots by default (extendable up to 8 slots with *Big Hands*).
* **Multi-Slot Totem of Undying Support**: Totems of Undying function from **any** offhand slot, not just the currently active one. On lethal damage, the mod sweeps through all offhand slots, consumes a totem, and triggers resurrection.
* **Smooth Cycling & Swap Hands**: Cycle through offhand items using `Ctrl + Mouse Wheel` (or rebindable keys), with seamless wrap-around. Using the Swap Hands key swaps the hotbar and offhand contents.
* **Hotbar HUD Indicator**: Displays the active offhand item in the offhand frame alongside smaller preview slots for the previous and next items.
* **Inventory Screen Layout**: Laid out as a 2-column grid to the right of the player's paper-doll model.

### 4. New Data-Driven Enchantments
* **Deep Pockets** (Max Level 4, **Leggings** only):
  * Each level adds 1 new row (9 slots) to your Deep Pockets storage.
  * Accessible in the Survival inventory, Creative inventory, and alongside other container GUIs (chests, crafting tables, furnaces, etc.).
  * Obtainable via the Enchanting Table, dungeon loot chests, and villager trades.
* **Big Hands** (Max Level 3, **Chestplates** only):
  * Each level adds 2 additional offhand slots (as additional columns to the right of the paper-doll).
* **Zero Item Loss Guarantee**:
  * When enchanted equipment is removed or downgraded, excess slots are safely drained into available main inventory slots, with any remaining overflow dropped at the player's feet.

### 5. Dynamic Inventory Screen Scaling
* Automatically recalculates the screen bounding box whenever slot counts change or the window is resized, scaling the GUI so it fits within any resolution and GUI scale without clipping.

### 6. Configurable Ender Chest Capacity
* `ender_chest_rows` allows increasing Ender Chest capacity (default 6 rows = 54 slots, up from vanilla's 3 rows = 27 slots), safely evacuating items to overflow if capacity is reduced.

### 7. Dark Mode Resource Pack Compatibility
* Automatically detects dark UI resource packs (e.g., Default Dark Mode, Vanilla Tweaks Dark UI) and switches to dedicated dark-mode textures in `assets/inventorial/textures/gui/dark/` (or can be forced via `force_dark_mode`).

---

## Installing in Horizon Launcher

> **Important:** Inventorial is a **native mod** and **must** be placed in the `horizon-native-mods` folder. It will **not** work if placed in the `horizon-mods` folder.

1. Download `inventorial.hmod` (from [Releases](../../releases) or by building from source).
2. Open your Horizon instance directory and place `inventorial.hmod` inside the **`horizon-native-mods`** folder (e.g. `~/.local/share/horizon/instances/<instance>/horizon-native-mods/`).
3. Launch the game through the Horizon Launcher (if using the mirror SDK, ensure `HORIZON_SDK_BASE=https://horizon-mirror.nikitagrimm.de` is set in your launcher environment).

---

## Building from Source

### Prerequisites
* Rust toolchain (`cargo`, `rustc`)
* `cargo-horizon-mod` CLI:
  ```bash
  cargo install --locked --git https://github.com/horizon-rust/horizon cargo-horizon-mod
  ```

### Build Commands
```bash
# Clone the repository
git clone https://github.com/AriSparks/Inventorial.git
cd Inventorial

# (Optional) Point to the Horizon SDK mirror if needed
export HORIZON_SDK_BASE="https://horizon-mirror.nikitagrimm.de"

# Run unit tests
cargo test

# Build and package target/horizon-mod/inventorial.hmod
cargo horizon-mod build
```

The packaged mod will be output to `target/horizon-mod/inventorial.hmod`.

---

## Controls & Keybindings

All keybindings are configurable in **Options → Controls → Inventorial**:

| Action | Default Key / Input | Description |
| :--- | :--- | :--- |
| **Cycle Offhand Modifier** | `Left Ctrl` (GLFW 341) | Hold while scrolling wheel to cycle offhand slots |
| **Cycle Offhand Next** | `Ctrl + Scroll Down` | Moves to the next offhand slot |
| **Cycle Offhand Prev** | `Ctrl + Scroll Up` | Moves to the previous offhand slot |
| **Quick Move** | `Shift + Left Click` | Transfers items between custom slots & main inventory |
| **Split Stack** | `Right Click` | Splits or places single items in custom slots |
| **Hotbar Swap** | Number keys `1` - `9` | Swaps held hotbar item with custom slot |

---

## Configuration

Core settings are defined in `src/config.rs` (or `inventorial.toml`):

```toml
tool_slots_count = 5                 # Number of dedicated tool slots
default_offhand_slots = 4            # Base offhand capacity
deep_pockets_max_level = 4           # Maximum level for Deep Pockets
deep_pockets_slots_per_level = 9     # Extra slots per Deep Pockets level
big_hands_max_level = 3              # Maximum level for Big Hands
big_hands_slots_per_level = 4        # Extra offhand slots per Big Hands level
auto_switch_enabled = true           # Toggle tool auto-switching
auto_switch_restore_on_finish = true # Restore previous hotbar slot after action
cycle_modifier_key = 341             # GLFW Keycode for Ctrl
min_gui_scale = 0.5                  # Minimum GUI scaling factor
max_gui_scale = 4.0                  # Maximum GUI scaling factor
screen_padding = 10.0                # Edge padding in pixels
ender_chest_rows = 6                 # Configurable Ender Chest capacity (6 rows = 54 slots)
totem_any_offhand_slot = true        # Totems of Undying function in any offhand slot
dark_mode_support = true             # Enable Dark Mode resource pack compatibility
force_dark_mode = false              # Force Dark Mode GUI theme
```

---

## Edge Case Handling

* **Death & Respawn**: When `keepInventory` is false, all custom slot contents (tools, offhand, extra pockets) drop into the world as entity items. On respawn, slots cleanly re-initialize based on newly equipped armor.
* **Save Data Compatibility**: Stored under `world/inventorial_data/<player_uuid>.json`. Loading a world without the mod leaves save files intact, and re-enabling the mod restores all items and active offhand indices.
* **Full Inventory**: If the player inventory is completely full when un-equipping Deep Pockets or Big Hands armor, excess items are dropped at the player's position rather than deleted.
