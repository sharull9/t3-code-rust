//! Shared look: the dark theme, icons and small formatting helpers.

use gpui_kit::assets::IconName;
use gpui_kit::component::theme::ThemeTokens;
use gpui_kit::component::{Icon, Sizable as _, Size, StyledExt as _, Theme, ThemeMode};
use gpui_kit::*;

pub const SIDEBAR_WIDTH: Pixels = px(300.);
/// Width of the transcript and composer column.
pub const CONTENT_WIDTH: Pixels = px(780.);

/// Surface and accent colors for one appearance. Every token the app or its
/// gpui-kit components read is derived from these, so buttons, tabs, menus,
/// focus rings and selections all share the copper accent instead of falling
/// back to the stock theme's white.
struct Palette {
    background: u32,
    foreground: u32,
    muted_foreground: u32,
    border: u32,
    sidebar: u32,
    sidebar_border: u32,
    raised: u32,
    raised_hover: u32,
    raised_active: u32,
    popover: u32,
    accent: u32,
    accent_hover: u32,
    accent_active: u32,
    accent_foreground: u32,
    link: u32,
    info: u32,
    success: u32,
    warning: u32,
    danger: u32,
}

/// Near-black surfaces with a copper accent.
const DARK: Palette = Palette {
    background: 0x0a0a0b,
    foreground: 0xe8e8ea,
    muted_foreground: 0x8b8b93,
    border: 0x232327,
    sidebar: 0x0f0f11,
    sidebar_border: 0x1c1c20,
    raised: 0x141417,
    raised_hover: 0x1c1c20,
    raised_active: 0x232327,
    popover: 0x121215,
    accent: 0xd49a6a,
    accent_hover: 0xe3ad80,
    accent_active: 0xc4895a,
    accent_foreground: 0x15100d,
    link: 0xe3ad80,
    info: 0x7fb3d5,
    success: 0x4fae7d,
    warning: 0xe0a84a,
    danger: 0xe5534b,
};

/// Warm paper surfaces with a deeper copper, so the accent keeps contrast.
const LIGHT: Palette = Palette {
    background: 0xf7f5f1,
    foreground: 0x1c1a17,
    muted_foreground: 0x6f6a62,
    border: 0xe2ddd5,
    sidebar: 0xefece6,
    sidebar_border: 0xe2ddd5,
    raised: 0xebe7e0,
    raised_hover: 0xe2ddd5,
    raised_active: 0xd8d2c8,
    popover: 0xfbfaf7,
    accent: 0xa9663a,
    accent_hover: 0x96582f,
    accent_active: 0x834c27,
    accent_foreground: 0xfffaf5,
    link: 0x96582f,
    info: 0x2f6f9a,
    success: 0x2f8a5b,
    warning: 0xa86d12,
    danger: 0xc23a33,
};

/// Applies the Rust code look in the dark or light appearance.
pub fn apply_theme(light: bool, cx: &mut App) {
    let (mode, p) = if light { (ThemeMode::Light, &LIGHT) } else { (ThemeMode::Dark, &DARK) };
    Theme::change(mode, None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = "Geist".into();
    theme.mono_font_family = "Geist Mono".into();
    let accent = hex(p.accent);
    theme.background = hex(p.background);
    theme.foreground = hex(p.foreground);
    theme.muted = hex(p.raised);
    theme.muted_foreground = hex(p.muted_foreground);
    theme.border = hex(p.border);
    theme.input = hex(p.raised_active);
    theme.title_bar = hex(p.background);
    theme.title_bar_border = hex(p.background);
    theme.sidebar = hex(p.sidebar);
    theme.sidebar_foreground = hex(p.foreground);
    theme.sidebar_border = hex(p.sidebar_border);
    theme.sidebar_accent = hex(p.raised_hover);
    theme.sidebar_accent_foreground = hex(p.foreground);
    theme.sidebar_primary = accent;
    theme.sidebar_primary_foreground = hex(p.accent_foreground);
    theme.colors.list = hex(p.background);
    theme.list_hover = hex(p.raised);
    theme.list_active = accent.opacity(0.14);
    theme.list_active_border = accent.opacity(0.6);
    theme.secondary = hex(p.raised);
    theme.secondary_hover = hex(p.raised_hover);
    theme.secondary_active = hex(p.raised_active);
    theme.secondary_foreground = hex(p.foreground);
    theme.accent = hex(p.raised_hover);
    theme.accent_foreground = hex(p.foreground);
    theme.popover = hex(p.popover);
    theme.popover_foreground = hex(p.foreground);
    theme.primary = accent;
    theme.primary_hover = hex(p.accent_hover);
    theme.primary_active = hex(p.accent_active);
    theme.primary_foreground = hex(p.accent_foreground);
    theme.button = hex(p.raised);
    theme.button_hover = hex(p.raised_hover);
    theme.button_active = hex(p.raised_active);
    theme.button_foreground = hex(p.foreground);
    theme.button_primary = accent;
    theme.button_primary_hover = hex(p.accent_hover);
    theme.button_primary_active = hex(p.accent_active);
    theme.button_primary_foreground = hex(p.accent_foreground);
    theme.button_secondary = hex(p.raised);
    theme.button_secondary_hover = hex(p.raised_hover);
    theme.button_secondary_active = hex(p.raised_active);
    theme.button_secondary_foreground = hex(p.foreground);
    theme.tab_bar = hex(p.sidebar);
    // The active segment paints with `background`, so the track sits a step above it.
    theme.tab_bar_segmented = hex(p.raised_active);
    theme.tab = hex(p.raised);
    theme.tab_foreground = hex(p.muted_foreground);
    theme.tab_active = hex(p.raised_active);
    theme.tab_active_foreground = hex(p.foreground);
    theme.ring = accent.opacity(0.55);
    theme.caret = accent;
    theme.selection = accent.opacity(0.28);
    theme.link = hex(p.link);
    theme.link_hover = hex(p.accent_hover);
    theme.link_active = hex(p.accent_active);
    theme.scrollbar = hex(p.background).opacity(0.);
    theme.scrollbar_thumb = hex(p.raised_active);
    theme.scrollbar_thumb_hover = hex(p.muted_foreground).opacity(0.5);
    theme.info = hex(p.info);
    theme.success = hex(p.success);
    theme.warning = hex(p.warning);
    theme.danger = hex(p.danger);
    // Components such as `Button` paint from the resolved tokens, which
    // `Theme::change` derived from the stock palette; rebuild them from ours.
    theme.tokens = ThemeTokens::from(&theme.colors);
    cx.refresh_windows();
}

/// A thread title fit for a single line: the first non-empty line, without a
/// leading Markdown heading marker. Titles often start as a pasted prompt.
pub fn display_title(title: &str) -> String {
    let line = title.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let line = line.trim_start_matches('#').trim_start();
    if line.is_empty() { "Untitled thread".into() } else { line.to_owned() }
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
    let words: Vec<&str> =
        title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
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

/// Time since `timestamp` as "42s", "7m" or "2h 5m", for running work.
pub fn elapsed(timestamp: &str) -> Option<String> {
    let then = chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
    let seconds = (chrono::Utc::now() - then.to_utc()).num_seconds().max(0);
    Some(match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        _ => format!("{}h {}m", seconds / 3_600, seconds % 3_600 / 60),
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
            chars
                .next()
                .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
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
    use super::{display_title, initials};

    #[test]
    fn display_titles_are_single_line_without_heading_markers() {
        assert_eq!(display_title("Polish the UI\ncheck that all features work"), "Polish the UI");
        assert_eq!(display_title("\n\n  # Plan: Vercel Workflow  "), "Plan: Vercel Workflow");
        assert_eq!(display_title("### "), "Untitled thread");
        assert_eq!(display_title("For actions"), "For actions");
    }

    #[test]
    fn initials_from_titles() {
        assert_eq!(initials("t3-code-gpui"), "TC");
        assert_eq!(initials("construction-erp"), "CE");
        assert_eq!(initials("zed"), "ZE");
        assert_eq!(initials(""), "");
    }
}
