//! Storage: automatic cleanup of worktrees, browser artifacts and logs.
//!
//! At "All projects" the rules live in the environment's `storageCleanup`.
//! At a project, `worktreeCleanup` overrides them: inherit (no override),
//! `{ mode: "off" }`, or `{ mode: "custom", rules }` with all four rules.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};

use crate::settings::row::{SettingRow, SettingsGroup, choice_button, server_row};
use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsPage};

/// Every server setting on this page, for "Restore defaults".
pub const KEYS: &[&str] = &["storageCleanup", "worktreeCleanup"];

pub const SEARCH: &[SearchEntry] = &[
    SearchEntry {
        title: "Automatic worktree cleanup",
        description: "Whether this project uses the environment's worktree cleanup rules, its own, or none.",
        keywords: &["project", "inherit", "off", "custom", "worktrees"],
        section: Section::Storage,
    },
    SearchEntry {
        title: "Delete worktrees with deleted threads",
        description: "Remove unused worktrees when active or archived threads are deleted.",
        keywords: &["cleanup", "remove", "disk", "storage"],
        section: Section::Storage,
    },
    SearchEntry {
        title: "Delete inactive worktrees",
        description: "Remove worktrees after their threads have been inactive for this many days.",
        keywords: &["cleanup", "days", "retention", "stale"],
        section: Section::Storage,
    },
    SearchEntry {
        title: "Delete merged worktrees",
        description: "Remove worktrees whose pull request is merged and whose commits are in the default branch.",
        keywords: &["cleanup", "pull request", "merge"],
        section: Section::Storage,
    },
    SearchEntry {
        title: "Delete unchanged worktrees",
        description: "Remove worktrees with no commits beyond the default branch.",
        keywords: &["cleanup", "empty", "no commits"],
        section: Section::Storage,
    },
    SearchEntry {
        title: "Delete old browser artifacts",
        description: "Delete saved browser captures after this many days.",
        keywords: &["screenshots", "recordings", "captures", "retention", "disk"],
        section: Section::Storage,
    },
    SearchEntry {
        title: "Delete old rotated logs",
        description: "Delete inactive rotated log files after this many days.",
        keywords: &["log files", "retention", "disk", "diagnostics"],
        section: Section::Storage,
    },
];

const DAYS_FIELDS: [&str; 3] = ["worktreeAfterDays", "browserArtifactsAfterDays", "logsAfterDays"];
const DEFAULT_RETENTION_DAYS: u64 = 30;
const MIN_DAYS: u64 = 1;
const MAX_DAYS: u64 = 3650;

/// The retention day inputs and which of them last held an invalid number.
pub struct State {
    inputs: Vec<(&'static str, Entity<InputState>)>,
    invalid: Vec<&'static str>,
}

impl State {
    pub fn new(
        window: &mut Window,
        subscriptions: &mut Vec<Subscription>,
        cx: &mut Context<SettingsPage>,
    ) -> Self {
        let mut inputs = Vec::new();
        for field in DAYS_FIELDS {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Days"));
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |page, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        commit_days(page, field, window, cx);
                    }
                },
            ));
            inputs.push((field, input));
        }
        Self { inputs, invalid: Vec::new() }
    }

    fn input(&self, field: &str) -> &Entity<InputState> {
        &self.inputs.iter().find(|(name, _)| *name == field).expect("known field").1
    }
}

/// The four worktree rules in effect at the current scope.
#[derive(Debug, Clone, PartialEq)]
struct Rules {
    after_days: Option<u64>,
    on_merge: bool,
    on_delete: bool,
    unchanged: bool,
}

impl Rules {
    fn from_value(value: &Value) -> Self {
        Self {
            after_days: value.get("worktreeAfterDays").and_then(Value::as_u64),
            on_merge: value["worktreeOnMerge"].as_bool().unwrap_or(false),
            on_delete: value["worktreeOnDelete"].as_bool().unwrap_or(false),
            unchanged: value["worktreeUnchanged"].as_bool().unwrap_or(false),
        }
    }
    fn to_value(&self) -> Value {
        json!({
            "worktreeAfterDays": self.after_days,
            "worktreeOnMerge": self.on_merge,
            "worktreeOnDelete": self.on_delete,
            "worktreeUnchanged": self.unchanged,
        })
    }
    fn set(&mut self, field: &str, value: &Value) {
        match field {
            "worktreeAfterDays" => self.after_days = value.as_u64(),
            "worktreeOnMerge" => self.on_merge = value.as_bool().unwrap_or(false),
            "worktreeOnDelete" => self.on_delete = value.as_bool().unwrap_or(false),
            "worktreeUnchanged" => self.unchanged = value.as_bool().unwrap_or(false),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Inherit,
    Off,
    Custom,
}

/// The project's cleanup mode; `Inherit` at All projects too.
fn mode(page: &SettingsPage) -> Mode {
    if page.project_scope().is_none() {
        return Mode::Inherit;
    }
    let cleanup = page.server_value("worktreeCleanup");
    match cleanup.value["mode"].as_str() {
        Some("off") if cleanup.overridden => Mode::Off,
        Some("custom") if cleanup.overridden => Mode::Custom,
        _ => Mode::Inherit,
    }
}

fn environment_rules(page: &SettingsPage) -> Rules {
    let mut stored = t3_client::settings::default_value("storageCleanup").unwrap_or(Value::Null);
    if let (Some(stored), Some(current)) =
        (stored.as_object_mut(), page.server_value("storageCleanup").value.as_object())
    {
        stored.extend(current.clone());
    }
    Rules::from_value(&stored)
}

/// The worktree rules in effect: the project's own when it has custom rules.
fn rules(page: &SettingsPage) -> Rules {
    if mode(page) == Mode::Custom {
        Rules::from_value(&page.server_value("worktreeCleanup").value["rules"])
    } else {
        environment_rules(page)
    }
}

/// The key and value that set one rule (or retention field) at the current
/// scope. A project always writes all four rules, starting from the ones it
/// currently shows, so the override stands on its own.
fn rule_edit(page: &SettingsPage, field: &str, value: Value) -> (&'static str, Value) {
    if page.project_scope().is_some() && field.starts_with("worktree") {
        let mut rules = rules(page);
        rules.set(field, &value);
        ("worktreeCleanup", json!({ "mode": "custom", "rules": rules.to_value() }))
    } else {
        ("storageCleanup", json!({ field: value }))
    }
}

fn set_rule(page: &mut SettingsPage, field: &str, value: Value, cx: &mut Context<SettingsPage>) {
    let (key, value) = rule_edit(page, field, value);
    page.set_server_value(key, value, cx);
}

/// The stored retention (days) for a day field.
fn days_of(page: &SettingsPage, field: &str) -> Option<u64> {
    if field == "worktreeAfterDays" {
        return rules(page).after_days;
    }
    page.server_value("storageCleanup").value.get(field).and_then(Value::as_u64)
}

/// Parses the day text: a whole number from 1 to 3650.
fn parse_days(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok().filter(|days| (MIN_DAYS..=MAX_DAYS).contains(days))
}

fn commit_days(
    page: &mut SettingsPage,
    field: &'static str,
    window: &mut Window,
    cx: &mut Context<SettingsPage>,
) {
    if !page.server_ready() || days_of(page, field).is_none() {
        return;
    }
    let text = page.storage.input(field).read(cx).value().to_string();
    match parse_days(&text) {
        Some(days) => {
            page.storage.invalid.retain(|name| *name != field);
            if days_of(page, field) != Some(days) {
                set_rule(page, field, json!(days), cx);
            }
        }
        None => {
            if !page.storage.invalid.contains(&field) {
                page.storage.invalid.push(field);
            }
            // Put the last saved number back.
            let saved = days_of(page, field).map(|days| days.to_string()).unwrap_or_default();
            page.storage.input(field).update(cx, |input, cx| input.set_value(saved, window, cx));
        }
    }
    cx.notify();
}

/// Shows each saved day count in its input unless it is being edited.
pub fn sync_inputs(page: &SettingsPage, window: &mut Window, cx: &mut Context<SettingsPage>) {
    for (field, input) in &page.storage.inputs {
        let saved = days_of(page, field).map(|days| days.to_string()).unwrap_or_default();
        if input.read(cx).focus_handle(cx).is_focused(window) || input.read(cx).value().as_ref() == saved
        {
            continue;
        }
        input.update(cx, |input, cx| input.set_value(saved, window, cx));
    }
}

pub fn modified(page: &SettingsPage, _: &App) -> bool {
    page.server_keys_modified(KEYS)
}

pub fn restore_defaults(page: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    page.reset_server_keys(KEYS, cx);
}

fn notice(text: &'static str, cx: &Context<SettingsPage>) -> AnyElement {
    div()
        .px_4()
        .py_3()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let in_project = page.project_scope().is_some();
    if in_project && !page.supports("projectWorktreeCleanup") {
        return notice("Update the server to configure project worktree cleanup.", cx);
    }
    if !page.supports("storageCleanup") {
        return notice("Update the server to use storage cleanup.", cx);
    }
    let mode = mode(page);
    let rules = rules(page);
    let defaults = Rules::from_value(&t3_client::settings::default_value("storageCleanup").unwrap_or_default());

    let mut worktrees = SettingsGroup::new("Worktrees");
    if in_project {
        let description = match mode {
            Mode::Off => "Keep this project's worktrees until you delete them manually.",
            Mode::Custom => "Use these rules for this project.",
            Mode::Inherit => "Use the environment's worktree cleanup settings.",
        };
        worktrees = worktrees.row(
            server_row(page, "setting-worktree-cleanup", "Automatic worktree cleanup", "worktreeCleanup", cx)
                .description(description)
                .control(mode_control(page, mode, &environment_rules(page), cx)),
        );
    }
    if !in_project || mode == Mode::Custom {
        worktrees = worktrees
            .row(switch_row(
                page,
                "setting-worktree-on-delete",
                "Delete worktrees with deleted threads",
                "Remove unused worktrees when active or archived threads are deleted. Worktrees with local changes are kept.",
                "worktreeOnDelete",
                rules.on_delete,
                rules.on_delete != defaults.on_delete,
                cx,
            ))
            .row(days_row(
                page,
                "setting-worktree-after-days",
                "Delete inactive worktrees",
                "Remove worktrees after their threads have been inactive for this many days. Branches and thread history are kept.",
                "worktreeAfterDays",
                cx,
            ))
            .row(switch_row(
                page,
                "setting-worktree-on-merge",
                "Delete merged worktrees",
                "Remove worktrees whose pull request is merged and whose commits are included in the default branch.",
                "worktreeOnMerge",
                rules.on_merge,
                rules.on_merge != defaults.on_merge,
                cx,
            ))
            .row(switch_row(
                page,
                "setting-worktree-unchanged",
                "Delete unchanged worktrees",
                "Remove worktrees with no commits beyond the default branch.",
                "worktreeUnchanged",
                rules.unchanged,
                rules.unchanged != defaults.unchanged,
                cx,
            ));
    }
    let mut page_content = v_flex().gap_6().child(worktrees);
    if !in_project {
        page_content = page_content.child(
            SettingsGroup::new("Artifacts and logs")
                .row(days_row(
                    page,
                    "setting-browser-artifacts-days",
                    "Delete old browser artifacts",
                    "Delete saved browser captures after this many days. Older capture links will no longer open.",
                    "browserArtifactsAfterDays",
                    cx,
                ))
                .row(days_row(
                    page,
                    "setting-logs-days",
                    "Delete old rotated logs",
                    "Delete inactive rotated log files after this many days. Current logs are kept.",
                    "logsAfterDays",
                    cx,
                )),
        );
    }
    page_content.into_any_element()
}

/// Inherit / Off / Custom. Custom starts from the environment's rules.
fn mode_control(
    page: &SettingsPage,
    current: Mode,
    environment: &Rules,
    cx: &Context<SettingsPage>,
) -> impl IntoElement {
    let label = match current {
        Mode::Inherit => "Inherit",
        Mode::Off => "Off",
        Mode::Custom => "Custom",
    };
    let custom = json!({ "mode": "custom", "rules": environment.to_value() });
    let view = cx.entity();
    choice_button("setting-worktree-cleanup-select", label, !page.server_ready(), move |menu| {
        let choices: [(&str, Mode, Value); 3] = [
            ("Inherit", Mode::Inherit, Value::Null),
            ("Off", Mode::Off, json!({ "mode": "off" })),
            ("Custom", Mode::Custom, custom.clone()),
        ];
        let mut menu = menu;
        for (label, mode, value) in choices {
            let view = view.clone();
            menu = menu.item(PopupMenuItem::new(label).checked(mode == current).on_click(
                move |_, _, cx| {
                    // `null` clears the override (inherit).
                    view.update(cx, |this, cx| this.set_server_value("worktreeCleanup", value.clone(), cx));
                },
            ));
        }
        menu
    })
}

/// A rule row that is not a server key of its own, so it carries its own
/// reset button at All projects (a project resets through the mode row).
fn rule_row(
    page: &SettingsPage,
    id: &'static str,
    title: &'static str,
    description: &'static str,
    field: &'static str,
    modified: bool,
    default: Value,
    cx: &Context<SettingsPage>,
) -> SettingRow {
    let environment = page.project_scope().is_none();
    SettingRow::new(id, title)
        .description(description)
        .modified(environment && page.server_ready() && modified)
        .on_reset(cx.listener(move |this, _, _, cx| set_rule(this, field, default.clone(), cx)))
}

#[allow(clippy::too_many_arguments)]
fn switch_row(
    page: &SettingsPage,
    id: &'static str,
    title: &'static str,
    description: &'static str,
    field: &'static str,
    checked: bool,
    modified: bool,
    cx: &Context<SettingsPage>,
) -> SettingRow {
    rule_row(page, id, title, description, field, modified, json!(false), cx).control(
        Switch::new(SharedString::from(format!("{id}-switch")))
            .checked(checked)
            .disabled(!page.server_ready())
            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                set_rule(this, field, json!(*checked), cx)
            })),
    )
}

/// A retention row: a switch (off is `null`, on starts at 30 days) and, when
/// on, the number of days.
fn days_row(
    page: &SettingsPage,
    id: &'static str,
    title: &'static str,
    description: &'static str,
    field: &'static str,
    cx: &Context<SettingsPage>,
) -> SettingRow {
    let days = days_of(page, field);
    let invalid = page.storage.invalid.contains(&field);
    let description = if invalid {
        format!("{description} Enter a whole number of days from {MIN_DAYS} to {MAX_DAYS}.")
    } else {
        description.to_owned()
    };
    let control = h_flex()
        .gap_2()
        .items_center()
        .when_some(days, |row, _| {
            row.child(
                div().w(px(88.)).child(
                    Input::new(page.storage.input(field))
                        .small()
                        .disabled(!page.server_ready())
                        .aria_label(title),
                ),
            )
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("days"))
        })
        .child(
            Switch::new(SharedString::from(format!("{id}-switch")))
                .checked(days.is_some())
                .disabled(!page.server_ready())
                .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                    let value = if *checked { json!(DEFAULT_RETENTION_DAYS) } else { Value::Null };
                    this.storage.invalid.retain(|name| *name != field);
                    set_rule(this, field, value, cx)
                })),
        );
    rule_row(page, id, title, "", field, days.is_some(), Value::Null, cx)
        .description(description)
        .control(control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt as _;
    use crate::settings::tests::{load, open_page, sent};
    use t3_client::{ProjectShell, ServerSettings};

    #[test]
    fn days_are_validated_to_the_schema_range() {
        assert_eq!(parse_days(" 30 "), Some(30));
        assert_eq!(parse_days("1"), Some(1));
        assert_eq!(parse_days("3650"), Some(3650));
        for bad in ["0", "3651", "-1", "", "1.5", "abc"] {
            assert_eq!(parse_days(bad), None, "{bad}");
        }
    }

    #[test]
    fn default_storage_cleanup_reads_as_unmodified() {
        let settings = ServerSettings::from_value(json!({ "storageCleanup": { "logsAfterDays": null } }));
        assert!(!settings.is_modified("storageCleanup", None));
        let settings = ServerSettings::from_value(json!({ "storageCleanup": { "logsAfterDays": 7 } }));
        assert!(settings.is_modified("storageCleanup", None));
        assert_eq!(
            settings.reset_patch(KEYS, None),
            Some(json!({ "storageCleanup": default_storage() }))
        );
    }

    fn default_storage() -> Value {
        t3_client::settings::default_value("storageCleanup").unwrap()
    }

    #[gpui_kit::test]
    fn rule_edits_write_environment_rules_and_project_overrides(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(1600.)));
        load(
            &page,
            cx,
            json!({ "storageCleanup": { "worktreeOnMerge": true, "worktreeAfterDays": 14 } }),
            true,
        );
        page.update(cx, |page, cx| {
            page.section = Section::Storage;
            set_rule(page, "worktreeAfterDays", json!(60), cx);
            set_rule(page, "logsAfterDays", Value::Null, cx);
            set_rule(page, "worktreeOnDelete", json!(true), cx);
        });
        assert_eq!(
            sent(&page, cx),
            [
                json!({ "storageCleanup": { "worktreeAfterDays": 60 } }),
                json!({ "storageCleanup": { "logsAfterDays": null } }),
                json!({ "storageCleanup": { "worktreeOnDelete": true } }),
            ]
        );

        let projects = serde_json::from_value::<Vec<ProjectShell>>(
            json!([{ "id": "p1", "title": "One", "workspaceRoot": "/one" }]),
        )
        .unwrap();
        page.update(cx, |page, cx| {
            page.set_projects(&projects, cx);
            page.set_scope(Some("p1".into()), cx);
            // Off, then inherit, are explicit patches on the project entry.
            page.set_server_value("worktreeCleanup", json!({ "mode": "off" }), cx);
            page.set_server_value("worktreeCleanup", Value::Null, cx);
            // The first rule edit makes the override "custom" with all four
            // rules, starting from the environment's.
            set_rule(page, "worktreeOnMerge", json!(false), cx);
        });
        let patches = sent(&page, cx);
        let entry = |patch: &Value| patch["projectSettingsOverrides"]["p1"]["worktreeCleanup"].clone();
        assert_eq!(entry(&patches[3]), json!({ "mode": "off" }));
        // `null` is not stored for a non-nullable project key: the entry goes away.
        assert_eq!(patches[4], json!({ "projectSettingsOverrides": { "p1": null } }));
        assert_eq!(
            entry(&patches[5]),
            json!({ "mode": "custom", "rules": {
                "worktreeAfterDays": 60, "worktreeOnMerge": false,
                "worktreeOnDelete": true, "worktreeUnchanged": false
            } })
        );
        // Further edits at the project keep the other rules.
        page.update(cx, |page, cx| set_rule(page, "worktreeUnchanged", json!(true), cx));
        let last = sent(&page, cx).pop().unwrap();
        assert_eq!(
            entry(&last)["rules"],
            json!({ "worktreeAfterDays": 60, "worktreeOnMerge": false,
                    "worktreeOnDelete": true, "worktreeUnchanged": true })
        );
        // Retention fields that are not worktree rules stay environment-wide.
        page.update(cx, |page, cx| set_rule(page, "logsAfterDays", json!(7), cx));
        assert_eq!(sent(&page, cx).pop().unwrap(), json!({ "storageCleanup": { "logsAfterDays": 7 } }));

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("setting-worktree-on-delete-switch").is_some());
            assert!(window.try_find("setting-worktree-cleanup-select").is_some());
            assert!(window.try_find("setting-logs-days-switch").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn unsupported_servers_get_a_notice(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(900.)));
        load(&page, cx, json!({}), true);
        page.update(cx, |page, cx| {
            page.section = Section::Storage;
            page.set_capabilities(json!({ "storageCleanup": false }), cx);
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("setting-logs-days-switch").is_none());
        })
        .unwrap();
    }
}
