use gilrs::{Axis, Button, Event, EventType, GamepadId, Gilrs};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamepadEvent {
    Connected { name: String },
    Disconnected { name: String },
    Action { action: GamepadAction, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadAction {
    Up,
    Down,
    Left,
    Right,
    Confirm,          // South (A / Cross)
    Back,             // East (B / Circle)
    ToggleViewMode,   // West (X / Square)
    ToggleSelectGame, // North (Y / Triangle)
    NextTab,          // Right Bumper (RB / R1)
    PrevTab,          // Left Bumper (LB / L1)
    ToggleBigPicture, // Select / Back / View
    OpenMenu,         // Start / Menu
    DeleteSelected,   // Right Thumb (R3)
}

const AXIS_THRESHOLD: f32 = 0.5;
const REPEAT_INITIAL_DELAY: Duration = Duration::from_millis(250);
const REPEAT_INTERVAL: Duration = Duration::from_millis(100);

struct AxisRepeatState {
    last_action: Option<GamepadAction>,
    last_trigger: Instant,
    initial_fired: bool,
}

impl AxisRepeatState {
    fn new() -> Self {
        Self {
            last_action: None,
            last_trigger: Instant::now(),
            initial_fired: false,
        }
    }

    fn update(&mut self, current_action: Option<GamepadAction>) -> Option<GamepadAction> {
        let now = Instant::now();
        match (self.last_action, current_action) {
            (None, Some(act)) => {
                self.last_action = Some(act);
                self.last_trigger = now;
                self.initial_fired = false;
                Some(act)
            }
            (Some(prev), Some(curr)) if prev == curr => {
                let delay = if self.initial_fired {
                    REPEAT_INTERVAL
                } else {
                    REPEAT_INITIAL_DELAY
                };
                if now.duration_since(self.last_trigger) >= delay {
                    self.last_trigger = now;
                    self.initial_fired = true;
                    Some(curr)
                } else {
                    None
                }
            }
            _ => {
                self.last_action = current_action;
                self.last_trigger = now;
                self.initial_fired = false;
                current_action
            }
        }
    }
}

pub fn spawn_gamepad_thread() -> Option<(mpsc::Receiver<GamepadEvent>, Arc<AtomicBool>)> {
    let (tx, rx) = mpsc::channel();
    let suspended = Arc::new(AtomicBool::new(false));
    let suspended_flag = Arc::clone(&suspended);

    thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("Failed to initialize Gilrs gamepad subsystem: {}", e);
                return;
            }
        };

        let mut known_gamepads: HashMap<GamepadId, String> = HashMap::new();
        let mut repeat_state = AxisRepeatState::new();

        // Register initial connected gamepads
        for (id, gamepad) in gilrs.gamepads() {
            let name = gamepad.name().to_string();
            known_gamepads.insert(id, name.clone());
            let _ = tx.send(GamepadEvent::Connected { name });
        }

        loop {
            // While the TUI is away (a game has focus) the gilrs thread must
            // NOT forward events: every button pressed during gameplay would
            // otherwise pile up in the unbounded channel and be replayed as
            // TUI commands (e.g. "Confirm") the instant the game closes,
            // causing a phantom relaunch. Events are still consumed so the
            // controller state stays fresh.
            //
            // The `suspended` flag is reloaded for EVERY event, not cached for
            // the whole batch: `gilrs.next_event()` blocks, and a controller
            // hot-plugged or a button pressed while the game started in the
            // meantime must be evaluated against the CURRENT flag value, or a
            // stale `false` would leak the first few gameplay events into the
            // TUI channel. The centralized action guard in `App::update` is
            // the real defense; this flag just keeps stale events out of the
            // channel in the first place (no unbounded pile-up).
            while let Some(Event { id, event, .. }) = gilrs.next_event() {
                let suspended_now = suspended_flag.load(Ordering::Relaxed);
                match event {
                    EventType::Connected => {
                        let name = gilrs.gamepad(id).name().to_string();
                        known_gamepads.insert(id, name.clone());
                        if !suspended_now {
                            let _ = tx.send(GamepadEvent::Connected { name });
                        }
                    }
                    EventType::Disconnected => {
                        let name = known_gamepads
                            .remove(&id)
                            .unwrap_or_else(|| "Controller".to_string());
                        if !suspended_now {
                            let _ = tx.send(GamepadEvent::Disconnected { name });
                        }
                    }
                    EventType::ButtonPressed(btn, _) => {
                        if let Some(action) = map_button_to_action(btn) {
                            let name = known_gamepads
                                .get(&id)
                                .cloned()
                                .unwrap_or_else(|| "Controller".to_string());
                            if !suspended_now {
                                let _ = tx.send(GamepadEvent::Action { action, name });
                            }
                        }
                    }
                    _ => {}
                }
            }

            let suspended_now = suspended_flag.load(Ordering::Relaxed);
            if suspended_now {
                // Discard any held-button auto-repeat state while the game runs.
                repeat_state = AxisRepeatState::new();
            } else {
                // Poll active gamepad axis & D-Pad direction with auto-repeat
                let mut active_pad_info: Option<(GamepadAction, String)> = None;
                for (id, gamepad) in gilrs.gamepads() {
                    if !gamepad.is_connected() {
                        continue;
                    }

                    let mut current_dir: Option<GamepadAction> = None;
                    // D-Pad buttons
                    if gamepad.is_pressed(Button::DPadUp) {
                        current_dir = Some(GamepadAction::Up);
                    } else if gamepad.is_pressed(Button::DPadDown) {
                        current_dir = Some(GamepadAction::Down);
                    } else if gamepad.is_pressed(Button::DPadLeft) {
                        current_dir = Some(GamepadAction::Left);
                    } else if gamepad.is_pressed(Button::DPadRight) {
                        current_dir = Some(GamepadAction::Right);
                    }
                    // Left Analog Stick
                    else if let Some(y) = gamepad.axis_data(Axis::LeftStickY) {
                        if y.value() > AXIS_THRESHOLD {
                            current_dir = Some(GamepadAction::Up);
                        } else if y.value() < -AXIS_THRESHOLD {
                            current_dir = Some(GamepadAction::Down);
                        }
                    }

                    if current_dir.is_none() {
                        if let Some(x) = gamepad.axis_data(Axis::LeftStickX) {
                            if x.value() > AXIS_THRESHOLD {
                                current_dir = Some(GamepadAction::Right);
                            } else if x.value() < -AXIS_THRESHOLD {
                                current_dir = Some(GamepadAction::Left);
                            }
                        }
                    }

                    if let Some(act) = current_dir {
                        let pad_name = known_gamepads
                            .get(&id)
                            .cloned()
                            .unwrap_or_else(|| gamepad.name().to_string());
                        active_pad_info = Some((act, pad_name));
                        break;
                    }
                }

                let act_dir = active_pad_info.as_ref().map(|(act, _)| *act);
                if let Some(action) = repeat_state.update(act_dir) {
                    if let Some((_, ref pad_name)) = active_pad_info {
                        let _ = tx.send(GamepadEvent::Action {
                            action,
                            name: pad_name.clone(),
                        });
                    }
                }
            }

            thread::sleep(Duration::from_millis(16)); // ~60 Hz polling rate
        }
    });

    Some((rx, suspended))
}

fn map_button_to_action(btn: Button) -> Option<GamepadAction> {
    match btn {
        Button::South => Some(GamepadAction::Confirm),
        Button::East => Some(GamepadAction::Back),
        Button::West => Some(GamepadAction::ToggleViewMode),
        Button::North => Some(GamepadAction::ToggleSelectGame),
        Button::RightThumb => Some(GamepadAction::DeleteSelected),
        Button::LeftTrigger | Button::LeftTrigger2 => Some(GamepadAction::PrevTab),
        Button::RightTrigger | Button::RightTrigger2 => Some(GamepadAction::NextTab),
        Button::Select => Some(GamepadAction::ToggleBigPicture),
        Button::Start => Some(GamepadAction::OpenMenu),
        Button::DPadUp => Some(GamepadAction::Up),
        Button::DPadDown => Some(GamepadAction::Down),
        Button::DPadLeft => Some(GamepadAction::Left),
        Button::DPadRight => Some(GamepadAction::Right),
        _ => None,
    }
}
