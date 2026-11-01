//! The actual seven-layer TOML map driven through released RMK's keyboard engine.
//! Only configuration-to-action translation and the clock/report sink are test utilities.
use embassy_time::{Duration, MockDriver, Timer};
use rmk::{
    config::{BehaviorConfig, Hand, PositionalConfig},
    core_traits::Runnable,
    event::KeyboardEvent,
    hid::Report,
    keyboard::Keyboard,
    keymap::{KeyMap, KeymapData},
    types::{
        action::{Action, KeyAction, KeyboardAction},
        keycode::{HidKeyCode as K, KeyCode},
        modifier::ModifierCombination as Mods,
        morse::{Morse, MorseMode, MorseProfile},
    },
};
use std::{
    future::Future,
    task::{Context, Poll},
};

fn profile(p: &rmk_config::resolved::behavior::MorseProfile) -> MorseProfile {
    let mode = if p.permissive_hold == Some(true) {
        Some(MorseMode::PermissiveHold)
    } else if p.hold_on_other_press == Some(true) {
        Some(MorseMode::HoldOnOtherPress)
    } else if p.normal_mode == Some(true) {
        Some(MorseMode::Normal)
    } else {
        None
    };
    MorseProfile::new(
        p.unilateral_tap,
        mode,
        p.hold_timeout_ms.map(|t| t.try_into().unwrap()),
        p.gap_timeout_ms.map(|t| t.try_into().unwrap()),
    )
    .with_enable_flow_tap(p.enable_flow_tap)
    .with_quick_tap_timeout_ms(p.quick_tap_timeout_ms.map(|t| t.try_into().unwrap()))
}

fn modifier(name: &str) -> Mods {
    match name {
        "LCtrl" => Mods::LCTRL,
        "LShift" => Mods::LSHIFT,
        "LAlt" => Mods::LALT,
        "LGui" => Mods::LGUI,
        "RCtrl" => Mods::RCTRL,
        "RShift" => Mods::RSHIFT,
        "RAlt" => Mods::RALT,
        "RGui" => Mods::RGUI,
        _ => panic!("unsupported modifier {name}"),
    }
}

fn hid(name: &str) -> K {
    let name = if name.len() == 1 && name.as_bytes()[0].is_ascii_digit() {
        format!("Kc{name}")
    } else {
        name.to_owned()
    };
    serde_json::from_value(serde_json::Value::String(name)).unwrap()
}

fn action(text: &str, profiles: &[&String]) -> KeyAction {
    let text = text.replace(' ', "");
    if text == "No" {
        return KeyAction::No;
    }
    if text == "_" {
        return KeyAction::Transparent;
    }
    if let Some((kind, args)) = text.split_once('(') {
        let args: Vec<_> = args.strip_suffix(')').unwrap().split(',').collect();
        return match kind {
            "LT" => KeyAction::TapHold(
                Action::Key(KeyCode::Hid(hid(args[1]))),
                Action::LayerOn(args[0].parse().unwrap()),
                profiles
                    .iter()
                    .position(|p| p.as_str() == args[2])
                    .unwrap()
                    .try_into()
                    .unwrap(),
            ),
            "MT" => KeyAction::TapHold(
                Action::Key(KeyCode::Hid(hid(args[0]))),
                Action::Modifier(modifier(args[1])),
                u8::MAX,
            ),
            "WM" => KeyAction::Single(Action::KeyWithModifier(hid(args[0]), modifier(args[1]))),
            "TD" => KeyAction::Morse(args[0].parse().unwrap()),
            "Macro" => KeyAction::Single(Action::TriggerMacro(args[0].parse().unwrap())),
            _ => panic!("unsupported action {text}"),
        };
    }
    if let Some(id) = text.strip_prefix("User") {
        return KeyAction::Single(Action::User(id.parse().unwrap()));
    }
    if let Ok(control) =
        serde_json::from_value::<KeyboardAction>(serde_json::Value::String(text.clone()))
    {
        return KeyAction::Single(Action::KeyboardControl(control));
    }
    KeyAction::Single(Action::Key(KeyCode::Hid(hid(&text))))
}

#[derive(Default)]
struct Trace {
    held: [u8; 6],
    presses: Vec<(K, u8)>,
    modifiers: Vec<u8>,
    layers: Vec<u8>,
    mouse: Vec<(u8, i8, i8, i8, i8)>,
    media: Vec<u16>,
}

fn drive<F: Future>(future: F, trace: &mut Trace) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    for _ in 0..50_000 {
        let result = future.as_mut().poll(&mut cx);
        while let Ok(report) = rmk::channel::USB_REPORT_CHANNEL.try_receive() {
            match report {
                Report::KeyboardReport(report) => {
                    trace.modifiers.push(report.modifier);
                    for key in report.keycodes {
                        if key != 0 && !trace.held.contains(&key) {
                            trace.presses.push((K::from(key), report.modifier));
                        }
                    }
                    trace.held = report.keycodes;
                }
                Report::MouseReport(r) => trace.mouse.push((r.buttons, r.x, r.y, r.wheel, r.pan)),
                Report::MediaKeyboardReport(r) => trace.media.push(r.usage_id),
                _ => panic!("unexpected report"),
            }
        }
        if let Poll::Ready(value) = result {
            return value;
        }
        MockDriver::get().advance(Duration::from_micros(100));
    }
    panic!("keyboard operation stalled for five virtual seconds");
}

enum Step {
    Down(u8, u8),
    Up(u8, u8),
    Wait(u64),
}
use Step::{Down, Up, Wait};

fn sequence(steps: &[Step]) -> Trace {
    let _lock = crate::tests::EVENT_LOCK.lock().unwrap();
    MockDriver::get().reset();
    rmk::state::set_usb_state(rmk::types::connection::UsbState::Configured);
    rmk::channel::USB_REPORT_CHANNEL.clear();
    let config = rmk_config::KeyboardTomlConfig::new_from_toml_path(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../keyboard.toml"
    ));
    let behavior_config = config.behavior().unwrap();
    let resolved = behavior_config.morse.unwrap();
    let mut behavior = BehaviorConfig::default();
    if let Some(macros) = behavior_config.macros {
        let sequences: Vec<rmk::heapless::Vec<_, { rmk::types::constants::MACRO_SPACE_SIZE }>> =
            macros
                .macros
                .iter()
                .map(|m| {
                    m.operations
                        .iter()
                        .map(|operation| match operation {
                            rmk_config::resolved::behavior::MacroOperation::Tap { keycode } => {
                                rmk::keyboard_macros::MacroOperation::Tap(hid(keycode))
                            }
                            _ => panic!("unsupported fixture macro operation"),
                        })
                        .collect()
                })
                .collect();
        behavior.keyboard_macros.macro_sequences =
            rmk::keyboard_macros::define_macro_sequences(&sequences);
    }
    behavior.morse.enable_flow_tap = resolved.enable_flow_tap;
    behavior.morse.prior_idle_time = Duration::from_millis(resolved.prior_idle_time_ms);
    behavior.morse.default_profile = profile(&resolved.default_profile);
    let mut names: Vec<_> = resolved.profiles.keys().collect();
    names.sort();
    for name in &names {
        behavior
            .morse
            .profiles
            .push(profile(&resolved.profiles[*name]))
            .unwrap();
    }
    for key in &resolved.morses {
        behavior
            .morse
            .morses
            .push(Morse::new_from_vial(
                action(key.tap.as_deref().unwrap(), &names).to_action(),
                action(key.hold.as_deref().unwrap(), &names).to_action(),
                action(key.hold_after_tap.as_deref().unwrap(), &names).to_action(),
                action(key.double_tap.as_deref().unwrap(), &names).to_action(),
                profile(&resolved.profiles[key.profile.as_ref().unwrap()]),
            ))
            .unwrap();
    }
    let resolved_map = config.keymap().unwrap();
    let map = std::array::from_fn::<_, 7, _>(|layer| {
        std::array::from_fn::<_, 4, _>(|row| {
            std::array::from_fn::<_, 10, _>(|col| {
                action(&resolved_map.keymap[layer][row][col], &names)
            })
        })
    });
    let mut data = KeymapData::new(map);
    let positions = PositionalConfig::new(std::array::from_fn(|_| {
        std::array::from_fn(|col| if col < 5 { Hand::Left } else { Hand::Right })
    }));
    let mut trace = Trace::default();
    let keymap = drive(
        KeyMap::new(&mut data, &mut behavior, &positions),
        &mut trace,
    );
    let mut keyboard = Keyboard::new(&keymap);
    MockDriver::get().advance(Duration::from_secs(1));
    for step in steps {
        match *step {
            Down(row, col) | Up(row, col) => {
                // Service RMK between physical events.
                drive(
                    embassy_futures::select::select(
                        keyboard.run(),
                        Timer::after(Duration::from_millis(30)),
                    ),
                    &mut trace,
                );
                // Send events through the real subscriber loop. Calling process_inner
                // directly bypasses run()'s post-event macro execution.
                rmk::event::publish_event(KeyboardEvent::key(row, col, matches!(step, Down(..))));
                drive(
                    embassy_futures::select::select(
                        keyboard.run(),
                        Timer::after(Duration::from_millis(3)),
                    ),
                    &mut trace,
                );
            }
            Wait(ms) => {
                drive(
                    embassy_futures::select::select(
                        keyboard.run(),
                        Timer::after(Duration::from_millis(ms)),
                    ),
                    &mut trace,
                );
            }
        }
        trace.layers.push(keymap.active_layer());
    }
    trace
}

#[test]
fn all_six_thumb_taps_emit_their_base_keys_without_modifiers_or_layers() {
    for (col, want) in [
        (2, K::Escape),
        (3, K::Space),
        (4, K::Tab),
        (5, K::Enter),
        (6, K::Backspace),
        (7, K::Delete),
    ] {
        let trace = sequence(&[Down(3, col), Up(3, col), Wait(250)]);
        assert_eq!(trace.presses, [(want, 0)]);
        assert!(trace.layers.iter().all(|layer| *layer == 0));
        assert_eq!(trace.held, [0; 6]);
    }
}

#[test]
fn all_six_thumb_holds_activate_and_release_only_their_own_layer() {
    for (col, layer) in [(2, 3), (3, 1), (4, 2), (5, 5), (6, 4), (7, 6)] {
        let trace = sequence(&[Down(3, col), Wait(210), Up(3, col)]);
        assert_eq!(trace.layers, [0, layer, 0]);
        assert!(trace.presses.is_empty());
        assert_eq!(trace.held, [0; 6]);
    }
}

#[test]
fn nav_num_sym_and_fun_holds_emit_opposite_hand_actions_not_base_letters() {
    for (thumb, row, col, want, mods) in [
        (3, 1, 6, K::Left, 0),
        (6, 0, 1, K::Kc7, 0),
        (5, 0, 1, K::Kc7, Mods::LSHIFT.into_bits()),
        (7, 0, 1, K::F7, 0),
    ] {
        let trace = sequence(&[
            Down(3, thumb),
            Wait(210),
            Down(row, col),
            Up(row, col),
            Up(3, thumb),
        ]);
        assert_eq!(trace.presses, [(want, mods)]);
        assert_eq!(trace.held, [0; 6]);
        assert_eq!(trace.layers.last(), Some(&0));
    }
}

#[test]
fn early_num_chord_resolves_the_thumb_hold_before_the_number_key() {
    let trace = sequence(&[Down(3, 6), Down(0, 1), Up(0, 1), Up(3, 6)]);
    assert_eq!(trace.presses, [(K::Kc7, 0)]);
    assert_eq!(trace.layers.last(), Some(&0));
}

#[test]
fn mouse_layer_emits_motion_and_click_reports_without_typing() {
    let trace = sequence(&[
        Down(3, 4),
        Wait(210),
        Down(1, 6),
        Wait(100),
        Up(1, 6),
        Down(3, 6),
        Up(3, 6),
        Up(3, 4),
    ]);
    assert!(trace.presses.is_empty());
    assert!(trace.mouse.iter().any(|r| r.1 < 0));
    assert!(trace.mouse.iter().any(|r| r.0 & 1 != 0));
    assert_eq!(trace.mouse.last().unwrap().0, 0);
    assert_eq!(trace.layers.last(), Some(&0));
}

#[test]
fn media_layer_emits_play_pause_and_release_without_delete_or_enter() {
    let trace = sequence(&[Down(3, 2), Wait(210), Down(3, 6), Up(3, 6), Up(3, 2)]);
    assert!(trace.presses.is_empty());
    assert_eq!(trace.media, [0xCD, 0]);
    assert_eq!(trace.layers.last(), Some(&0));
}

#[test]
fn held_home_row_modifiers_are_mirrored_super_alt_ctrl_shift() {
    for (col, want) in [
        (0, Mods::LGUI),
        (1, Mods::LALT),
        (2, Mods::LCTRL),
        (3, Mods::LSHIFT),
        (6, Mods::RSHIFT),
        (7, Mods::RCTRL),
        (8, Mods::RALT),
        (9, Mods::RGUI),
    ] {
        let trace = sequence(&[Down(1, col), Wait(210), Up(1, col)]);
        assert!(trace.presses.is_empty());
        assert!(trace.modifiers.contains(&want.into_bits()));
        assert_eq!(trace.modifiers.last(), Some(&0));
    }
}

fn redo_after(thumb: u8, previous: (u8, u8), previous_key: K) {
    let trace = sequence(&[
        Down(3, thumb),
        Wait(210),
        Down(previous.0, previous.1),
        Up(previous.0, previous.1),
        Down(0, 5),
        Up(0, 5),
        Up(3, thumb),
    ]);
    assert_eq!(trace.presses, [(previous_key, 0), (K::Again, 0)]);
    assert_eq!(trace.held, [0; 6]);
    assert_eq!(trace.layers.last(), Some(&0));
}

#[test]
fn nav_redo_emits_hid_again_instead_of_another_undo() {
    redo_after(3, (0, 9), K::Undo);
}

#[test]
fn mouse_redo_emits_hid_again_instead_of_another_undo() {
    redo_after(4, (0, 9), K::Undo);
}

fn redo_after_q(thumb: u8) {
    let trace = sequence(&[
        Down(0, 0),
        Up(0, 0),
        Down(3, thumb),
        Wait(210),
        Down(0, 5),
        Up(0, 5),
        Up(3, thumb),
    ]);
    assert_eq!(trace.presses, [(K::Q, 0), (K::Again, 0)]);
    assert_eq!(trace.held, [0; 6]);
}

#[test]
fn nav_redo_does_not_type_the_previous_letter() {
    redo_after_q(3);
}

#[test]
fn mouse_redo_does_not_type_the_previous_letter() {
    redo_after_q(4);
}
