//! Colour emoji in the platform's own style, via fastframe-emoji:
//! the desktop's emoji font with bundled Noto Color Emoji as fallback.

/// Bundled Noto Color Emoji (OFL; see assets/fonts/NotoColorEmoji-LICENSE.txt).
const BUNDLED: &[u8] = include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

fn setup() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        fastframe_emoji::EmojiSetup::default()
            .bundled(BUNDLED)
            .install();
    });
}

/// Find + map the emoji fonts off the UI thread before the first frame.
pub fn warm_up() {
    setup();
    fastframe_emoji::warm_up();
}

/// The egui plugin that colours emoji in every text egui draws.
pub fn plugin() -> fastframe_emoji::EmojiPlugin {
    setup();
    fastframe_emoji::EmojiPlugin::default()
}
