//! Rebindable controls and input interception.

#[derive(Debug, Clone)]
pub struct KeybindSettings {
    /// Modifier key code (e.g. GLFW 341 = Left Control).
    pub modifier_key: i32,
    /// Optional discrete key for cycling next offhand.
    pub cycle_next_key: Option<i32>,
    /// Optional discrete key for cycling previous offhand.
    pub cycle_prev_key: Option<i32>,
    /// Whether mouse wheel cycling with modifier is enabled.
    pub enable_mouse_wheel_cycling: bool,
}

impl Default for KeybindSettings {
    fn default() -> Self {
        Self {
            modifier_key: 341, // GLFW_KEY_LEFT_CONTROL
            cycle_next_key: None,
            cycle_prev_key: None,
            enable_mouse_wheel_cycling: true,
        }
    }
}

pub struct InputManager {
    pub settings: KeybindSettings,
    is_modifier_pressed: bool,
}

impl InputManager {
    pub fn new(settings: KeybindSettings) -> Self {
        Self {
            settings,
            is_modifier_pressed: false,
        }
    }

    /// Update modifier key state on key events.
    pub fn on_key_event(&mut self, key: i32, action: KeyAction) -> Option<i32> {
        if key == self.settings.modifier_key {
            self.is_modifier_pressed = matches!(action, KeyAction::Press | KeyAction::Repeat);
        }

        if action == KeyAction::Press {
            if Some(key) == self.settings.cycle_next_key {
                return Some(1);
            }
            if Some(key) == self.settings.cycle_prev_key {
                return Some(-1);
            }
        }
        None
    }

    /// Process mouse scroll wheel events.
    /// If modifier key (Ctrl) is pressed, intercepts the scroll to cycle offhand.
    ///
    /// Returns:
    /// - `Some(1)` for next
    /// - `Some(-1)` for previous
    /// - `None` if modifier not held (allows normal hotbar slot cycling).
    pub fn on_mouse_scroll(&self, _x_offset: f64, y_offset: f64) -> Option<i32> {
        if !self.settings.enable_mouse_wheel_cycling || !self.is_modifier_pressed {
            return None;
        }

        if y_offset > 0.0 {
            // Scrolled up -> cycle previous
            Some(-1)
        } else if y_offset < 0.0 {
            // Scrolled down -> cycle next
            Some(1)
        } else {
            None
        }
    }

    pub fn is_modifier_held(&self) -> bool {
        self.is_modifier_pressed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Release = 0,
    Press = 1,
    Repeat = 2,
}
