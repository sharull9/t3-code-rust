//! Source Control: how commit and pull request text is written and how pull
//! requests start. All of these are server settings; every key here can be
//! overridden per project.

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, v_flex};
use gpui_kit::*;
use serde_json::{Value, json};

use crate::model_picker;
use crate::prefs::Prefs;
use crate::settings::row::{
    SettingRow, SettingsGroup, choice_button, server_row, server_switch,
};
use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsPage};

/// Every server setting on this page, for "Restore defaults".
pub const KEYS: &[&str] = &[
    "sourceControlWritingStyle",
    "sourceControlWriterModelSelection",
    "defaultAutoPull",
    "pullRequestMergeMethod",
];

pub const SEARCH: &[SearchEntry] = &[
    SearchEntry {
        title: "Writing style",
        description: "How change descriptions and change request text are written.",
        keywords: &[
            "commit message",
            "pull request",
            "conventional commits",
            "repository conventions",
            "custom instructions",
            "text generation",
        ],
        section: Section::SourceControl,
    },
    SearchEntry {
        title: "Custom instructions",
        description: "Instructions used for change descriptions and change requests in every project.",
        keywords: &["prompt", "commit", "writing style", "custom"],
        section: Section::SourceControl,
    },
    SearchEntry {
        title: "Follow change request templates",
        description: "Use the repository's template for change request descriptions when available.",
        keywords: &["pull request template", "pr template", "description"],
        section: Section::SourceControl,
    },
    SearchEntry {
        title: "Writer model",
        description: "Model for source control text and branch or bookmark names.",
        keywords: &["source control writer", "text generation", "commit", "branch name", "separate model"],
        section: Section::SourceControl,
    },
    SearchEntry {
        title: "Automatically pull",
        description: "Keeps the default branch current when the checkout has no local changes or commits.",
        keywords: &["auto pull", "git pull", "default branch", "sync"],
        section: Section::SourceControl,
    },
    SearchEntry {
        title: "Merge method",
        description: "The method pull requests start with.",
        keywords: &["pull request", "squash", "rebase", "merge commit", "last selected"],
        section: Section::SourceControl,
    },
];

const MERGE_METHODS: &[(&str, &str)] =
    &[("merge", "Create a merge commit"), ("squash", "Squash and merge"), ("rebase", "Rebase and merge")];
const STYLE_MODES: &[(&str, &str)] = &[
    ("repo_conventions", "Repository conventions"),
    ("conventional_commits", "Conventional Commits"),
    ("custom", "Custom instructions"),
];

/// The custom-instructions text area.
pub struct State {
    instructions: Entity<TextareaState>,
}

impl State {
    pub fn new(
        window: &mut Window,
        subscriptions: &mut Vec<Subscription>,
        cx: &mut Context<SettingsPage>,
    ) -> Self {
        let instructions = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 10)
                .placeholder("Write the instructions to use for commit and pull request text.")
        });
        // Saved when focus leaves, not per keystroke.
        subscriptions.push(cx.subscribe(&instructions, |page, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Blur) {
                commit_instructions(page, cx);
            }
        }));
        Self { instructions }
    }
}

fn style_field(page: &SettingsPage, field: &str) -> Value {
    let style = page.server_value("sourceControlWritingStyle").value;
    style
        .get(field)
        .cloned()
        .or_else(|| t3_client::settings::default_value("sourceControlWritingStyle")?.get(field).cloned())
        .unwrap_or(Value::Null)
}

/// The writing-style object with `field` replaced. The whole object is sent so
/// that a project override stands on its own.
fn style_with(page: &SettingsPage, field: &str, value: Value) -> Value {
    let mut style =
        t3_client::settings::default_value("sourceControlWritingStyle").unwrap_or(Value::Null);
    if let (Some(style), Some(current)) =
        (style.as_object_mut(), page.server_value("sourceControlWritingStyle").value.as_object())
    {
        style.extend(current.clone());
        style.insert(field.to_owned(), value);
    }
    style
}

fn set_style_field(page: &mut SettingsPage, field: &str, value: Value, cx: &mut Context<SettingsPage>) {
    let style = style_with(page, field, value);
    page.set_server_value("sourceControlWritingStyle", style, cx);
}

fn commit_instructions(page: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    if !page.server_ready() {
        return;
    }
    let text = page.source_control.instructions.read(cx).value().trim().to_owned();
    if style_field(page, "customInstructions").as_str() != Some(text.as_str()) {
        set_style_field(page, "customInstructions", Value::String(text), cx);
    }
}

/// Shows the saved instructions in the text area unless it is being edited.
pub fn sync_inputs(page: &SettingsPage, window: &mut Window, cx: &mut Context<SettingsPage>) {
    let saved = style_field(page, "customInstructions").as_str().unwrap_or_default().to_owned();
    let input = &page.source_control.instructions;
    if input.read(cx).focus_handle(cx).is_focused(window) || input.read(cx).value().as_ref() == saved {
        return;
    }
    input.update(cx, |input, cx| input.set_value(saved, window, cx));
}

pub fn modified(page: &SettingsPage, _: &App) -> bool {
    page.server_keys_modified(KEYS)
}

pub fn restore_defaults(page: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    page.reset_server_keys(KEYS, cx);
}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let in_project = page.project_scope().is_some();
    let style_mode = style_field(page, "mode");
    let mode_description = match style_mode.as_str() {
        Some("conventional_commits") => "Use Conventional Commit prefixes and keep change request text concise.",
        Some("custom") => "Use your instructions for change descriptions and change requests in every project.",
        _ => "In each project, matches recent change descriptions and change request titles.",
    };
    let style = server_row(page, "setting-writing-style", "Writing style", "sourceControlWritingStyle", cx)
        .description(mode_description)
        .control(server_choice_style(page, cx));
    let instructions = SettingRow::new("setting-custom-instructions", "Custom instructions")
        .description("Applied to every change description and change request.")
        .hidden(style_mode.as_str() != Some("custom"))
        .control(
            div()
                .w(px(360.))
                .child(Textarea::new(&page.source_control.instructions).disabled(!page.server_ready())),
        );
    let follow = server_row(
        page,
        "setting-follow-templates",
        "Follow change request templates",
        "sourceControlWritingStyle",
        cx,
    )
    .description("Use the repository's template for change request descriptions when available.")
    .modified(false)
    .control(
        Switch::new("setting-follow-templates-switch")
            .checked(style_field(page, "followChangeRequestTemplates").as_bool().unwrap_or(true))
            .disabled(!page.server_ready())
            .on_change(cx.listener(|this, checked: &bool, _, cx| {
                set_style_field(this, "followChangeRequestTemplates", Value::Bool(*checked), cx)
            })),
    );
    let writer = server_row(
        page,
        "setting-writer-model",
        "Writer model",
        "sourceControlWriterModelSelection",
        cx,
    )
    .description(
        "Model for source control text and branch or bookmark names. Off uses the text generation model.",
    )
    .control(writer_control(page, cx));

    let pull = server_row(page, "setting-auto-pull", "Automatically pull", "defaultAutoPull", cx)
        .description(if in_project {
            "Keeps this project's default branch current when the checkout has no local changes or commits."
        } else {
            "Keeps the default branch current when the checkout has no local changes or commits. Projects can override it."
        })
        .control(server_switch(page, "setting-auto-pull-switch", "defaultAutoPull", cx));
    let merge = server_row(page, "setting-merge-method", "Merge method", "pullRequestMergeMethod", cx)
        .description(if in_project {
            "Pull requests in this project start with this method."
        } else {
            "Pull requests start with this method. Last selected reuses whatever you chose most recently on this device."
        })
        .control(merge_control(page, cx));

    v_flex()
        .gap_6()
        .child(
            SettingsGroup::new("Text generation")
                .row(style)
                .row(instructions)
                .row(follow)
                .row(writer),
        )
        .child(SettingsGroup::new("Pull requests").row(pull).row(merge))
        .into_any_element()
}

fn server_choice_style(page: &SettingsPage, cx: &Context<SettingsPage>) -> impl IntoElement {
    let current = style_field(page, "mode");
    let label = STYLE_MODES
        .iter()
        .find(|(value, _)| current.as_str() == Some(*value))
        .map_or("Repository conventions", |(_, label)| *label);
    let view = cx.entity();
    choice_button("setting-writing-style-select", label, !page.server_ready(), move |mut menu| {
        for &(value, label) in STYLE_MODES {
            let view = view.clone();
            menu = menu.item(PopupMenuItem::new(label).checked(current.as_str() == Some(value)).on_click(
                move |_, _, cx| {
                    view.update(cx, |this, cx| set_style_field(this, "mode", json!(value), cx));
                },
            ));
        }
        menu
    })
}

/// "Last selected" (`null`) plus the three merge methods.
fn merge_control(page: &SettingsPage, cx: &Context<SettingsPage>) -> impl IntoElement {
    let current = page.server_value("pullRequestMergeMethod").value;
    let label = MERGE_METHODS
        .iter()
        .find(|(value, _)| current.as_str() == Some(*value))
        .map_or("Last selected", |(_, label)| *label);
    let view = cx.entity();
    choice_button("setting-merge-method-select", label, !page.server_ready(), move |mut menu| {
        let last_view = view.clone();
        menu = menu.item(PopupMenuItem::new("Last selected").checked(current.is_null()).on_click(
            move |_, _, cx| {
                last_view
                    .update(cx, |this, cx| this.set_server_value("pullRequestMergeMethod", Value::Null, cx));
            },
        ));
        for &(value, label) in MERGE_METHODS {
            let view = view.clone();
            menu = menu.item(PopupMenuItem::new(label).checked(current.as_str() == Some(value)).on_click(
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.set_server_value("pullRequestMergeMethod", json!(value), cx)
                    });
                },
            ));
        }
        menu
    })
}

/// Models of usable providers that this device's picker has not hidden (the
/// selected one is always kept): `(provider, [(instance, model, label)])`.
fn model_groups(
    page: &SettingsPage,
    selected: Option<&(String, String)>,
    cx: &App,
) -> Vec<(String, Vec<(String, String, String)>)> {
    let prefs = Prefs::global(cx);
    page.providers
        .iter()
        .filter(|provider| model_picker::usable(provider))
        .map(|provider| {
            let models = provider
                .models
                .iter()
                .filter(|model| {
                    !prefs.is_hidden(&provider.instance_id, &model.id)
                        || selected.is_some_and(|(i, m)| *i == provider.instance_id && *m == model.id)
                })
                .map(|model| (provider.instance_id.clone(), model.id.clone(), model.label.clone()))
                .collect();
            (model_picker::provider_name(provider), models)
        })
        .filter(|(_, models): &(String, Vec<_>)| !models.is_empty())
        .collect()
}

fn selection_of(value: &Value) -> Option<(String, String)> {
    value
        .get("instanceId")
        .and_then(Value::as_str)
        .zip(value.get("model").and_then(Value::as_str))
        .map(|(instance, model)| (instance.to_owned(), model.to_owned()))
}

/// A switch for "use a separate model" and, when on, the model dropdown.
fn writer_control(page: &SettingsPage, cx: &Context<SettingsPage>) -> impl IntoElement {
    let selected = selection_of(&page.server_value("sourceControlWriterModelSelection").value);
    let groups = model_groups(page, selected.as_ref(), cx);
    // What turning the switch on selects: the default model if it is usable,
    // otherwise the first usable model.
    let default = selection_of(&page.server_value("defaultModelSelection").value)
        .filter(|(instance, model)| {
            groups.iter().flat_map(|(_, models)| models).any(|(i, m, _)| i == instance && m == model)
        })
        .or_else(|| {
            groups
                .iter()
                .flat_map(|(_, models)| models)
                .next()
                .map(|(instance, model, _)| (instance.clone(), model.clone()))
        });
    let enabled = selected.is_some();
    let switch = Switch::new("setting-writer-model-switch")
        .checked(enabled)
        .disabled(!page.server_ready() || (!enabled && default.is_none()))
        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
            let value = match (&default, *checked) {
                (Some((instance, model)), true) => json!({ "instanceId": instance, "model": model }),
                _ => Value::Null,
            };
            this.set_server_value("sourceControlWriterModelSelection", value, cx)
        }));
    let picker = selected.clone().map(|(instance, model)| {
        let label = groups
            .iter()
            .flat_map(|(_, models)| models)
            .find(|(i, m, _)| *i == instance && *m == model)
            .map_or_else(|| model.clone(), |(_, _, label)| label.clone());
        let view = cx.entity();
        choice_button("setting-writer-model-select", label, !page.server_ready(), move |mut menu| {
            for (provider, models) in &groups {
                menu = menu.separator().item(PopupMenuItem::label(provider.clone()));
                for (instance_id, model_id, label) in models {
                    let checked = *instance_id == instance && *model_id == model;
                    let view = view.clone();
                    let value = json!({ "instanceId": instance_id, "model": model_id });
                    menu = menu.item(PopupMenuItem::new(label.clone()).checked(checked).on_click(
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.set_server_value("sourceControlWriterModelSelection", value.clone(), cx)
                            });
                        },
                    ));
                }
            }
            menu
        })
    });
    gpui_kit::component::h_flex().gap_2().items_center().children(picker).child(switch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt as _;
    use crate::settings::tests::{load, open_page, sent};
    use t3_client::ProjectShell;

    #[gpui_kit::test]
    fn writing_style_merge_method_and_scope_write_the_right_patches(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(1600.)));
        load(&page, cx, json!({}), true);
        page.update(cx, |page, cx| {
            page.section = Section::SourceControl;
            set_style_field(page, "mode", json!("custom"), cx);
            page.set_server_value("pullRequestMergeMethod", json!("squash"), cx);
            page.set_server_value("pullRequestMergeMethod", Value::Null, cx);
        });
        assert_eq!(
            sent(&page, cx)[0],
            json!({ "sourceControlWritingStyle": {
                "mode": "custom", "customInstructions": "", "followChangeRequestTemplates": true
            } })
        );
        assert_eq!(sent(&page, cx)[1], json!({ "pullRequestMergeMethod": "squash" }));
        assert_eq!(sent(&page, cx)[2], json!({ "pullRequestMergeMethod": null }));

        let projects = serde_json::from_value::<Vec<ProjectShell>>(
            json!([{ "id": "p1", "title": "One", "workspaceRoot": "/one" }]),
        )
        .unwrap();
        page.update(cx, |page, cx| {
            page.set_projects(&projects, cx);
            page.set_scope(Some("p1".into()), cx);
            set_style_field(page, "followChangeRequestTemplates", json!(false), cx);
            page.set_server_value("defaultAutoPull", json!(true), cx);
        });
        let patches = sent(&page, cx);
        assert_eq!(
            patches[3]["projectSettingsOverrides"]["p1"]["sourceControlWritingStyle"],
            json!({ "mode": "custom", "customInstructions": "", "followChangeRequestTemplates": false })
        );
        // The project's whole entry is resent.
        assert_eq!(
            patches[4]["projectSettingsOverrides"]["p1"]["defaultAutoPull"],
            json!(true)
        );
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("setting-merge-method-select").is_some());
            assert!(window.try_find("setting-follow-templates-switch").is_some());
        })
        .unwrap();
    }
}
