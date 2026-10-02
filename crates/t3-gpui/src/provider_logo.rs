//! Embedded SVG brand marks, tinted for contrast by each native surface.
use gpui_kit::*;
use std::sync::{Arc, OnceLock};

pub fn logo(driver: &str, size: Pixels, color: Hsla) -> AnyElement {
    // Full-color SVGs use the image renderer. An alpha mask would flatten
    // gradients and OpenCode's two-tone interior into a solid rectangle.
    macro_rules! colored {
        ($file:literal, $cache:ident) => {{
            static $cache: OnceLock<Arc<Image>> = OnceLock::new();
            let image = $cache.get_or_init(|| {
                Arc::new(Image::from_bytes(
                    ImageFormat::Svg,
                    include_bytes!(concat!("../assets/providers/", $file)).to_vec(),
                ))
            });
            return img(image.clone())
                .size(size)
                .object_fit(ObjectFit::Contain)
                .flex_shrink_0()
                .into_any_element();
        }};
    }
    match driver {
        "antigravity" => colored!("antigravity.svg", ANTIGRAVITY),
        "gemini" => colored!("google-gemini-icon.svg", GEMINI),
        "opencode" | "openCode" if color.l > 0.5 => {
            colored!("opencode-icon-dark.svg", OPENCODE_DARK)
        }
        "opencode" | "openCode" => colored!("opencode-icon.svg", OPENCODE),
        _ => {}
    }
    let bytes: Option<&'static [u8]> = match driver {
        "codex" | "openai" => Some(include_bytes!("../assets/providers/codex.svg")),
        "claudeAgent" | "claude" => Some(include_bytes!("../assets/providers/claude-icon.svg")),
        "cursor" => Some(include_bytes!("../assets/providers/cursor-icon.svg")),
        "grok" => Some(include_bytes!("../assets/providers/grok.svg")),
        _ => None,
    };
    match bytes {
        Some(bytes) => svg()
            .data(bytes)
            .size(size)
            .flex_shrink_0()
            .text_color(color)
            .into_any_element(),
        None => div()
            .size(size)
            .flex_shrink_0()
            .text_color(color)
            .text_size(size * 0.8)
            .child(crate::ui::initials(driver))
            .into_any_element(),
    }
}
