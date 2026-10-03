//! `@` file and `$` skill mentions in the composer.
//!
//! Typing `@` or `$` at the start of a word opens a suggestion menu. Picking a
//! suggestion replaces the typed word with an atomic inline token whose text
//! is upstream's wire format (`@path`, `@"path with spaces"`, `$skill-name`),
//! so the server and other clients read the prompt exactly as T3's composer
//! writes it. See `packages/shared/src/composerInlineTokens.ts`.

use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InlineToken, InlineTokenContext, InputContent, InputToken};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{ProviderSkill, WorkspaceEntry};

use crate::ui::icon;

pub const FILE_RESULT_LIMIT: u32 = 40;
const SKILL_RESULT_LIMIT: usize = 50;
const FILE_TOKEN: &str = "file:";
const SKILL_TOKEN: &str = "skill:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionKind {
    File,
    Skill,
}

/// The word being typed at the cursor, `@query` or `$query`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub kind: MentionKind,
    /// Byte range of the word including its sigil.
    pub range: Range<usize>,
    pub query: String,
}

/// The mention being typed when the cursor ends a word that starts with `@`
/// or `$` at the start of the text or after whitespace.
pub fn detect_trigger(text: &str, cursor: usize) -> Option<Trigger> {
    let before = text.get(..cursor)?;
    let start = before.rfind(char::is_whitespace).map_or(0, |ix| {
        ix + before[ix..].chars().next().map_or(1, char::len_utf8)
    });
    let word = &before[start..];
    let kind = match word.chars().next()? {
        '@' => MentionKind::File,
        '$' => MentionKind::Skill,
        _ => return None,
    };
    let query = &word[1..];
    // A second sigil means this is prose such as an email address.
    if query.contains(['@', '$', '"']) {
        return None;
    }
    // `$20` and similar stay prose, as upstream's skill grammar requires.
    if kind == MentionKind::Skill && query.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(Trigger { kind, range: start..cursor, query: query.to_owned() })
}

/// `@path`, quoted when the path has whitespace or quotes.
pub fn file_token_text(path: &str) -> String {
    if path.contains(|c: char| c.is_whitespace() || c == '"' || c == '@') {
        format!("@\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("@{path}")
    }
}

pub fn file_token(path: &str) -> InlineToken {
    InlineToken::new(format!("{FILE_TOKEN}{path}"), file_token_text(path)).with_label(basename(path))
}

pub fn skill_token(name: &str, label: &str) -> InlineToken {
    InlineToken::new(format!("{SKILL_TOKEN}{name}"), format!("${name}")).with_label(label.to_owned())
}

pub fn basename(path: &str) -> &str {
    let path = path.trim_end_matches(['/', '\\']);
    path.rsplit(['/', '\\']).next().filter(|name| !name.is_empty()).unwrap_or(path)
}

fn parent(path: &str) -> &str {
    let path = path.trim_end_matches(['/', '\\']);
    path.rfind(['/', '\\']).map_or("", |ix| &path[..ix])
}

/// Turns the mentions in saved text back into tokens, so a restored draft
/// shows chips again. A token must be followed by whitespace, as upstream
/// requires; skills are only chipped when `skill_label` knows them.
pub fn tokenize(text: &str, skill_label: impl Fn(&str) -> Option<String>) -> InputContent {
    let mut content = InputContent::new(text.to_owned());
    let mut offset = 0;
    while offset < text.len() {
        let rest = &text[offset..];
        let at_word_start = offset == 0 || text[..offset].ends_with(char::is_whitespace);
        let next = rest.chars().next().map_or(1, char::len_utf8);
        let parsed = at_word_start.then(|| parse_token(rest, &skill_label)).flatten();
        match parsed {
            Some((len, token)) if text[offset + len..].starts_with(char::is_whitespace) => {
                if let Ok(next_content) = content.clone().with_token(offset..offset + len, token) {
                    content = next_content;
                }
                offset += len;
            }
            _ => offset += next,
        }
    }
    content
}

fn parse_token(
    rest: &str,
    skill_label: &impl Fn(&str) -> Option<String>,
) -> Option<(usize, InlineToken)> {
    if let Some(body) = rest.strip_prefix("@\"") {
        let mut path = String::new();
        let mut chars = body.char_indices();
        while let Some((ix, c)) = chars.next() {
            match c {
                '\\' => path.push(chars.next()?.1),
                '"' => return Some((ix + 3, file_token(&path))).filter(|_| !path.is_empty()),
                _ => path.push(c),
            }
        }
        return None;
    }
    if let Some(body) = rest.strip_prefix('@') {
        let len = body.find(|c: char| c.is_whitespace() || c == '@' || c == '"').unwrap_or(body.len());
        let path = &body[..len];
        return (!path.is_empty()).then(|| (len + 1, file_token(path)));
    }
    let body = rest.strip_prefix('$')?;
    let len = body
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-')))
        .unwrap_or(body.len());
    let name = &body[..len];
    let label = skill_label(name).filter(|_| !name.is_empty())?;
    Some((len + 1, skill_token(name, &label)))
}

/// Mirrors `formatProviderSkillSourceKind`, as shown beside each skill.
pub fn skill_scope_label(skill: &ProviderSkill) -> String {
    match skill.scope.as_deref().map(str::to_lowercase).as_deref() {
        None | Some("") => "Skill".into(),
        Some("user" | "personal" | "global") => "Personal Skill".into(),
        Some("project" | "workspace" | "repo" | "repository") => "Project Skill".into(),
        Some("system" | "builtin" | "bundled") => "System Skill".into(),
        Some("plugin") => "Plugin Skill".into(),
        Some(other) => {
            let mut chars = other.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
            format!("{first}{} Skill", chars.as_str())
        }
    }
}

/// Skills matching `query`, best first: name prefix, label prefix, then any
/// match in the name, label or description.
pub fn search_skills(skills: &[&ProviderSkill], query: &str) -> Vec<Suggestion> {
    let query = query.trim().to_lowercase();
    let mut ranked: Vec<(usize, Suggestion)> = skills
        .iter()
        .filter_map(|skill| {
            let name = skill.name.to_lowercase();
            let label = skill.label();
            let label_lower = label.to_lowercase();
            let summary = skill.summary().unwrap_or_default().to_lowercase();
            let rank = if query.is_empty() || name == query {
                0
            } else if name.starts_with(&query) {
                1
            } else if label_lower.starts_with(&query) {
                2
            } else if name.contains(&query) || label_lower.contains(&query) {
                3
            } else if summary.contains(&query) {
                4
            } else {
                return None;
            };
            Some((
                rank,
                Suggestion {
                    token: skill_token(&skill.name, &label),
                    title: label.into(),
                    detail: skill.summary().map(|text| text.to_owned().into()),
                    badge: Some(skill_scope_label(skill).into()),
                    kind: SuggestionKind::Skill,
                },
            ))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().take(SKILL_RESULT_LIMIT).map(|(_, suggestion)| suggestion).collect()
}

pub fn file_suggestions(entries: &[WorkspaceEntry]) -> Vec<Suggestion> {
    entries
        .iter()
        .filter(|entry| !entry.path.trim().is_empty())
        .map(|entry| {
            let directory = entry.kind == "directory";
            Suggestion {
                token: file_token(&entry.path),
                title: basename(&entry.path).to_owned().into(),
                detail: Some(parent(&entry.path).to_owned().into()).filter(|p: &SharedString| !p.is_empty()),
                badge: None,
                kind: if directory {
                    SuggestionKind::Directory
                } else {
                    SuggestionKind::File(extension(&entry.path))
                },
            }
        })
        .collect()
}

fn extension(path: &str) -> Option<SharedString> {
    let name = basename(path);
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty() && ext.len() <= 4).then(|| ext.to_lowercase().into())
}

fn is_image(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico")
}

#[derive(Debug, Clone, PartialEq)]
pub enum SuggestionKind {
    File(Option<SharedString>),
    Directory,
    Skill,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub token: InlineToken,
    pub title: SharedString,
    pub detail: Option<SharedString>,
    pub badge: Option<SharedString>,
    pub kind: SuggestionKind,
}

/// The open suggestion menu.
pub struct MentionMenu {
    pub trigger: Trigger,
    pub items: Vec<Suggestion>,
    pub highlighted: usize,
    /// The file search this menu is waiting for, if any.
    pub pending_request: Option<u64>,
    pub error: Option<SharedString>,
}

impl MentionMenu {
    pub fn new(trigger: Trigger) -> Self {
        Self { trigger, items: Vec::new(), highlighted: 0, pending_request: None, error: None }
    }

    pub fn move_highlight(&mut self, delta: isize) {
        if !self.items.is_empty() {
            let len = self.items.len() as isize;
            self.highlighted = (self.highlighted as isize + delta).rem_euclid(len) as usize;
        }
    }
}

/// A badge like the file-type marks in T3's file picker.
fn kind_mark(kind: &SuggestionKind, cx: &App) -> AnyElement {
    let theme = cx.theme();
    match kind {
        SuggestionKind::Directory => {
            icon(IconName::FolderClosed).xsmall().text_color(theme.muted_foreground).into_any_element()
        }
        SuggestionKind::Skill => icon(IconName::Box).xsmall().text_color(theme.muted_foreground).into_any_element(),
        SuggestionKind::File(Some(ext)) if is_image(ext) => {
            icon(IconName::Image).xsmall().text_color(theme.muted_foreground).into_any_element()
        }
        SuggestionKind::File(Some(ext)) => {
            let text: String = ext.chars().take(3).collect::<String>().to_uppercase();
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .w(px(18.))
                .h(px(14.))
                .rounded(px(3.))
                .bg(crate::ui::project_color(ext).opacity(0.85))
                .text_color(crate::ui::hex(0xffffff))
                .text_size(px(7.))
                .font_bold()
                .child(text)
                .into_any_element()
        }
        SuggestionKind::File(None) => {
            icon(IconName::File).xsmall().text_color(theme.muted_foreground).into_any_element()
        }
    }
}

/// The menu's rows; the caller places it above the composer and routes
/// clicks to `on_choose`.
pub fn render_menu(
    menu: &MentionMenu,
    on_choose: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let empty_text = match (&menu.error, menu.pending_request, menu.trigger.kind) {
        (Some(error), _, _) => Some(error.clone()),
        (None, Some(_), _) if menu.items.is_empty() => Some("Searching…".into()),
        (None, None, MentionKind::File) if menu.items.is_empty() => Some("No matching files".into()),
        (None, None, MentionKind::Skill) if menu.items.is_empty() => {
            Some("No skills for this provider".into())
        }
        _ => None,
    };
    let rows = menu.items.iter().enumerate().map(|(ix, item)| {
        let highlighted = ix == menu.highlighted;
        let on_choose = on_choose.clone();
        h_flex()
            .id(("mention-row", ix))
            .test_support()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .when(highlighted, |row| row.bg(theme.secondary_hover))
            .when(!highlighted, |row| row.hover(|style| style.bg(theme.list_hover)))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| on_choose(ix, window, cx))
            .child(kind_mark(&item.kind, cx))
            .child(div().flex_shrink_0().font_semibold().child(item.title.clone()))
            .children(item.detail.clone().map(|detail| {
                div().flex_1().min_w_0().truncate().text_color(theme.muted_foreground).child(detail)
            }))
            .when(item.detail.is_none(), |row| row.child(div().flex_1()))
            .children(item.badge.clone().map(|badge| {
                h_flex()
                    .flex_shrink_0()
                    .gap_1()
                    .px_1p5()
                    .py_0p5()
                    .rounded_md()
                    .bg(theme.secondary)
                    .text_xs()
                    .font_semibold()
                    .child(icon(IconName::User).xsmall().text_color(theme.muted_foreground))
                    .child(badge)
            }))
    });
    v_flex()
        .id("mention-menu")
        .test_support()
        .w_full()
        .max_h(px(280.))
        .p_1()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg()
        .overflow_y_scrollbar()
        .children(rows)
        .children(empty_text.map(|text| {
            div().px_2().py_2().text_sm().text_color(theme.muted_foreground).child(text)
        }))
}

/// How a mention token looks inside the composer: a tinted chip with a file
/// type or skill icon.
pub fn render_token(context: &InlineTokenContext, _: &mut Window, cx: &mut App) -> InputToken {
    let id = context.token().id();
    let skill = id.starts_with(SKILL_TOKEN);
    let path = id.strip_prefix(FILE_TOKEN).unwrap_or_default();
    let ext = extension(path);
    let mark = if skill {
        IconName::Box
    } else if ext.as_deref().is_some_and(is_image) {
        IconName::Image
    } else if path.ends_with(['/', '\\']) || ext.is_none() {
        IconName::FileText
    } else {
        IconName::FileCode
    };
    let tint = if skill { cx.theme().magenta } else { cx.theme().cyan };
    InputToken::new(context)
        .icon(mark)
        .font_semibold()
        .when(!context.is_selected(), |token| {
            token.bg(tint.opacity(0.14)).border_color(tint.opacity(0.4)).text_color(tint)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn triggers_start_words_and_skip_prose() {
        let at = detect_trigger("see @src/ma", 11).unwrap();
        assert_eq!((at.kind, at.range.clone(), at.query.as_str()), (MentionKind::File, 4..11, "src/ma"));
        let skill = detect_trigger("$Repo", 5).unwrap();
        assert_eq!((skill.kind, skill.query.as_str()), (MentionKind::Skill, "Repo"));
        assert_eq!(detect_trigger("@", 1).unwrap().query, "");
        assert!(detect_trigger("mail a@b.com", 12).is_none());
        assert!(detect_trigger("costs $20", 9).is_none());
        assert!(detect_trigger("@src done", 9).is_none(), "the cursor left the word");
    }

    #[test]
    fn tokens_use_the_upstream_wire_format() {
        assert_eq!(file_token_text("src/api/index.ts"), "@src/api/index.ts");
        assert_eq!(file_token_text("docs/my notes.md"), "@\"docs/my notes.md\"");
        assert_eq!(file_token("src/api/index.ts").label().as_ref(), "index.ts");
        assert_eq!(skill_token("repo-explorer", "Repo Explorer").text().as_ref(), "$repo-explorer");
    }

    #[test]
    fn restored_text_gets_its_chips_back() {
        let text = "look at @src/a.ts and @\"my file.md\" with $fallow please $20 $unknown ";
        let content = tokenize(text, |name| (name == "fallow").then(|| "Fallow".to_owned()));
        let labels: Vec<_> = content.tokens().iter().map(|span| span.token().label().to_string()).collect();
        assert_eq!(labels, ["a.ts", "my file.md", "Fallow"]);
        assert_eq!(content.text().as_ref(), text);
        let trailing = tokenize("@src/a.ts", |_| None);
        assert!(trailing.tokens().is_empty(), "a token must be followed by whitespace");
    }

    #[test]
    fn skills_rank_name_prefixes_first() {
        let skills: Vec<ProviderSkill> = serde_json::from_value(serde_json::json!([
            {"name":"turborepo","description":"Repo build system","enabled":true,"scope":"user"},
            {"name":"repo-explorer","enabled":true},
        ]))
        .unwrap();
        let refs: Vec<&ProviderSkill> = skills.iter().collect();
        let titles: Vec<_> = search_skills(&refs, "repo").into_iter().map(|s| s.title.to_string()).collect();
        assert_eq!(titles, ["Repo Explorer", "Turborepo"]);
        assert_eq!(skill_scope_label(&skills[0]), "Personal Skill");
    }
}
