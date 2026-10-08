use std::borrow::Cow;

/// The bundled sans-serif family, Roboto, Android's system font. `.SystemUIFont` resolves to it,
/// as it does on Android.
pub const SANS_FAMILY: &str = "Roboto";

/// The bundled monospace family, DejaVu Sans Mono, GPUI Kit's default monospace family on Linux.
pub const MONO_FAMILY: &str = "DejaVu Sans Mono";

/// The fonts every [`crate::ScreenshotApp`] starts with, and the only ones it has unless a test
/// adds more: Roboto Regular, Medium and Bold, and DejaVu Sans Mono.
///
/// Text in a family outside this set panics, as it does in GPUI when a font is missing. Weights
/// between the bundled ones resolve to the nearest bundled weight, 600 to Bold for example.
pub fn bundled_fonts() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(include_bytes!("../fonts/Roboto-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/Roboto-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/Roboto-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/DejaVuSansMono.ttf")),
    ]
}
