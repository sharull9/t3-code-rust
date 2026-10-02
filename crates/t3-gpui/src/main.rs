mod app;
mod attachments;
mod backend;
mod directory_picker;
mod drafts;
mod fonts;
mod limits_view;
mod managed_server;
mod mentions;
mod model_picker;
mod prefs;
mod provider_logo;
mod project_picker;
mod settings;
mod sidebar;
mod thread_view;
mod transcript;
mod ui;
mod usage;
mod user_input;
mod workspace;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    gpui_kit::application()
        // The complete Lucide catalog, for icons beyond the component defaults.
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            fonts::register(cx);
            // Must run before any component-backed view is created.
            gpui_kit::init(cx);
            app::init(cx);
            project_picker::init(cx);
            user_input::init(cx);
            model_picker::init(cx);
            let prefs = prefs::Prefs::load();
            ui::apply_theme(prefs.light_theme, cx);
            cx.set_global(prefs);

            let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(720.), px(480.))),
                // The app draws its own title bar (see `T3App::render_title_bar`).
                ..TitleBar::window_options()
            };
            // Wraps the view in `Root`, which hosts dialogs, notifications and menus.
            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title("Rust code");
                cx.new(|cx| app::T3App::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
