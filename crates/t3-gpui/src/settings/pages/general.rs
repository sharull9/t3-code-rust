//! General: defaults for new threads and how threads are organized. All of
//! these are server settings; most can be overridden per project.

use gpui_kit::component::switch::Switch;
use gpui_kit::component::v_flex;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::*;
use serde_json::{Value, json};

use crate::model_picker;
use crate::prefs::Prefs;
use crate::settings::row::{SettingsGroup, choice_button, server_choice, server_row, server_switch};
use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsPage};

/// Every server setting on this page, for "Restore defaults".
pub const KEYS: &[&str] = &[
    "defaultModelSelection",
    "defaultRuntimeMode",
    "defaultThreadEnvMode",
    "worktreeSubmodules",
    "newWorktreesStartFromOrigin",
    "sidebarAutoSettleOnMerge",
    "sidebarAutoSettleAfterDays",
    "responseStreamingMode",
    "continueThreadsAfterServerUpdate",
];

pub const SEARCH: &[SearchEntry] = &[
    SearchEntry {
        title: "Model",
        description: "Default model for new threads. Projects can override it.",
        keywords: &["default model", "new thread", "project", "provider", "reasoning effort"],
        section: Section::General,
    },
    SearchEntry {
        title: "Permissions",
        description: "Default permissions for new threads. Projects can override them.",
        keywords: &["runtime mode", "supervised", "approvals", "auto accept edits", "full access"],
        section: Section::General,
    },
    SearchEntry {
        title: "Workspace",
        description: "Where new threads start. Projects and their t3.json can override it.",
        keywords: &["new threads", "local", "worktree", "checkout", "environment mode"],
        section: Section::General,
    },
    SearchEntry {
        title: "Submodules",
        description: "How new worktrees populate git submodules.",
        keywords: &["git submodule", "recursive", "top-level", "skip", "worktree", "t3.json"],
        section: Section::General,
    },
    SearchEntry {
        title: "Start from origin",
        description: "Creates the worktree from the latest matching branch on origin instead of your local branch.",
        keywords: &["new worktrees", "remote", "branch", "local"],
        section: Section::General,
    },
    SearchEntry {
        title: "Auto-settle merged threads",
        description: "Settle a thread when its pull request merges.",
        keywords: &["pull request", "merge", "closed", "sidebar", "automatically"],
        section: Section::General,
    },
    SearchEntry {
        title: "Auto-settle inactive threads",
        description: "Sidebar threads with no activity for this long settle automatically.",
        keywords: &["inactivity", "days", "no activity", "timeout", "sidebar"],
        section: Section::General,
    },
    SearchEntry {
        title: "Response streaming",
        description: "How assistant text reaches the app while a turn runs.",
        keywords: &["output", "token", "paragraph", "buffered", "wait", "turn", "legacy"],
        section: Section::General,
    },
    SearchEntry {
        title: "Continue threads after restarts",
        description: "Automatically resume interrupted threads after an update, crash, or machine restart.",
        keywords: &["resume", "running", "interrupted", "server update", "reboot"],
        section: Section::General,
    },
];

const RUNTIME_MODES: &[(&str, &str)] = &[
    ("approval-required", "Ask first"),
    ("auto-accept-edits", "Accept edits"),
    ("auto", "Auto"),
    ("full-access", "Full access"),
];
const ENV_MODES: &[(&str, &str)] = &[("local", "Current checkout"), ("worktree", "New worktree")];
const SUBMODULES: &[(&str, &str)] =
    &[("recursive", "Recursive"), ("top-level", "Top level only"), ("none", "Skip")];
const STREAMING: &[(&str, &str)] = &[
    ("turn", "Wait for the full response"),
    ("paragraph", "Show finished paragraphs"),
    ("token", "Token by token (legacy)"),
];
/// Choices for "days of inactivity" (the schema allows 1 through 90).
const SETTLE_DAYS: &[u32] = &[1, 2, 3, 5, 7, 14, 30, 60, 90];

pub fn modified(page: &SettingsPage, _: &App) -> bool {
    page.server_keys_modified(KEYS)
}

pub fn restore_defaults(page: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    page.reset_server_keys(KEYS, cx);
}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let in_project = page.project_scope().is_some();
    let scoped_text = |project: &'static str, environment: &'static str| {
        if in_project { project } else { environment }
    };

    let model = server_row(page, "setting-default-model", "Model", "defaultModelSelection", cx)
        .description(scoped_text(
            "Model for new threads in this project.",
            "Default model for new threads. Projects can override it.",
        ))
        .control(model_control(page, cx));
    let permissions =
        server_row(page, "setting-default-permissions", "Permissions", "defaultRuntimeMode", cx)
            .description(scoped_text(
                "Permissions for new threads in this project.",
                "Default permissions for new threads. Projects can override them.",
            ))
            .control(server_choice(
                page,
                "setting-default-permissions-select",
                "defaultRuntimeMode",
                RUNTIME_MODES,
                "Full access",
                cx,
            ));
    let workspace =
        server_row(page, "setting-default-workspace", "Workspace", "defaultThreadEnvMode", cx)
            .description(scoped_text(
                "Where new threads in this project start.",
                "Where new threads start. Projects and their t3.json can override it.",
            ))
            .control(server_choice(
                page,
                "setting-default-workspace-select",
                "defaultThreadEnvMode",
                ENV_MODES,
                "Current checkout",
                cx,
            ));
    let submodules =
        server_row(page, "setting-worktree-submodules", "Submodules", "worktreeSubmodules", cx)
            .description(scoped_text(
                "How new worktrees in this project populate git submodules.",
                "How new worktrees populate git submodules. Projects and their t3.json can override it.",
            ))
            .control(server_choice(
                page,
                "setting-worktree-submodules-select",
                "worktreeSubmodules",
                SUBMODULES,
                "Recursive",
                cx,
            ));
    let origin = server_row(
        page,
        "setting-start-from-origin",
        "Start from origin",
        "newWorktreesStartFromOrigin",
        cx,
    )
    .description(
        "Creates the worktree from the latest matching branch on origin instead of your local branch.",
    )
    .control(server_switch(page, "setting-start-from-origin-switch", "newWorktreesStartFromOrigin", cx));

    let mut organization = SettingsGroup::new("Organization");
    if page.supports("threadAutoSettlement") {
        let days = page.server_value("sidebarAutoSettleAfterDays").value.as_u64();
        let settle_merged = server_row(
            page,
            "setting-auto-settle-merged",
            "Auto-settle merged threads",
            "sidebarAutoSettleOnMerge",
            cx,
        )
        .description(
            "Settle a thread when its pull request merges. Closed pull requests still settle automatically.",
        )
        .control(server_switch(page, "setting-auto-settle-merged-switch", "sidebarAutoSettleOnMerge", cx));
        let settle_inactive = server_row(
            page,
            "setting-auto-settle-inactive",
            "Auto-settle inactive threads",
            "sidebarAutoSettleAfterDays",
            cx,
        )
        .description("Sidebar threads with no activity for this long settle automatically.")
        .control(
            Switch::new("setting-auto-settle-inactive-switch")
                .checked(days.is_some())
                .disabled(!page.server_ready())
                .on_change(cx.listener(|this, checked: &bool, _, cx| {
                    // Off is an explicit `null`: at a project that is a stored
                    // "never settle" override, not an inherited value.
                    let value = if *checked { json!(3) } else { Value::Null };
                    this.set_server_value("sidebarAutoSettleAfterDays", value, cx)
                })),
        );
        organization = organization.row(settle_merged).row(settle_inactive);
        if let Some(days) = days {
            organization = organization.row(
                crate::settings::row::SettingRow::new(
                    "setting-auto-settle-days",
                    "Days of inactivity before auto-settle",
                )
                .description("Any new activity un-settles a thread automatically.")
                .control(days_control(page, days as u32, cx)),
            );
        }
    }
    let streaming_description = match page.server_value("responseStreamingMode").value.as_str() {
        Some("turn") => "Text appears once the agent finishes its turn.",
        Some("token") => {
            "Every token repaints the answer as it arrives. Slower and harder to read. Thinking traces still arrive a paragraph at a time."
        }
        _ => "Each paragraph or code block appears as soon as it is complete.",
    };
    let streaming =
        server_row(page, "setting-response-streaming", "Response streaming", "responseStreamingMode", cx)
            .description(streaming_description)
            .control(server_choice(
                page,
                "setting-response-streaming-select",
                "responseStreamingMode",
                STREAMING,
                "Show finished paragraphs",
                cx,
            ));
    let supports_continuation = page.supports("threadRestartContinuation");
    let continuation = server_row(
        page,
        "setting-continue-threads",
        "Continue threads after restarts",
        "continueThreadsAfterServerUpdate",
        cx,
    )
    .description(if supports_continuation {
        "Automatically resume interrupted threads after an update, crash, or machine restart."
    } else {
        "This server does not support restart continuation."
    })
    .control(
        server_switch(page, "setting-continue-threads-switch", "continueThreadsAfterServerUpdate", cx)
            .disabled(!page.server_ready() || !supports_continuation),
    );
    organization = organization.row(streaming).row(continuation);

    v_flex()
        .gap_6()
        .child(
            SettingsGroup::new("New threads")
                .row(model)
                .row(permissions)
                .row(workspace)
                .row(submodules)
                .row(origin),
        )
        .child(organization)
        .into_any_element()
}

/// Model dropdown: "Automatic" (no default) plus the usable instances' models
/// that this device's picker has not hidden.
fn model_control(page: &SettingsPage, cx: &Context<SettingsPage>) -> impl IntoElement {
    let selection = page.server_value("defaultModelSelection").value;
    let selected = selection
        .get("instanceId")
        .and_then(Value::as_str)
        .zip(selection.get("model").and_then(Value::as_str))
        .map(|(instance, model)| (instance.to_owned(), model.to_owned()));
    let prefs = Prefs::global(cx);
    let groups: Vec<(String, Vec<(String, String, String)>)> = page
        .providers
        .iter()
        .filter(|provider| model_picker::usable(provider))
        .map(|provider| {
            let models = provider
                .models
                .iter()
                .filter(|model| {
                    !prefs.is_hidden(&provider.instance_id, &model.id)
                        || selected.as_ref().is_some_and(|(i, m)| {
                            *i == provider.instance_id && *m == model.id
                        })
                })
                .map(|model| (provider.instance_id.clone(), model.id.clone(), model.label.clone()))
                .collect();
            (model_picker::provider_name(provider), models)
        })
        .filter(|(_, models): &(String, Vec<_>)| !models.is_empty())
        .collect();
    let label = match &selected {
        None => "Automatic".to_owned(),
        Some((instance, model)) => groups
            .iter()
            .flat_map(|(_, models)| models)
            .find(|(i, m, _)| i == instance && m == model)
            .map_or_else(|| model.clone(), |(_, _, label)| label.clone()),
    };
    let view = cx.entity();
    choice_button("setting-default-model-select", label, !page.server_ready(), move |mut menu| {
        let automatic_view = view.clone();
        menu = menu.item(PopupMenuItem::new("Automatic").checked(selected.is_none()).on_click(
            move |_, _, cx| {
                automatic_view
                    .update(cx, |this, cx| this.set_server_value("defaultModelSelection", Value::Null, cx));
            },
        ));
        for (provider, models) in &groups {
            menu = menu.separator().item(PopupMenuItem::label(provider.clone()));
            for (instance, model, label) in models {
                let checked = selected.as_ref().is_some_and(|(i, m)| i == instance && m == model);
                let view = view.clone();
                let value = json!({ "instanceId": instance, "model": model });
                menu = menu.item(PopupMenuItem::new(label.clone()).checked(checked).on_click(
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.set_server_value("defaultModelSelection", value.clone(), cx)
                        });
                    },
                ));
            }
        }
        menu
    })
}

fn days_control(page: &SettingsPage, days: u32, cx: &Context<SettingsPage>) -> impl IntoElement {
    let view = cx.entity();
    let mut choices = SETTLE_DAYS.to_vec();
    if !choices.contains(&days) {
        choices.push(days);
        choices.sort_unstable();
    }
    choice_button(
        "setting-auto-settle-days-select",
        day_label(days),
        !page.server_ready(),
        move |mut menu| {
            for &choice in &choices {
                let view = view.clone();
                menu = menu.item(PopupMenuItem::new(day_label(choice)).checked(choice == days).on_click(
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.set_server_value("sidebarAutoSettleAfterDays", json!(choice), cx)
                        });
                    },
                ));
            }
            menu
        },
    )
}

fn day_label(days: u32) -> String {
    if days == 1 { "1 day".to_owned() } else { format!("{days} days") }
}
