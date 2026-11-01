//! Configuration-boundary regressions derived from REMAKE's KiCad nets.
use rmk_config::{resolved::hardware::BoardConfig, KeyboardTomlConfig};

fn config() -> KeyboardTomlConfig {
    KeyboardTomlConfig::new_from_toml_path(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../keyboard.toml"
    ))
}

#[test]
fn host_layout_exposes_only_the_36_physical_switches() {
    let layout =
        rmk_config::layout::LayoutInfo::from_compressed_blob(&config().layout().unwrap().blob)
            .unwrap();
    for variant in layout.variants {
        assert_eq!(variant.keys.len(), 36);
        let thumbs: Vec<_> = variant
            .keys
            .iter()
            .filter(|key| key.row == 3)
            .map(|key| key.col)
            .collect();
        assert_eq!(thumbs, [2, 3, 4, 5, 6, 7]);
    }
}

#[test]
fn all_six_thumbs_access_the_miryoku_layers() {
    let map = config().keymap().unwrap();
    assert_eq!(map.layers, 7);
    for (col, want) in [
        (2, "LT(3,Escape,THUMB)"),
        (3, "LT(1,Space,THUMB)"),
        (4, "LT(2,Tab,THUMB)"),
        (5, "LT(5,Enter,THUMB)"),
        (6, "LT(4,Backspace,THUMB)"),
        (7, "LT(6,Delete,THUMB)"),
    ] {
        assert_eq!(map.keymap[0][3][col].replace(' ', ""), want);
    }
}

#[test]
fn absent_thumb_switches_have_no_action_on_any_layer() {
    let map = config().keymap().unwrap();
    for layer in &map.keymap {
        for col in [0, 1, 8, 9] {
            assert_eq!(layer[3][col], "No");
        }
    }
}

#[test]
fn miryoku_has_no_legacy_tri_layer_or_thumb_combos() {
    let behavior = config().behavior().unwrap();
    assert!(behavior.tri_layer.is_none());
    assert!(behavior.combos.is_none());
}

#[test]
fn physical_column_nets_reach_the_correct_logical_keys_on_both_halves() {
    let config = config();
    let BoardConfig::Split(split) = config.hardware().unwrap().board else {
        panic!("REMAKE must be split");
    };
    let map = config.keymap().unwrap();
    // JPL/JPR socket contacts traced to the vendor nice!nano v2 pinout.
    // U1's D18/P0.15 label is a typo: the actual module GPIO is P1.15.
    // The right module is flipped: socket n is module contact 25-n. The right
    // physical COL0 is the outer column (P), not the inner column (Y).
    for (board, nets, wants) in [
        (
            &split.central,
            ["P1_15", "P1_13", "P1_11", "P0_10", "P0_09"],
            ["Q", "W", "F", "P", "B"],
        ),
        (
            &split.peripheral[0],
            ["P1_15", "P1_13", "P1_11", "P0_10", "P0_09"],
            ["Quote", "Y", "U", "L", "J"],
        ),
    ] {
        let pins = board.matrix.col_pins.as_ref().unwrap();
        for (pin, want) in nets.into_iter().zip(wants) {
            let local_col = pins.iter().position(|p| p == pin).unwrap();
            assert_eq!(
                map.keymap[0][board.row_offset][board.col_offset + local_col],
                want
            );
        }
    }
}

#[test]
fn opposite_hand_layers_have_miryoku_navigation_numbers_symbols_and_functions() {
    let map = config().keymap().unwrap();
    for (layer, row, col, want) in [
        (1, 1, 6, "Left"),
        (1, 1, 7, "Down"),
        (1, 1, 8, "Up"),
        (1, 1, 9, "Right"),
        (2, 1, 6, "MouseLeft"),
        (2, 2, 8, "MouseWheelUp"),
        (2, 3, 6, "MouseBtn1"),
        (3, 1, 7, "AudioVolDown"),
        (3, 3, 6, "MediaPlayPause"),
        (4, 0, 1, "7"),
        (4, 3, 3, "0"),
        (5, 0, 1, "WM(7,LShift)"),
        (5, 3, 3, "WM(0,LShift)"),
        (6, 0, 1, "F7"),
        (6, 3, 2, "Application"),
    ] {
        assert_eq!(map.keymap[layer][row][col].replace(' ', ""), want);
    }
}

#[test]
fn flipped_right_socket_routes_each_row_to_the_same_gpio_as_the_left() {
    let BoardConfig::Split(split) = config().hardware().unwrap().board else {
        panic!("REMAKE must be split");
    };
    for board in [&split.central, &split.peripheral[0]] {
        assert_eq!(
            board.matrix.row_pins.as_ref().unwrap(),
            &["P1_00", "P0_11", "P1_04", "P1_06"]
        );
        assert!(!board.matrix.row2col);
    }
}
