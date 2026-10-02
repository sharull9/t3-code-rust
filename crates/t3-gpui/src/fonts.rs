//! Embedded OFL fonts: no system installation or network access at runtime.
use gpui_kit::App;
use std::borrow::Cow;

pub fn register(cx: &App) {
    macro_rules! font {
        ($name:literal) => {
            Cow::Borrowed(include_bytes!(concat!("../assets/fonts/", $name)) as &'static [u8])
        };
    }
    cx.text_system()
        .add_fonts(vec![
            font!("Geist-Regular.ttf"),
            font!("Geist-Medium.ttf"),
            font!("Geist-SemiBold.ttf"),
            font!("Geist-Bold.ttf"),
            font!("Geist-Italic.ttf"),
            font!("Geist-BoldItalic.ttf"),
            font!("GeistMono-Regular.ttf"),
            font!("GeistMono-Medium.ttf"),
            font!("GeistMono-SemiBold.ttf"),
            font!("GeistMono-Bold.ttf"),
            font!("GeistMono-Italic.ttf"),
            font!("GeistMono-BoldItalic.ttf"),
        ])
        .expect("failed to register bundled Geist fonts");
}
