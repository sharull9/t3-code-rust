mod app;
mod backend;
mod thread_view;

use gpui_kit::*;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            // Must run before any component-backed view is created.
            gpui_kit::init(cx);

            let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("T3 Code".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            // Wraps the view in `Root`, which hosts dialogs, notifications and menus.
            gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::T3App::new(window, cx)))
                .expect("failed to open window");
            cx.activate(true);
        });
}
