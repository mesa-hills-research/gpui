//! Looking up one font family by its name, on the Linux text system.
#![cfg(target_os = "linux")]

use std::{borrow::Cow, sync::Arc};

use gpui::{HeadlessAppContext, PlatformTextSystem as _, font};
use gpui_screenshot::{MONO_FAMILY, SANS_FAMILY, bundled_fonts};
use gpui_wgpu::CosmicTextSystem;

const FIGTREE: &[u8] = include_bytes!("fonts/Figtree[wght].ttf");

#[test]
fn a_font_family_is_available_once_its_fonts_are_added() {
    let text_system = Arc::new(CosmicTextSystem::new_without_system_fonts(SANS_FAMILY));
    let cx = HeadlessAppContext::new(text_system);
    let text_system = cx.text_system().clone();
    assert!(!text_system.has_font_family(SANS_FAMILY));

    text_system.add_fonts(bundled_fonts()).unwrap();
    assert!(text_system.has_font_family(SANS_FAMILY));
    assert!(text_system.has_font_family(MONO_FAMILY));
    assert!(!text_system.has_font_family("Figtree"));

    text_system.add_fonts(vec![Cow::Borrowed(FIGTREE)]).unwrap();
    assert!(text_system.has_font_family("Figtree"));
    for name in text_system.all_font_names() {
        assert!(text_system.has_font_family(&name), "{name}");
    }
}

#[test]
fn a_font_family_is_available_when_a_font_of_it_resolves() {
    let platform_text_system = Arc::new(CosmicTextSystem::new_without_system_fonts(SANS_FAMILY));
    let cx = HeadlessAppContext::new(platform_text_system.clone());
    let text_system = cx.text_system().clone();
    text_system.add_fonts(bundled_fonts()).unwrap();

    for name in [
        SANS_FAMILY,
        "roboto",
        ".SystemUIFont",
        MONO_FAMILY,
        "DejaVu Sans",
        "Nonexistent Sans",
        "",
    ] {
        assert_eq!(
            text_system.has_font_family(name),
            platform_text_system.font_id(&font(name)).is_ok(),
            "{name:?}"
        );
    }
    assert!(text_system.has_font_family(".SystemUIFont"));
    assert!(!text_system.has_font_family("Nonexistent Sans"));
}
