use gilrs::{Axis, Button, Event, EventType, GamepadId, Gilrs};
use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamepadEvent {
    Connected { name: String },
    Disconnected { name: String },
    Action(GamepadAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadAction {
    Up,
    Down,
    Left,
    Right,
    Confirm,          // South (A / Cross)
    Back,             // East (B / Circle)
    Search,           // West (X / Square)
    Details,          // North (Y / Triangle)
    NextTab,          // Right Bumper (RB / R1)
    PrevTab,          // Left Bumper (LB / L1)
    ToggleBigPicture, // Select / Back / View
    OpenMenu,         // Start / Menu
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

pub fn spawn_gamepad_thread() -> Option<mpsc::Receiver<GamepadEvent>> {
    let (tx, rx) = mpsc::channel();

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
            while let Some(Event { id, event, .. }) = gilrs.next_event() {
                match event {
                    EventType::Connected => {
                        let name = gilrs.gamepad(id).name().to_string();
                        known_gamepads.insert(id, name.clone());
                        let _ = tx.send(GamepadEvent::Connected { name });
                    }
                    EventType::Disconnected => {
                        let name = known_gamepads
                            .remove(&id)
                            .unwrap_or_else(|| "Controller".to_string());
                        let _ = tx.send(GamepadEvent::Disconnected { name });
                    }
                    EventType::ButtonPressed(btn, _) => {
                        if let Some(action) = map_button_to_action(btn) {
                            let _ = tx.send(GamepadEvent::Action(action));
                        }
                    }
                    _ => {}
                }
            }

            // Poll active gamepad axis & D-Pad direction with auto-repeat
            let mut current_dir: Option<GamepadAction> = None;
            for (_id, gamepad) in gilrs.gamepads() {
                if !gamepad.is_connected() {
                    continue;
                }

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

                if current_dir.is_some() {
                    break;
                }
            }

            if let Some(action) = repeat_state.update(current_dir) {
                let _ = tx.send(GamepadEvent::Action(action));
            }

            thread::sleep(Duration::from_millis(16)); // ~60 Hz polling rate
        }
    });

    Some(rx)
}

fn map_button_to_action(btn: Button) -> Option<GamepadAction> {
    match btn {
        Button::South => Some(GamepadAction::Confirm),
        Button::East => Some(GamepadAction::Back),
        Button::West => Some(GamepadAction::Search),
        Button::North => Some(GamepadAction::Details),
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
