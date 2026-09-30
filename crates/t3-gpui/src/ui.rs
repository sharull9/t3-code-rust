//! Shared look: the dark theme, icons and small formatting helpers.

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable as _, Size, StyledExt as _, Theme, ThemeMode};
use gpui_kit::*;

pub const SIDEBAR_WIDTH: Pixels = px(300.);
/// Width of the transcript and composer column.
pub const CONTENT_WIDTH: Pixels = px(780.);

/// Near-black surfaces with a violet accent, after the T3 Code desktop app.
pub fn apply_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let theme = Theme::global_mut(cx);
    theme.background = hex(0x0a0a0b);
    theme.foreground = hex(0xe8e8ea);
    theme.muted_foreground = hex(0x8b8b93);
    theme.border = hex(0x232327);
    theme.title_bar = hex(0x0a0a0b);
    theme.title_bar_border = hex(0x0a0a0b);
    theme.sidebar = hex(0x0f0f11);
    theme.sidebar_border = hex(0x1c1c20);
    theme.sidebar_accent = hex(0x1e1e23);
    theme.list_hover = hex(0x17171a);
    theme.secondary = hex(0x141417);
    theme.secondary_hover = hex(0x1c1c20);
    theme.primary = hex(0x5b4bdb);
    theme.primary_hover = hex(0x6a5be6);
    theme.primary_foreground = hex(0xffffff);
}

pub fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

pub fn icon(name: IconName) -> Icon {
    Icon::new(name)
}

/// A working indicator, as Zed draws it: a linear two-second rotation shared by
/// every loader on screen. `Spinner` eases each 0.8 s turn, so a dropped frame
/// shows as a visible jump.
pub fn loader(id: impl Into<ElementId>, size: Size) -> impl IntoElement {
    Icon::new(IconName::LoaderCircle).with_size(size).with_animation(
        id,
        Animation::new(std::time::Duration::from_secs(2)).repeat_synced(),
        |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
    )
}

/// A stable color per project, for its tag in the sidebar.
pub fn project_color(project_id: &str) -> Hsla {
    const COLORS: [u32; 6] = [0xe5484d, 0x3e9b6c, 0x3b82f6, 0xd97706, 0x8b5cf6, 0x0ea5a4];
    let hash = project_id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    hex(COLORS[hash as usize % COLORS.len()])
}

/// The small colored initials square that identifies a project.
pub fn project_tag(project_id: &str, title: &str) -> impl IntoElement {
    let color = project_color(project_id);
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(16.))
        .rounded(px(4.))
        .bg(color.opacity(0.18))
        .text_color(color)
        .text_size(px(8.))
        .font_bold()
        .child(initials(title))
}

/// Up to two letters: the initials of the first two words, or the first two letters.
pub fn initials(title: &str) -> String {
    let words: Vec<&str> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let letters: String = match words.as_slice() {
        [] => String::new(),
        [only] => only.chars().take(2).collect(),
        [first, second, ..] => first.chars().take(1).chain(second.chars().take(1)).collect(),
    };
    letters.to_uppercase()
}

/// "now", "4m", "6h", "2d", "3w" since an RFC 3339 timestamp.
pub fn relative_time(timestamp: &str) -> Option<String> {
    let then = chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
    let seconds = (chrono::Utc::now() - then.to_utc()).num_seconds().max(0);
    Some(match seconds {
        0..60 => "now".into(),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        86_400..604_800 => format!("{}d", seconds / 86_400),
        _ => format!("{}w", seconds / 604_800),
    })
}

/// Display name for a provider id such as "claudeAgent".
pub fn provider_label(provider: Option<&str>) -> String {
    match provider {
        None | Some("") => "Agent".into(),
        Some("claudeAgent") => "Claude".into(),
        Some("codex") => "Codex".into(),
        Some(other) => {
            let mut chars = other.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        }
    }
}

/// Label and icon for a `RuntimeMode`.
pub fn runtime_mode(mode: &str) -> (&'static str, IconName) {
    match mode {
        "approval-required" => ("Ask first", IconName::Lock),
        "auto-accept-edits" => ("Accept edits", IconName::LockKeyholeOpen),
        "auto" => ("Auto", IconName::LockOpen),
        _ => ("Full access", IconName::LockOpen),
    }
}

/// Label and icon for a `ProviderInteractionMode`.
pub fn interaction_mode(mode: &str) -> (&'static str, IconName) {
    match mode {
        "plan" => ("Plan", IconName::Map),
        _ => ("Build", IconName::Hammer),
    }
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn initials_from_titles() {
        assert_eq!(initials("t3-code-gpui"), "TC");
        assert_eq!(initials("construction-erp"), "CE");
        assert_eq!(initials("zed"), "ZE");
        assert_eq!(initials(""), "");
    }
}
