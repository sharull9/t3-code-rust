//! Full-page settings for device preferences and the connected environment.
//!
//! `SettingsPage` is the shell: navigation, search, the scope selector and
//! save state. Each section lives in `pages/<name>.rs`. Pages read server
//! settings through [`SettingsPage::server_value`] and write them through
//! [`SettingsPage::set_server_value`] / [`SettingsPage::reset_server_keys`],
//! which route to the environment or to the selected project's overrides.

use std::borrow::Cow;

use gpui_kit::assets::IconName;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, StyledExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use t3_client::settings::ScopedValue;
use t3_client::{ProjectShell, ServerProvider, ServerSettings};


pub mod pages;
pub mod row;
pub mod search;

gpui_kit::actions!(t3_settings, [FocusSearch]);

pub fn init(cx: &mut App) {
    // Not while typing: the input has its own "Input" key context.
    cx.bind_keys([KeyBinding::new("/", FocusSearch, Some("SettingsPage && !Input"))]);
}

pub enum SettingsEvent {
    Close,
    RefreshProviders,
    ChooseManagedServer,
    SwitchServer,
    /// Send `patch` with `server.updateSettings`, then answer through
    /// [`SettingsPage::settings_saved`] with the same `request_id`.
    UpdateServerSettings { request_id: u64, patch: Value },
    /// Send `ops` with `server.upsertKeybinding` / `server.removeKeybinding`,
    /// then answer through [`SettingsPage::keybindings_saved`].
    UpdateKeybindings { request_id: u64, ops: Vec<t3_client::KeybindingOp> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Appearance,
    Keybindings,
    Providers,
    SourceControl,
    Storage,
    Connections,
    Archive,
}

impl Section {
    /// Navigation order (upstream's).
    pub const ALL: [Self; 8] = [
        Self::General,
        Self::Appearance,
        Self::Keybindings,
        Self::Providers,
        Self::SourceControl,
        Self::Storage,
        Self::Connections,
        Self::Archive,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::General => "settings-nav-general",
            Self::Appearance => "settings-nav-appearance",
            Self::Keybindings => "settings-nav-keybindings",
            Self::Providers => "settings-nav-providers",
            Self::SourceControl => "settings-nav-source-control",
            Self::Storage => "settings-nav-storage",
            Self::Connections => "settings-nav-connections",
            Self::Archive => "settings-nav-archive",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Keybindings => "Keybindings",
            Self::Providers => "Providers",
            Self::SourceControl => "Source Control",
            Self::Storage => "Storage",
            Self::Connections => "Connections",
            Self::Archive => "Archive",
        }
    }
    fn icon(self) -> IconName {
        match self {
            Self::General => IconName::SlidersHorizontal,
            Self::Appearance => IconName::Palette,
            Self::Keybindings => IconName::Keyboard,
            Self::Providers => IconName::Bot,
            Self::SourceControl => IconName::GitBranch,
            Self::Storage => IconName::HardDrive,
            Self::Connections => IconName::Server,
            Self::Archive => IconName::Archive,
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::General => "Defaults for new threads and how threads are organized.",
            Self::Appearance => "Choose the appearance of this app on this device.",
            Self::Keybindings => "Shortcuts for navigating and composing in this app.",
            Self::Providers => "Agent providers reported by the connected server.",
            Self::SourceControl => "Git and pull request behavior.",
            Self::Storage => "Disk usage and cleanup.",
            Self::Connections => "Connect to another server or start a local server.",
            Self::Archive => "Archived threads.",
        }
    }
    /// Whether the page edits server settings, and so shows the scope selector.
    fn scoped(self) -> bool {
        matches!(self, Self::General | Self::SourceControl | Self::Storage)
    }
    pub fn search_entries(self) -> &'static [search::SearchEntry] {
        match self {
            Self::General => pages::general::SEARCH,
            Self::Appearance => pages::appearance::SEARCH,
            Self::Keybindings => pages::keybindings::SEARCH,
            Self::Providers => pages::providers::SEARCH,
            Self::SourceControl => pages::source_control::SEARCH,
            Self::Storage => pages::storage::SEARCH,
            Self::Connections => pages::connections::SEARCH,
            Self::Archive => pages::archive::SEARCH,
        }
    }
    fn render(self, page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
        match self {
            Self::General => pages::general::render(page, cx),
            Self::Appearance => pages::appearance::render(page, cx),
            Self::Keybindings => pages::keybindings::render(page, cx),
            Self::Providers => pages::providers::render(page, cx),
            Self::SourceControl => pages::source_control::render(page, cx),
            Self::Storage => pages::storage::render(page, cx),
            Self::Connections => pages::connections::render(page, cx),
            Self::Archive => pages::archive::render(page, cx),
        }
    }
    /// Whether any setting on the page differs from its default at the
    /// current scope ("Restore defaults" is enabled).
    fn modified(self, page: &SettingsPage, cx: &App) -> bool {
        match self {
            Self::General => pages::general::modified(page, cx),
            Self::Appearance => pages::appearance::modified(page, cx),
            Self::Keybindings => pages::keybindings::modified(page, cx),
            Self::Providers => pages::providers::modified(page, cx),
            Self::SourceControl => pages::source_control::modified(page, cx),
            Self::Storage => pages::storage::modified(page, cx),
            Self::Connections => pages::connections::modified(page, cx),
            Self::Archive => pages::archive::modified(page, cx),
        }
    }
    fn restore_defaults(self, page: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
        match self {
            Self::General => pages::general::restore_defaults(page, cx),
            Self::Appearance => pages::appearance::restore_defaults(page, cx),
            Self::Keybindings => pages::keybindings::restore_defaults(page, cx),
            Self::Providers => pages::providers::restore_defaults(page, cx),
            Self::SourceControl => pages::source_control::restore_defaults(page, cx),
            Self::Storage => pages::storage::restore_defaults(page, cx),
            Self::Connections => pages::connections::restore_defaults(page, cx),
            Self::Archive => pages::archive::restore_defaults(page, cx),
        }
    }
}

/// A server-settings write that has been sent and not yet answered. Shown on
/// top of the last server value so the UI reacts at once.
struct PendingSave {
    request_id: u64,
    patch: Value,
}

pub struct SettingsPage {
    providers: Vec<ServerProvider>,
    connected: bool,
    open: bool,
    section: Section,
    /// The provider whose model list is expanded.
    expanded_provider: Option<String>,
    focus_handle: FocusHandle,
    search: Entity<InputState>,
    /// What the connected server advertises (`environment.capabilities`).
    capabilities: Value,
    /// The environment's settings as the server last reported them.
    server_settings: Option<ServerSettings>,
    /// `(id, title)` of the projects a scope can name.
    projects: Vec<(String, String)>,
    /// `None` edits environment defaults ("All projects"); `Some` edits that
    /// project's overrides. Only the single connected environment is scoped:
    /// choosing among several environments is out of scope for now.
    scope: Option<String>,
    pending: Vec<PendingSave>,
    save_error: Option<String>,
    next_request_id: u64,
    /// Shortcut recording state of the Keybindings page.
    keys: pages::keybindings::KeysState,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<SettingsEvent> for SettingsPage {}
impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscription = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            providers: Vec::new(),
            connected: false,
            open: false,
            section: Section::General,
            expanded_provider: None,
            focus_handle: cx.focus_handle(),
            search,
            capabilities: Value::Null,
            server_settings: None,
            projects: Vec::new(),
            scope: None,
            pending: Vec::new(),
            save_error: None,
            next_request_id: 1,
            keys: Default::default(),
            _subscriptions: vec![subscription],
        }
    }
    pub fn set_providers(&mut self, providers: Vec<ServerProvider>, cx: &mut Context<Self>) {
        self.providers = providers;
        cx.notify();
    }
    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected != connected {
            self.connected = connected;
            cx.notify();
        }
    }
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            cx.notify();
        }
    }
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    /// The server's `environment.capabilities`.
    pub fn set_capabilities(&mut self, capabilities: Value, cx: &mut Context<Self>) {
        self.capabilities = capabilities;
        cx.notify();
    }
    /// Authoritative settings from the server (a load, a save's reply or the
    /// config stream).
    pub fn set_server_settings(&mut self, settings: ServerSettings, cx: &mut Context<Self>) {
        self.server_settings = Some(settings);
        cx.notify();
    }
    /// The projects a scope can name. A selected project that is gone falls
    /// back to "All projects".
    pub fn set_projects(&mut self, projects: &[ProjectShell], cx: &mut Context<Self>) {
        self.projects =
            projects.iter().map(|project| (project.id.clone(), project.title.clone())).collect();
        if self.scope.as_ref().is_some_and(|scope| !self.projects.iter().any(|(id, _)| id == scope))
        {
            self.scope = None;
        }
        cx.notify();
    }
    /// The answer to a [`SettingsEvent::UpdateServerSettings`]. A failure drops
    /// the pending change, so the page shows the server's value again, and
    /// reports the error.
    pub fn settings_saved(
        &mut self,
        request_id: u64,
        result: Result<ServerSettings, String>,
        cx: &mut Context<Self>,
    ) {
        self.pending.retain(|save| save.request_id != request_id);
        match result {
            Ok(settings) => self.server_settings = Some(settings),
            Err(error) => self.save_error = Some(error),
        }
        cx.notify();
    }

    // What pages use to read and write server settings.

    /// Server settings can be changed: connected, and the settings are loaded.
    pub fn server_ready(&self) -> bool {
        self.connected && self.server_settings.is_some()
    }
    /// The selected project's ID; `None` is "All projects".
    pub fn project_scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }
    /// Whether the server advertises `capability` (`environment.capabilities`).
    /// Until a config is known, everything counts as supported.
    pub fn supports(&self, capability: &str) -> bool {
        !self.capabilities.is_object() || self.capabilities[capability] == true
    }
    /// The settings including saves still in flight.
    fn effective_settings(&self) -> Cow<'_, ServerSettings> {
        let base = self.server_settings.clone().unwrap_or_default();
        if self.pending.is_empty() {
            return Cow::Owned(base);
        }
        let mut settings = base;
        for save in &self.pending {
            settings.apply_patch(&save.patch);
        }
        Cow::Owned(settings)
    }
    /// `key` at the current scope; at a project, whether an override supplies it.
    pub fn server_value(&self, key: &str) -> ScopedValue {
        self.effective_settings().scoped(key, self.project_scope())
    }
    /// Differs from the default at this scope (an override, at a project).
    pub fn server_modified(&self, key: &str) -> bool {
        self.effective_settings().is_modified(key, self.project_scope())
    }
    pub fn server_keys_modified(&self, keys: &[&str]) -> bool {
        let settings = self.effective_settings();
        keys.iter().any(|key| settings.is_modified(key, self.project_scope()))
    }
    /// Writes `value` for `key` at the current scope: the environment value,
    /// or the project's override when `key` is project-scoped. A `null` for a
    /// project key that is not nullable clears the override.
    pub fn set_server_value(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        let patch = self.effective_settings().set_patch(key, Some(value), self.project_scope());
        self.save(patch, cx);
    }
    /// Restores every modified key in `keys` to its default at the current
    /// scope (clears overrides at a project) in one save.
    pub fn reset_server_keys(&mut self, keys: &[&str], cx: &mut Context<Self>) {
        if let Some(patch) = self.effective_settings().reset_patch(keys, self.project_scope()) {
            self.save(patch, cx);
        }
    }
    fn save(&mut self, patch: Value, cx: &mut Context<Self>) {
        if !self.server_ready() {
            return;
        }
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.save_error = None;
        self.pending.push(PendingSave { request_id, patch: patch.clone() });
        cx.emit(SettingsEvent::UpdateServerSettings { request_id, patch });
        cx.notify();
    }

    fn set_scope(&mut self, scope: Option<String>, cx: &mut Context<Self>) {
        if self.scope != scope {
            self.scope = scope;
            cx.notify();
        }
    }
    fn select_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        if !self.search.read(cx).value().is_empty() {
            self.search.update(cx, |search, cx| search.set_value("", window, cx));
        }
        self.focus(window, cx);
        cx.notify();
    }
    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.focus(window, cx));
    }

    fn render_scope(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let current = self
            .scope
            .as_ref()
            .and_then(|scope| self.projects.iter().find(|(id, _)| id == scope))
            .map_or("All projects", |(_, title)| title.as_str())
            .to_owned();
        let view = cx.entity();
        let projects = self.projects.clone();
        let selected = self.scope.clone();
        h_flex()
            .gap_2()
            .items_center()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child("Applying settings for")
            .child(
                Button::new("settings-scope")
                    .outline()
                    .small()
                    .label(current)
                    .icon(Icon::new(IconName::ChevronDown))
                    .dropdown_menu(move |mut menu, _, _| {
                        let entries = std::iter::once((None, "All projects".to_owned())).chain(
                            projects.iter().map(|(id, title)| (Some(id.clone()), title.clone())),
                        );
                        for (scope, title) in entries {
                            let view = view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(title).checked(selected == scope).on_click(
                                    move |_, _, cx| {
                                        view.update(cx, |this, cx| this.set_scope(scope.clone(), cx));
                                    },
                                ),
                            );
                        }
                        menu.scrollable(true)
                    }),
            )
    }

    fn render_navigation(&self, wide: bool, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let query = self.search.read(cx).value().trim().to_owned();
        let list = if query.is_empty() {
            self.nav_items(wide, cx)
        } else {
            self.search_results(&query, cx)
        };
        v_flex()
            .gap_2()
            .p_3()
            .flex_shrink_0()
            .when(wide, |nav| nav.w(px(220.)).border_r_1())
            .when(!wide, |nav| nav.border_b_1())
            .border_color(theme.border)
            .child(
                h_flex().gap_1().items_center().child(
                    div().flex_1().min_w_0().child(
                        Input::new(&self.search)
                            .small()
                            .cleanable(true)
                            .prefix(
                                Icon::new(IconName::Search)
                                    .small()
                                    .text_color(theme.muted_foreground),
                            )
                            .suffix(
                                Kbd::new(Keystroke::parse("/").expect("valid keystroke"))
                            ),
                    ),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .when(wide, |list| list.flex_col())
                    .when(!wide, |list| list.flex_row().flex_wrap())
                    .children(list),
            )
    }

    fn nav_items(&self, wide: bool, cx: &Context<Self>) -> Vec<AnyElement> {
        Section::ALL
            .into_iter()
            .map(|section| {
                Button::new(section.id())
                    .ghost()
                    .small()
                    .selected(self.section == section)
                    .when(wide, |button| button.w_full())
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .when(wide, |item| item.w_full())
                            .child(Icon::new(section.icon()).small())
                            .child(section.label()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_section(section, window, cx)
                    }))
                    .into_any_element()
            })
            .collect()
    }

    fn search_results(&self, query: &str, cx: &Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme();
        let results = search::search(query);
        if results.is_empty() {
            return vec![
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No settings found")
                    .into_any_element(),
            ];
        }
        results
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let section = entry.section;
                Button::new(SharedString::from(format!("settings-search-result-{index}")))
                    .ghost()
                    .small()
                    .w_full()
                    .child(
                        v_flex()
                            .w_full()
                            .items_start()
                            .child(div().text_sm().child(entry.title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(section.label()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_section(section, window, cx)
                    }))
                    .into_any_element()
            })
            .collect()
    }
}
impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let wide = window.viewport_size().width >= px(1000.);
        let section = self.section;
        let scoped = section.scoped();
        let can_restore = section.modified(self, cx) && (!scoped || self.server_ready());
        let navigation = self.render_navigation(wide, cx);
        let content = section.render(self, cx);
        let theme = cx.theme();
        let saving = !self.pending.is_empty();
        let scope_bar = scoped.then(|| {
            v_flex()
                .gap_1()
                .child(self.render_scope(cx))
                .when(!self.connected, |bar| {
                    bar.child(
                        div()
                            .id("settings-offline-note")
                            .test_support()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Reconnect to change server settings"),
                    )
                })
                .when(self.connected && self.server_settings.is_none(), |bar| {
                    bar.child(
                        div()
                            .id("settings-loading-note")
                            .test_support()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Loading settings…"),
                    )
                })
        });
        v_flex()
            .id("settings-page")
            .test_support()
            .key_context("SettingsPage")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.focus_search(window, cx)
            }))
            .size_full()
            .min_h_0()
            .min_w_0()
            .bg(theme.background)
            .child(
                h_flex()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .items_center()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Icon::new(IconName::Settings)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        h_flex()
                            .id("settings-breadcrumb")
                            .flex_1()
                            .min_w_0()
                            .gap_1p5()
                            .text_base()
                            .child(div().text_color(theme.muted_foreground).child("Settings"))
                            .child(div().text_color(theme.muted_foreground).child("/"))
                            .child(div().font_semibold().child(section.label())),
                    )
                    .when(saving, |header| {
                        header.child(
                            div()
                                .id("settings-saving")
                                .test_support()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Saving…"),
                        )
                    })
                    .child(
                        Button::new("settings-restore-defaults")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::RotateCcw))
                            .label("Restore defaults")
                            .disabled(!can_restore)
                            .tooltip("Reset the settings on this page to their defaults")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.section.restore_defaults(this, cx)
                            })),
                    )
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::ArrowLeft))
                            .label("Back")
                            .tooltip("Back to previous view (Esc)")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .when(!wide, |body| body.flex_col())
                    .child(navigation)
                    .child(
                        div()
                            .id(section.id().to_owned() + "-content")
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_y_scrollbar()
                            .child(
                                v_flex()
                                    .w_full()
                                    .max_w(px(760.))
                                    .p_4()
                                    .gap_4()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .child(section.description()),
                                    )
                                    .children(scope_bar)
                                    .children(self.save_error.clone().map(|error| {
                                        Alert::error("settings-save-error", error).on_close(
                                            cx.listener(|this, _, _, cx| {
                                                this.save_error = None;
                                                cx.notify();
                                            }),
                                        )
                                    }))
                                    .child(content),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Prefs;
    use core::prelude::v1::test;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn open_page(
        cx: &mut TestAppContext,
        size: Size<Pixels>,
    ) -> (AnyWindowHandle, Entity<SettingsPage>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            init(cx);
            let (handle, page) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size,
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

    /// Patches sent and not yet answered, in order.
    fn pending(page: &Entity<SettingsPage>, cx: &App) -> Vec<Value> {
        page.read(cx).pending.iter().map(|save| save.patch.clone()).collect()
    }

    fn load(page: &Entity<SettingsPage>, cx: &mut TestAppContext, settings: Value, connected: bool) {
        page.update(cx, |page, cx| {
            page.set_open(true, cx);
            page.set_connected(connected, cx);
            page.set_server_settings(ServerSettings::from_value(settings), cx);
        });
    }

    #[gpui_kit::test]
    fn offline_refresh_is_disabled_and_category_navigation_keeps_theme(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(700.)));
        let providers = serde_json::from_value::<Vec<ServerProvider>>(json!([
            {"instanceId":"claudeAgent","driver":"claude","displayName":"Claude","enabled":true,"installed":true,"models":[{"slug":"m1","name":"Model 1"}]}
        ])).unwrap();
        page.update(cx, |page, cx| {
            page.set_open(true, cx);
            page.set_providers(providers, cx);
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let captured = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&page, move |_, event: &SettingsEvent, _| {
                captured.borrow_mut().push(match event {
                    SettingsEvent::Close => "close",
                    SettingsEvent::RefreshProviders => "refresh",
                    SettingsEvent::ChooseManagedServer => "managed",
                    SettingsEvent::SwitchServer => "switch",
                    SettingsEvent::UpdateServerSettings { .. } => "update",
                    SettingsEvent::UpdateKeybindings { .. } => "keybindings",
                });
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("settings-nav-appearance", cx);
            window.render_frame(cx);
            window.click("theme-light", cx);
            assert_eq!(crate::prefs::Prefs::global(cx).theme, crate::prefs::ThemeMode::Light);
            window.click("settings-nav-providers", cx);
            window.render_frame(cx);
            window.click("settings-refresh-providers", cx);
            page.update(cx, |page, cx| page.set_connected(true, cx));
            window.render_frame(cx);
            window.click("settings-refresh-providers", cx);
            window.click("settings-nav-connections", cx);
            window.render_frame(cx);
            window.click("settings-managed-server", cx);
            window.click("settings-switch-server", cx);
            window.click("settings-nav-appearance", cx);
            window.render_frame(cx);
            window.click("theme-dark", cx);
            window.click("settings-close", cx);
        })
        .unwrap();
        assert_eq!(
            *events.borrow(),
            ["refresh", "managed", "switch", "close"]
        );
    }

    #[gpui_kit::test]
    fn appearance_controls_change_prefs_and_restore_defaults(cx: &mut TestAppContext) {
        use crate::prefs::{ChatWidth, Prefs, ThemeMode};
        let (handle, page) = open_page(cx, size(px(1000.), px(900.)));
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| page.set_open(true, cx));
            window.render_frame(cx);
            window.click("settings-nav-appearance", cx);
            window.render_frame(cx);
            assert!(!pages::appearance::modified(page.read(cx), cx));
            window.click("chat-width-wide", cx);
            window.click("font-code-up", cx);
            window.click("font-prompt-down", cx);
            window.click("font-interface-up", cx);
            window.click("setting-confirm-archive-switch", cx);
            let prefs = Prefs::global(cx).clone();
            assert_eq!(prefs.chat_width, ChatWidth::Wide);
            assert_eq!(prefs.font_size_code, 14);
            assert_eq!(prefs.font_size_prompt, 13);
            assert_eq!(prefs.font_size_interface, 17);
            assert!(prefs.confirm_thread_archive);
            window.render_frame(cx);
            assert!(pages::appearance::modified(page.read(cx), cx));
            window.click("settings-restore-defaults", cx);
            assert!(!Prefs::global(cx).appearance_modified());
            assert_eq!(Prefs::global(cx).theme, ThemeMode::System);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn model_switches_hide_models_from_the_picker(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(700.)));
        let providers = serde_json::from_value::<Vec<ServerProvider>>(json!([
            {"instanceId":"codex-a","driver":"codex","displayName":"Work","enabled":true,"installed":true,
             "auth":{"email":"me@example.com"},
             "models":[{"slug":"m1","name":"Model 1"},{"slug":"m2","name":"Model 2"}]}
        ])).unwrap();
        page.update(cx, |page, cx| {
            page.set_open(true, cx);
            page.set_connected(true, cx);
            page.set_providers(providers, cx);
            page.section = Section::Providers;
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("model-visible-codex-a-m1").is_none());
            window.click("provider-models-codex-a", cx);
            window.render_frame(cx);
            window.click("model-visible-codex-a-m1", cx);
            window.render_frame(cx);
            assert!(Prefs::global(cx).is_hidden("codex-a", "m1"));
            assert!(!Prefs::global(cx).is_hidden("codex-a", "m2"));
            window.click("provider-models-bulk-codex-a", cx);
            window.render_frame(cx);
            assert!(Prefs::global(cx).hidden_models.is_empty(), "Show all clears the hidden list");
            window.click("provider-models-bulk-codex-a", cx);
            assert!(Prefs::global(cx).is_hidden("codex-a", "m2"), "Hide all hides every model");
            // Restore defaults on this page shows every model again.
            window.render_frame(cx);
            window.click("settings-restore-defaults", cx);
            assert!(Prefs::global(cx).hidden_models.is_empty());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn row_reset_sends_the_default_and_a_failed_save_reverts(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(700.)));
        load(&page, cx, json!({ "defaultRuntimeMode": "approval-required", "other": 1 }), true);
        let sent = Rc::new(RefCell::new(Vec::new()));
        let captured = sent.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&page, move |_, event: &SettingsEvent, _| {
                if let SettingsEvent::UpdateServerSettings { request_id, patch } = event {
                    captured.borrow_mut().push((*request_id, patch.clone()));
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("setting-default-workspace-reset").is_none());
            window.click("setting-default-permissions-reset", cx);
            assert_eq!(pending(&page, cx), [json!({ "defaultRuntimeMode": "full-access" })]);
            // The change shows while the save is pending.
            window.render_frame(cx);
            assert!(window.try_find("settings-saving").is_some());
            assert!(window.try_find("setting-default-permissions-reset").is_none());
            page.update(cx, |page, cx| page.settings_saved(1, Err("Nope".into()), cx));
            window.render_frame(cx);
            assert!(window.try_find("settings-saving").is_none());
            assert!(page.read(cx).save_error.is_some());
            assert!(window.try_find("setting-default-permissions-reset").is_some(), "reverted");
        })
        .unwrap();
        assert_eq!(*sent.borrow(), [(1, json!({ "defaultRuntimeMode": "full-access" }))]);
    }

    #[gpui_kit::test]
    fn restore_defaults_resets_every_modified_key_in_one_patch(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(700.)));
        load(&page, cx, json!({}), true);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                !page.read(cx).server_keys_modified(pages::general::KEYS),
                "nothing differs from the defaults"
            );
            window.click("settings-restore-defaults", cx);
            assert!(pending(&page, cx).is_empty(), "disabled when nothing differs");
            page.update(cx, |page, cx| {
                page.set_server_settings(
                    ServerSettings::from_value(json!({
                        "responseStreamingMode": "turn", "newWorktreesStartFromOrigin": false
                    })),
                    cx,
                )
            });
            window.render_frame(cx);
            window.click("settings-restore-defaults", cx);
            assert_eq!(
                pending(&page, cx),
                [json!({ "responseStreamingMode": "paragraph", "newWorktreesStartFromOrigin": true })]
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn project_scope_writes_and_resets_overrides_with_inheritance(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(1600.)));
        load(&page, cx, json!({ "responseStreamingMode": "token" }), true);
        let projects = serde_json::from_value::<Vec<ProjectShell>>(json!([
            { "id": "p1", "title": "One", "workspaceRoot": "/one" },
        ]))
        .unwrap();
        page.update(cx, |page, cx| {
            page.set_projects(&projects, cx);
            page.set_scope(Some("p1".into()), cx);
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // No override: the environment value is shown as inherited.
            let inherited = page.read(cx).server_value("responseStreamingMode");
            assert_eq!((inherited.value, inherited.overridden), (json!("token"), false));
            assert!(window.try_find("setting-response-streaming-reset").is_none());
            page.update(cx, |page, cx| {
                page.set_server_value("responseStreamingMode", json!("turn"), cx);
                // `null` clears a non-nullable key; stored for a nullable one.
                page.set_server_value("sidebarAutoSettleAfterDays", Value::Null, cx);
            });
            window.render_frame(cx);
            assert_eq!(
                pending(&page, cx),
                [
                    json!({ "projectSettingsOverrides": { "p1": { "responseStreamingMode": "turn" } } }),
                    json!({ "projectSettingsOverrides": { "p1": {
                        "responseStreamingMode": "turn", "sidebarAutoSettleAfterDays": null
                    } } }),
                ]
            );
            let own = page.read(cx).server_value("responseStreamingMode");
            assert_eq!((own.value, own.overridden), (json!("turn"), true));
            // Resetting a row removes only that override.
            window.click("setting-response-streaming-reset", cx);
            assert_eq!(
                pending(&page, cx)[2],
                json!({ "projectSettingsOverrides": { "p1": { "sidebarAutoSettleAfterDays": null } } })
            );
            // Back at All projects the environment value is untouched.
            page.update(cx, |page, cx| page.set_scope(None, cx));
            assert_eq!(page.read(cx).server_value("responseStreamingMode").value, json!("token"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn search_filters_entries_and_selecting_a_result_opens_its_page(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(700.)));
        load(&page, cx, json!({}), true);
        page.update(cx, |page, _| page.section = Section::Connections);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let search = page.read(cx).search.clone();
            search.update(cx, |search, cx| search.set_value("streaming", window, cx));
            window.render_frame(cx);
            assert!(window.try_find("settings-nav-general").is_none(), "results replace the nav");
            assert!(window.try_find("settings-search-result-0").is_some());
            window.click("settings-search-result-0", cx);
            assert_eq!(page.read(cx).section, Section::General);
            assert!(search.read(cx).value().is_empty(), "choosing a result clears the search");
            window.render_frame(cx);
            assert!(window.try_find("settings-nav-general").is_some());
        })
        .unwrap();
        let results = search::search("shortcut");
        assert!(results.iter().all(|entry| entry.section == Section::Keybindings));
        assert!(results.iter().any(|entry| entry.title == "New thread"));
        // Every word must match; matches in a title lead.
        assert_eq!(search::search("auto settle inactive")[0].title, "Auto-settle inactive threads");
        assert!(search::search("zzzz").is_empty());
        assert!(search::search("  ").is_empty());
    }

    #[gpui_kit::test]
    fn offline_server_settings_are_read_only_with_a_note(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(900.), px(700.)));
        load(&page, cx, json!({ "defaultRuntimeMode": "auto" }), false);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("settings-offline-note").is_some());
            assert!(!page.read(cx).server_ready());
            // The last known value is shown but cannot be changed or reset.
            assert!(window.try_find("setting-default-permissions-reset").is_none());
            page.update(cx, |page, cx| {
                page.set_server_value("responseStreamingMode", json!("turn"), cx);
                page.reset_server_keys(&["defaultRuntimeMode"], cx);
            });
            window.click("settings-restore-defaults", cx);
            assert!(pending(&page, cx).is_empty());
            page.update(cx, |page, cx| page.set_connected(true, cx));
            window.render_frame(cx);
            assert!(window.try_find("settings-offline-note").is_none());
            assert!(window.try_find("setting-default-permissions-reset").is_some());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn narrow_window_keeps_the_nav_and_slash_focuses_search(cx: &mut TestAppContext) {
        let (handle, page) = open_page(cx, size(px(720.), px(480.)));
        load(&page, cx, json!({}), true);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            for section in Section::ALL {
                assert!(window.find(section.id()).visible(), "{}", section.label());
            }
            let search = page.read(cx).search.clone();
            assert!(!search.read(cx).focus_handle(cx).is_focused(window));
            page.update(cx, |page, cx| page.focus(window, cx));
            window.render_frame(cx);
            assert!(page.read(cx).focus_handle.is_focused(window), "page focused");
            window.press("/", cx);
            window.render_frame(cx);
            assert!(search.read(cx).focus_handle(cx).is_focused(window));
        })
        .unwrap();
    }
}
