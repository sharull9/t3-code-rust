//! Keybindings: rebind the app's shortcuts. Commands the web app also has
//! are stored on the server; the rest on this device (see `crate::keymap`).

use gpui_kit::component::button::Button;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::keymap::{self, Command, Shortcut};
use crate::prefs::Prefs;
use crate::settings::row::{SettingRow, SettingsGroup};
use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsEvent, SettingsPage};

pub const SEARCH: &[SearchEntry] = &[
    SearchEntry {
        title: "New thread",
        description: "Start a new thread. Shared with the web app.",
        keywords: &["chat", "create", "shortcut", "keybinding", "hotkey"],
        section: Section::Keybindings,
    },
    SearchEntry {
        title: "Toggle sidebar",
        description: "Show or hide the thread sidebar. Shared with the web app.",
        keywords: &["panel", "threads", "hide", "shortcut", "keybinding", "hotkey"],
        section: Section::Keybindings,
    },
    SearchEntry {
        title: "Focus composer",
        description: "Move focus to the message composer.",
        keywords: &["input", "message", "prompt", "shortcut", "keybinding", "hotkey"],
        section: Section::Keybindings,
    },
    SearchEntry {
        title: "Toggle workspace",
        description: "Show or hide the file and terminal workspace.",
        keywords: &["files", "terminal", "panel", "shortcut", "keybinding", "hotkey"],
        section: Section::Keybindings,
    },
    SearchEntry {
        title: "Open or close Settings",
        description: "Open Settings, or go back when it is open.",
        keywords: &["preferences", "options", "shortcut", "keybinding", "hotkey"],
        section: Section::Keybindings,
    },
    SearchEntry {
        title: "Close or go back",
        description: "Close the open dialog, picker, or Settings page.",
        keywords: &["escape", "esc", "cancel", "back", "shortcut", "keybinding", "hotkey"],
        section: Section::Keybindings,
    },
    SearchEntry {
        title: "Fixed shortcuts",
        description: "Choose answers and list items with Ctrl+1 through Ctrl+9; search Settings with /.",
        keywords: &["question", "answer", "picker", "slot", "search", "keybinding"],
        section: Section::Keybindings,
    },
];

/// Recording state of the page.
#[derive(Default)]
pub struct KeysState {
    /// The command whose next keystroke is being captured.
    recording: Option<Command>,
    /// The last refusal or error, shown under the row it concerns.
    message: Option<(Command, String)>,
    /// Swallows keystrokes while recording so they are not run as shortcuts.
    interceptor: Option<Subscription>,
}

impl KeysState {
    #[cfg(test)]
    pub fn recording(&self) -> Option<Command> {
        self.recording
    }
}

pub fn modified(_: &SettingsPage, cx: &App) -> bool {
    let server = keymap::server_keybindings(cx);
    let prefs = Prefs::global(cx);
    Command::ALL.into_iter().any(|command| keymap::is_modified(command, &server, prefs))
}

pub fn restore_defaults(page: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    page.keys.recording = None;
    page.keys.interceptor = None;
    page.keys.message = None;
    let server = keymap::server_keybindings(cx);
    for command in Command::ALL {
        if command.upstream().is_none() {
            keymap::set_device_override(cx, command, None);
        }
    }
    let ops: Vec<_> = Command::ALL
        .into_iter()
        .filter(|command| keymap::is_modified(*command, &server, Prefs::global(cx)))
        .flat_map(|command| keymap::reset_ops(command, &server))
        .collect();
    page.send_keybinding_ops(Command::NewThread, ops, cx);
}

impl SettingsPage {
    /// The answer to a [`SettingsEvent::UpdateKeybindings`]. The new rules are
    /// applied by the app before this is called; a failure is shown.
    pub fn keybindings_saved(
        &mut self,
        _request_id: u64,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = result {
            let command = self.keys.recording.unwrap_or(Command::NewThread);
            self.keys.message = Some((command, error));
        }
        cx.notify();
    }

    fn send_keybinding_ops(
        &mut self,
        command: Command,
        ops: Vec<t3_client::KeybindingOp>,
        cx: &mut Context<Self>,
    ) {
        if ops.is_empty() {
            cx.notify();
            return;
        }
        if !self.connected {
            self.keys.message = Some((command, "Reconnect to change shared shortcuts.".into()));
            cx.notify();
            return;
        }
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        cx.emit(SettingsEvent::UpdateKeybindings { request_id, ops });
        cx.notify();
    }

    fn start_recording(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        self.keys.recording = Some(command);
        self.keys.message = None;
        let view = cx.entity().downgrade();
        self.keys.interceptor = Some(cx.intercept_keystrokes(move |event, _, cx| {
            let keystroke = event.keystroke.clone();
            let active = view
                .update(cx, |this, cx| this.record_keystroke(&keystroke, cx))
                .unwrap_or(false);
            if active {
                cx.stop_propagation();
            }
        }));
        self.focus(window, cx);
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        self.keys.recording = None;
        self.keys.interceptor = None;
        cx.notify();
    }

    /// Handles a keystroke while recording. Returns whether it was consumed.
    fn record_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(command) = self.keys.recording else { return false };
        if !self.open || self.section != Section::Keybindings {
            self.stop_recording(cx);
            return false;
        }
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            self.stop_recording(cx);
            return true;
        }
        let Some(shortcut) = Shortcut::from_keystroke(keystroke) else {
            // A modifier on its own: wait for the rest of the combination.
            return true;
        };
        if let Some(problem) = shortcut.problem() {
            self.keys.message = Some((command, problem.to_owned()));
            cx.notify();
            return true;
        }
        let server = keymap::server_keybindings(cx);
        if let Some((_, user)) = keymap::conflict(command, &shortcut, &server, Prefs::global(cx)) {
            self.keys.message = Some((
                command,
                format!(
                    "Already used by {user}. Change that shortcut first, or press another."
                ),
            ));
            cx.notify();
            return true;
        }
        self.stop_recording(cx);
        self.keys.message = None;
        if command.upstream().is_some() {
            let ops = keymap::rebind_ops(command, &shortcut, &server, Prefs::global(cx));
            self.send_keybinding_ops(command, ops, cx);
        } else {
            keymap::set_device_override(cx, command, Some(&shortcut));
        }
        true
    }

    fn reset_command(&mut self, command: Command, cx: &mut Context<Self>) {
        self.keys.message = None;
        if command.upstream().is_some() {
            let ops = keymap::reset_ops(command, &keymap::server_keybindings(cx));
            self.send_keybinding_ops(command, ops, cx);
        } else {
            keymap::set_device_override(cx, command, None);
            cx.notify();
        }
    }
}

fn kbd(shortcut: &str) -> Option<Kbd> {
    let shortcut = Shortcut::parse_upstream(shortcut)?;
    Keystroke::parse(&shortcut.to_gpui()).ok().map(Kbd::new)
}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let server = keymap::server_keybindings(cx);
    let prefs = Prefs::global(cx);
    let theme = cx.theme();
    let mut group = SettingsGroup::new("Shortcuts");
    for command in Command::ALL {
        let id = command.id();
        let recording = page.keys.recording == Some(command);
        let message = page.keys.message.as_ref().filter(|(target, _)| *target == command);
        let description = match (recording, message) {
            (_, Some((_, text))) => text.clone(),
            (true, None) => "Press the new shortcut. Esc cancels.".to_owned(),
            (false, None) => command.description().to_owned(),
        };
        let shortcuts = keymap::shortcuts(command, &server, prefs);
        let modified = keymap::is_modified(command, &server, prefs);
        let shared = command.upstream().is_some();
        let row = SettingRow::new(format!("setting-key-{id}"), command.label())
            .description(description)
            .modified(modified)
            .on_reset(cx.listener(move |this, _, _, cx| this.reset_command(command, cx)))
            .control(
                h_flex()
                    .gap_2()
                    .items_center()
                    .when(shortcuts.is_empty(), |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Unbound"),
                        )
                    })
                    .children(shortcuts.iter().filter_map(|shortcut| kbd(shortcut)))
                    .child(
                        Button::new(SharedString::from(format!("key-record-{id}")))
                            .outline()
                            .small()
                            .label(if recording { "Press keys…" } else { "Record" })
                            .disabled(shared && !page.connected)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.keys.recording == Some(command) {
                                    this.stop_recording(cx);
                                } else {
                                    this.start_recording(command, window, cx);
                                }
                            })),
                    ),
            );
        group = group.row(row);
    }
    let fixed = SettingsGroup::new("Fixed shortcuts")
        .row(
            SettingRow::new("setting-key-fixed-choices", "Choose an answer or list item")
                .description("Questions, the model picker, and the project picker.")
                .control(h_flex().children(kbd("mod+1")).children(kbd("mod+9"))),
        )
        .row(
            SettingRow::new("setting-key-fixed-search", "Search Settings")
                .description("Only on this page.")
                .control(h_flex().children(kbd("/"))),
        );
    gpui_kit::component::v_flex()
        .gap_6()
        .child(group)
        .child(fixed)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;
    use t3_client::{KeybindingOp, ResolvedKeybinding, ServerSettings};

    fn rule(command: &str, key: &str, modifiers: &[&str], when: Option<&str>) -> ResolvedKeybinding {
        let has = |name: &str| modifiers.contains(&name);
        let mut value = json!({ "command": command, "shortcut": {
            "key": key, "metaKey": false, "ctrlKey": false, "modKey": has("mod"),
            "shiftKey": has("shift"), "altKey": has("alt") } });
        if let Some(when) = when {
            value["whenAst"] = json!({ "type": "not", "node":
                { "type": "identifier", "name": when } });
        }
        serde_json::from_value(value).unwrap()
    }

    fn server_defaults() -> Vec<ResolvedKeybinding> {
        vec![
            rule("chat.new", "n", &["mod"], Some("terminalFocus")),
            rule("chat.new", "o", &["mod", "shift"], Some("terminalFocus")),
            rule("sidebar.toggle", "b", &["mod"], None),
        ]
    }

    fn open_page(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<SettingsPage>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            crate::settings::init(cx);
            cx.set_global(Prefs::default());
            keymap::set_server_keybindings(cx, server_defaults());
            keymap::apply(cx);
            let (handle, page) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(900.), px(900.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| SettingsPage::new(window, cx)),
            )
            .unwrap();
            (handle.into(), page)
        })
    }

    fn show(page: &Entity<SettingsPage>, cx: &mut TestAppContext) {
        page.update(cx, |page, cx| {
            page.set_open(true, cx);
            page.set_connected(true, cx);
            page.set_server_settings(ServerSettings::from_value(json!({})), cx);
            page.section = Section::Keybindings;
        });
    }

    fn sent(page: &Entity<SettingsPage>, cx: &mut TestAppContext) -> Rc<RefCell<Vec<Vec<KeybindingOp>>>> {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let captured = sent.clone();
        let subscription = cx.update(|cx| {
            cx.subscribe(page, move |_, event: &SettingsEvent, _| {
                if let SettingsEvent::UpdateKeybindings { ops, .. } = event {
                    captured.borrow_mut().push(ops.clone());
                }
            })
        });
        std::mem::forget(subscription);
        sent
    }

    #[gpui_kit::test]
    fn recording_a_native_command_stores_a_device_override_and_rebinds(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx);
        show(&page, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("setting-key-focus-composer-reset").is_none());
            window.click("key-record-focus-composer", cx);
            window.render_frame(cx);
            assert_eq!(page.read(cx).keys.recording(), Some(Command::FocusComposer));
            // Escape cancels without closing the page or changing anything.
            window.press("escape", cx);
            assert_eq!(page.read(cx).keys.recording(), None);
            assert!(page.read(cx).is_open());
            window.click("key-record-focus-composer", cx);
            window.press("ctrl-shift-y", cx);
            assert_eq!(page.read(cx).keys.recording(), None);
            assert_eq!(
                Prefs::global(cx).keybindings.get("focus-composer"),
                Some(&vec!["mod+shift+y".to_owned()])
            );
            window.render_frame(cx);
            assert!(window.try_find("setting-key-focus-composer-reset").is_some());
            // The new binding is live: the old one is gone.
            let live = |cx: &App, keys: &str| {
                let typed = [Keystroke::parse(keys).unwrap()];
                cx.key_bindings()
                    .borrow()
                    .all_bindings_for_input(&typed)
                    .iter()
                    .any(|binding| binding.action().name().ends_with("FocusComposer"))
            };
            assert!(live(cx, "ctrl-shift-y"));
            assert!(!live(cx, "ctrl-l"));
            // Reset restores the default and its binding.
            window.click("setting-key-focus-composer-reset", cx);
            assert!(Prefs::global(cx).keybindings.is_empty());
            assert!(live(cx, "ctrl-l"));
            assert!(!live(cx, "ctrl-shift-y"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn conflicts_are_refused_and_bare_keys_rejected(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx);
        show(&page, cx);
        let sent = sent(&page, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("key-record-focus-composer", cx);
            window.press("ctrl-b", cx);
            // Still recording, with the conflict explained.
            assert_eq!(page.read(cx).keys.recording(), Some(Command::FocusComposer));
            let message = page.read(cx).keys.message.clone().unwrap().1;
            assert!(message.contains("Already used by Toggle sidebar"), "{message}");
            window.press("ctrl-3", cx);
            assert!(page.read(cx).keys.message.clone().unwrap().1.contains("Already used by"));
            window.press("x", cx);
            assert!(page.read(cx).keys.message.clone().unwrap().1.contains("Include Ctrl"));
            assert!(Prefs::global(cx).keybindings.is_empty());
            window.render_frame(cx);
            assert!(window.try_find("key-record-focus-composer").is_some());
            window.press("escape", cx);
        })
        .unwrap();
        assert!(sent.borrow().is_empty());
    }

    fn changed_rules() -> Vec<ResolvedKeybinding> {
        vec![
            rule("chat.new", "y", &["mod", "shift"], Some("terminalFocus")),
            rule("sidebar.toggle", "b", &["mod"], None),
        ]
    }

    #[gpui_kit::test]
    fn recording_a_shared_command_sends_server_operations_then_applies_the_reply(
        cx: &mut TestAppContext,
    ) {
        let (handle, page) = open_page(cx);
        show(&page, cx);
        let sent = sent(&page, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("key-record-new-thread", cx);
            window.press("ctrl-shift-y", cx);
        })
        .unwrap();
        assert_eq!(sent.borrow().len(), 1);
        assert!(matches!(
            &sent.borrow()[0][0],
            KeybindingOp::Upsert { rule, replace: Some(replace) }
                if rule.key == "mod+shift+y" && rule.command == "chat.new" && replace.key == "mod+n"
        ));
        cx.update_window(handle, |_, window, cx| {
            // Nothing changes until the server answers.
            assert!(!modified(page.read(cx), cx));
            keymap::set_server_keybindings(cx, changed_rules());
            assert!(modified(page.read(cx), cx));
            window.render_frame(cx);
            window.click("setting-key-new-thread-reset", cx);
        })
        .unwrap();
        assert_eq!(sent.borrow().len(), 2);
        assert!(
            matches!(&sent.borrow()[1][0], KeybindingOp::Remove(rule) if rule.key == "mod+shift+y")
        );
        cx.update(|cx| {
            keymap::set_server_keybindings(cx, server_defaults());
            assert!(!modified(page.read(cx), cx));
        });
    }

    #[gpui_kit::test]
    fn restore_defaults_clears_device_and_server_overrides(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx);
        show(&page, cx);
        let sent = sent(&page, cx);
        cx.update(|cx| {
            keymap::set_device_override(
                cx,
                Command::ShowSettings,
                Shortcut::parse_upstream("mod+shift+,").as_ref(),
            );
            keymap::set_server_keybindings(cx, changed_rules());
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("settings-restore-defaults", cx);
            assert!(Prefs::global(cx).keybindings.is_empty());
        })
        .unwrap();
        assert_eq!(sent.borrow().len(), 1);
        assert_eq!(sent.borrow()[0].len(), 1, "only the modified server command");
    }
    #[gpui_kit::test]
    fn shared_commands_cannot_be_edited_offline(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx);
        show(&page, cx);
        page.update(cx, |page, cx| page.set_connected(false, cx));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("key-record-new-thread", cx);
            assert_eq!(page.read(cx).keys.recording(), None, "button is disabled");
            window.click("key-record-focus-composer", cx);
            assert_eq!(page.read(cx).keys.recording(), Some(Command::FocusComposer));
        })
        .unwrap();
    }
}
