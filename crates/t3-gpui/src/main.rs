mod app;
mod backend;
mod project_picker;
mod sidebar;
mod thread_view;
mod transcript;
mod ui;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    gpui_kit::application()
        // The complete Lucide catalog, for icons beyond the component defaults.
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            // Must run before any component-backed view is created.
            gpui_kit::init(cx);
            project_picker::init(cx);
            ui::apply_theme(cx);

            let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(720.), px(480.))),
                // The app draws its own title bar (see `T3App::render_title_bar`).
                ..TitleBar::window_options()
            };
            // Wraps the view in `Root`, which hosts dialogs, notifications and menus.
            gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::T3App::new(window, cx)))
                .expect("failed to open window");
            cx.activate(true);
        });
}
