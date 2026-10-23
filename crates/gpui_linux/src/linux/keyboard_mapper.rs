use collections::HashMap;
use gpui::{KeybindingKeystroke, Keystroke, PlatformKeyboardMapper};
use xkbcommon::xkb::{self, Keycode};

/// The keys of a US keyboard that carry two symbols, and the extra key of an ISO keyboard, as
/// xkb keycodes (the evdev code plus 8).
const SYMBOL_KEYCODES: [u32; 22] = [
    49, // `
    10, 11, 12, 13, 14, 15, 16, 17, 18, 19, // 1 to 0
    20, 21, // - and =
    34, 35, 51, // [, ] and \
    47, 48, // ; and '
    59, 60, 61, // , . and /
    94, // the key left of Z on an ISO keyboard
];

/// Reads a binding's symbol keys the way the active xkb layout types them, so `ctrl-shift-7` is
/// Ctrl+/ on a German layout, where Shift+7 types `/`.
///
/// `Keystroke::should_match` reads Shift on a symbol key as a US keyboard types it. A binding
/// that reading already gets right stays as written, so a US layout changes nothing.
pub(crate) struct LinuxKeyboardMapper {
    /// What each symbol key types alone and with Shift.
    keys: Vec<(String, String)>,
}

impl LinuxKeyboardMapper {
    pub(crate) fn new(state: &xkb::State) -> Self {
        let keymap = state.get_keymap();
        let layout = state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
        // A key counts when it types one ASCII symbol or digit at both levels. Some layouts put
        // letters on these keys (`m` on a French one), and a letter keeps its Shift modifier.
        let types = |keycode: u32, level: u32| {
            let [keysym] = keymap.key_get_syms_by_level(Keycode::new(keycode), layout, level)
            else {
                return None;
            };
            keysym
                .key_char()
                .filter(|key| key.is_ascii_graphic() && !key.is_ascii_alphabetic())
                .map(String::from)
        };
        let keys = SYMBOL_KEYCODES
            .iter()
            .filter_map(|&keycode| Some((types(keycode, 0)?, types(keycode, 1)?)))
            .collect();
        Self { keys }
    }
}

impl PlatformKeyboardMapper for LinuxKeyboardMapper {
    fn map_key_equivalent(
        &self,
        keystroke: Keystroke,
        _use_key_equivalents: bool,
    ) -> KeybindingKeystroke {
        let Some((plain, shifted)) = self
            .keys
            .iter()
            .find(|(plain, shifted)| keystroke.key == *plain || keystroke.key == *shifted)
        else {
            return KeybindingKeystroke::from_keystroke(keystroke);
        };

        let mut mapped = keystroke.clone();
        mapped.key = if keystroke.modifiers.shift || keystroke.key == *shifted {
            shifted.clone()
        } else {
            plain.clone()
        };
        mapped.modifiers.shift = false;
        let mapped = KeybindingKeystroke::from_keystroke(mapped);
        if keystroke.should_match(&mapped) {
            KeybindingKeystroke::from_keystroke(keystroke)
        } else {
            mapped
        }
    }

    fn get_key_equivalents(&self) -> Option<&HashMap<char, char>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::{keystroke_from_xkb, modifiers_from_xkb};
    use gpui::Modifiers;

    const LEFT_CTRL: u32 = 37;
    const LEFT_SHIFT: u32 = 50;

    /// The state of the named layout with Ctrl and Shift held, made without a display.
    fn state(layout: &str) -> xkb::State {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "evdev",
            "pc105",
            layout,
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap_or_else(|| panic!("no xkb keymap for layout {layout}"));
        let mut state = xkb::State::new(&keymap);
        state.update_key(Keycode::new(LEFT_CTRL), xkb::KeyDirection::Down);
        state.update_key(Keycode::new(LEFT_SHIFT), xkb::KeyDirection::Down);
        state
    }

    fn control_shift(key: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::control_shift(),
            key: key.into(),
            key_char: None,
        }
    }

    #[test]
    fn test_a_us_layout_leaves_bindings_as_written() {
        let mapper = LinuxKeyboardMapper::new(&state("us"));
        assert!(!mapper.keys.is_empty());
        for (plain, shifted) in &mapper.keys {
            for key in [plain, shifted] {
                for modifiers in [Modifiers::control(), Modifiers::control_shift()] {
                    let keystroke = Keystroke {
                        modifiers,
                        key: key.clone(),
                        key_char: None,
                    };
                    assert_eq!(
                        mapper.map_key_equivalent(keystroke.clone(), false),
                        KeybindingKeystroke::from_keystroke(keystroke)
                    );
                }
            }
        }
    }

    #[test]
    fn test_a_binding_matches_what_the_layout_sends_for_its_shifted_symbol() {
        // The layout, the key a binding names, the key Ctrl+Shift+<that key> presses, and the
        // symbol the binding reads as.
        for (layout, key, keycode, symbol) in [
            ("de", "7", 16, "/"),
            ("de", "<", 94, ">"),
            ("fr", "1", 10, "1"),
        ] {
            let state = state(layout);
            let binding = control_shift(key);
            let mapped =
                LinuxKeyboardMapper::new(&state).map_key_equivalent(binding.clone(), false);
            assert_eq!(mapped.inner().unparse(), format!("ctrl-{symbol}"));

            let sent =
                keystroke_from_xkb(&state, modifiers_from_xkb(&state), Keycode::new(keycode));
            assert!(
                sent.should_match(&mapped),
                "{layout} ctrl-shift-{key} for {sent:?}"
            );
            let as_written = KeybindingKeystroke::from_keystroke(binding);
            assert!(!sent.should_match(&as_written), "{layout} ctrl-shift-{key}");
        }
    }

    #[test]
    fn test_letters_and_named_keys_keep_their_shift() {
        // `m` is on the key a US keyboard types `;` with.
        for (layout, key) in [("fr", "m"), ("de", "tab")] {
            let binding = control_shift(key);
            let mapped =
                LinuxKeyboardMapper::new(&state(layout)).map_key_equivalent(binding.clone(), false);
            assert_eq!(mapped, KeybindingKeystroke::from_keystroke(binding));
        }
    }
}
