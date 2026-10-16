//! Colour emoji in the platform's own style, via fastframe-emoji:
//! the desktop's emoji font with bundled Noto Color Emoji as fallback.

/// Bundled Noto Color Emoji (OFL; see assets/fonts/NotoColorEmoji-LICENSE.txt).
const BUNDLED: &[u8] = include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

pub fn setup() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        fastframe_emoji::EmojiSetup::default()
            .bundled(BUNDLED)
            .install();
    });
}
