//! An image shown full size in a dialog, for attachment thumbnails.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use crate::ui::icon;

/// Opens `image` in a dialog sized to the window. `open_externally`, when
/// given, adds a button that hands the file to the system instead.
pub fn open(
    window: &mut Window,
    cx: &mut App,
    image: Arc<Image>,
    name: SharedString,
    open_externally: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let viewport = window.viewport_size();
        let width = (viewport.width * 0.8).min(px(1200.));
        let image_height = viewport.height * 0.7;
        let theme = cx.theme();
        let external = open_externally.clone().map(|open| {
            Button::new("image-viewer-open-externally")
                .ghost()
                .small()
                .icon(icon(IconName::ExternalLink))
                .label("Open in browser")
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    open(window, cx);
                })
        });
        dialog.width(width).overlay_closable(true).title(name.clone()).child(
            v_flex()
                .id("image-viewer")
                .gap_3()
                .child(
                    div()
                        .flex()
                        .justify_center()
                        .w_full()
                        .h(image_height)
                        .rounded_lg()
                        .overflow_hidden()
                        .bg(theme.muted)
                        .child(img(image.clone()).size_full().object_fit(ObjectFit::Contain)),
                )
                .children(external.map(|button| h_flex().justify_end().child(button))),
        )
    });
}
