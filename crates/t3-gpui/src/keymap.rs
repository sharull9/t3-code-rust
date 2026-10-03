//! User-editable shortcuts for the app-level actions.
//!
//! Storage, per command (`Command::upstream`):
//!
//! | Native action     | Upstream id      | Stored                                  |
//! |-------------------|------------------|-----------------------------------------|
//! | `NewThread`       | `chat.new`       | server (`server.upsertKeybinding`)      |
//! | `ToggleSidebar`   | `sidebar.toggle` | server                                  |
//! | `FocusComposer`   | none             | device prefs (`Prefs::keybindings`)     |
//! | `ToggleWorkspace` | none             | device prefs                            |
//! | `ShowSettings`    | none             | device prefs                            |
//! | `DismissModal`    | none             | device prefs                            |
//!
//! Commands with an upstream id are shared with the web app: the effective
//! rules arrive in `ServerConfig::keybindings` and edits go through the
//! server. The rest only exist in this app, so the server has no id for them.
//!
//! Shortcuts are kept in upstream syntax (`mod+shift+n`, `mod` is Ctrl, or
//! Cmd on macOS) and converted to GPUI keystrokes (`ctrl-shift-n`) by
//! [`Shortcut::to_gpui`]. [`apply`] rebuilds the GPUI bindings whenever the
//! server rules or the device overrides change.

use std::collections::{BTreeSet, HashMap};

use gpui_kit::*;
use t3_client::ResolvedKeybinding;
use t3_client::keybindings::{KeybindingOp, Rule};

use crate::app::{DismissModal, FocusComposer, NewThread, ShowSettings, ToggleSidebar, ToggleWorkspace};
use crate::prefs::Prefs;

const MAC: bool = cfg!(target_os = "macos");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    NewThread,
    ToggleSidebar,
    FocusComposer,
    ToggleWorkspace,
    ShowSettings,
    DismissModal,
}

impl Command {
    pub const ALL: [Self; 6] = [
        Self::NewThread,
        Self::ToggleSidebar,
        Self::FocusComposer,
        Self::ToggleWorkspace,
        Self::ShowSettings,
        Self::DismissModal,
    ];

    /// Stable key for prefs, element ids and tests.
    pub fn id(self) -> &'static str {
        match self {
            Self::NewThread => "new-thread",
            Self::ToggleSidebar => "toggle-sidebar",
            Self::FocusComposer => "focus-composer",
            Self::ToggleWorkspace => "toggle-workspace",
            Self::ShowSettings => "show-settings",
            Self::DismissModal => "dismiss-modal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NewThread => "New thread",
            Self::ToggleSidebar => "Toggle sidebar",
            Self::FocusComposer => "Focus composer",
            Self::ToggleWorkspace => "Toggle workspace",
            Self::ShowSettings => "Open or close Settings",
            Self::DismissModal => "Close or go back",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::NewThread => "Start a new thread. Shared with the web app.",
            Self::ToggleSidebar => "Show or hide the thread sidebar. Shared with the web app.",
            Self::FocusComposer => "Move focus to the message composer.",
            Self::ToggleWorkspace => "Show or hide the file and terminal workspace.",
            Self::ShowSettings => "Open Settings, or go back when it is open.",
            Self::DismissModal => "Close the open dialog, picker, or Settings page.",
        }
    }

    /// The command id the server and the web app use, if there is one.
    pub fn upstream(self) -> Option<&'static str> {
        match self {
            Self::NewThread => Some("chat.new"),
            Self::ToggleSidebar => Some("sidebar.toggle"),
            _ => None,
        }
    }

    /// The `when` clause upstream puts on this command's default rules.
    fn upstream_when(self) -> Option<&'static str> {
        match self {
            Self::NewThread => Some("!terminalFocus"),
            _ => None,
        }
    }

    /// Default shortcuts (upstream's `DEFAULT_KEYBINDINGS` for server commands).
    pub fn defaults(self) -> &'static [&'static str] {
        match self {
            Self::NewThread => &["mod+n", "mod+shift+o"],
            Self::ToggleSidebar => &["mod+b"],
            Self::FocusComposer => &["mod+l"],
            Self::ToggleWorkspace => &["mod+j"],
            Self::ShowSettings => &["mod+,"],
            Self::DismissModal => &["escape"],
        }
    }

    fn binding(self, keystrokes: &str) -> KeyBinding {
        let context = Some("T3App");
        match self {
            Self::NewThread => KeyBinding::new(keystrokes, NewThread, context),
            Self::ToggleSidebar => KeyBinding::new(keystrokes, ToggleSidebar, context),
            Self::FocusComposer => KeyBinding::new(keystrokes, FocusComposer, context),
            Self::ToggleWorkspace => KeyBinding::new(keystrokes, ToggleWorkspace, context),
            Self::ShowSettings => KeyBinding::new(keystrokes, ShowSettings, context),
            Self::DismissModal => KeyBinding::new(keystrokes, DismissModal, context),
        }
    }
}

/// Shortcuts that other components own and the user cannot reassign.
pub const FIXED: &[(&str, &str)] = &[
    ("mod+1", "Choose answer or item 1"),
    ("mod+2", "Choose answer or item 2"),
    ("mod+3", "Choose answer or item 3"),
    ("mod+4", "Choose item 4"),
    ("mod+5", "Choose item 5"),
    ("mod+6", "Choose item 6"),
    ("mod+7", "Choose item 7"),
    ("mod+8", "Choose item 8"),
    ("mod+9", "Choose item 9"),
    ("/", "Search Settings"),
];

/// One key combination, independent of syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub primary: bool,
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Lowercase upstream key name: `n`, `arrowup`, `space`, `escape`.
    pub key: String,
}

const KEY_NAMES: &[(&str, &str)] = &[
    ("arrowup", "up"),
    ("arrowdown", "down"),
    ("arrowleft", "left"),
    ("arrowright", "right"),
];

impl Shortcut {
    /// Parses upstream syntax (`mod+shift+n`, `mod++`, `esc`).
    pub fn parse_upstream(source: &str) -> Option<Self> {
        if source.trim().is_empty() {
            return None;
        }
        let mut tokens: Vec<String> =
            source.to_lowercase().split('+').map(|token| token.trim().to_owned()).collect();
        let mut trailing = 0;
        while tokens.last().is_some_and(String::is_empty) {
            tokens.pop();
            trailing += 1;
        }
        if trailing > 0 {
            tokens.push("+".into());
        }
        if tokens.is_empty() || tokens.iter().any(String::is_empty) {
            return None;
        }
        let mut shortcut = Self {
            primary: false,
            meta: false,
            ctrl: false,
            alt: false,
            shift: false,
            key: String::new(),
        };
        let mut key = None;
        for token in tokens {
            match token.as_str() {
                "cmd" | "meta" => shortcut.meta = true,
                "ctrl" | "control" => shortcut.ctrl = true,
                "shift" => shortcut.shift = true,
                "alt" | "option" => shortcut.alt = true,
                "mod" => shortcut.primary = true,
                _ => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(if token == "esc" { "escape".to_owned() } else { token });
                }
            }
        }
        shortcut.key = key?;
        Some(shortcut)
    }

    /// Upstream syntax in canonical modifier order (`mod meta ctrl alt shift`).
    pub fn to_upstream(&self) -> String {
        let mut parts = Vec::new();
        for (on, name) in [
            (self.primary, "mod"),
            (self.meta, "meta"),
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// GPUI keystroke syntax (`ctrl-shift-n`) for the given platform.
    pub fn to_gpui_for(&self, mac: bool) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.primary || self.meta {
            parts.push(if self.primary && !mac { "ctrl" } else { "cmd" });
        }
        if self.ctrl && !(self.primary && !mac) {
            parts.push("ctrl");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.shift {
            parts.push("shift");
        }
        let key = KEY_NAMES
            .iter()
            .find(|(upstream, _)| *upstream == self.key)
            .map_or(self.key.as_str(), |(_, gpui)| gpui);
        parts.push(key);
        parts.join("-")
    }

    pub fn to_gpui(&self) -> String {
        self.to_gpui_for(MAC)
    }

    /// The shortcut a typed keystroke records, for the given platform.
    /// `None` for a bare modifier press.
    pub fn from_keystroke_for(keystroke: &Keystroke, mac: bool) -> Option<Self> {
        let modifiers = &keystroke.modifiers;
        if modifiers.function
            || matches!(keystroke.key.as_str(), "shift" | "control" | "alt" | "platform" | "function")
            || keystroke.key.is_empty()
        {
            return None;
        }
        let key = KEY_NAMES
            .iter()
            .find(|(_, gpui)| *gpui == keystroke.key)
            .map_or(keystroke.key.as_str(), |(upstream, _)| upstream);
        Some(Self {
            primary: if mac { modifiers.platform } else { modifiers.control },
            meta: !mac && modifiers.platform,
            ctrl: mac && modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
            key: key.to_owned(),
        })
    }

    pub fn from_keystroke(keystroke: &Keystroke) -> Option<Self> {
        Self::from_keystroke_for(keystroke, MAC)
    }

    /// Why this shortcut would be a bad global binding, if it would.
    pub fn problem(&self) -> Option<&'static str> {
        let modified = self.primary || self.meta || self.ctrl || self.alt;
        let function_key = self.key.len() > 1
            && self.key.starts_with('f')
            && self.key[1..].chars().all(|c| c.is_ascii_digit());
        if !modified && !function_key && self.key != "escape" {
            return Some("Include Ctrl, Alt, or Cmd so typing keeps working.");
        }
        Keystroke::parse(&self.to_gpui()).err().map(|_| "That key cannot be bound.")
    }
}

/// Canonical upstream string, or the input unchanged when it does not parse.
fn normalize(source: &str) -> String {
    Shortcut::parse_upstream(source).map_or_else(|| source.to_owned(), |s| s.to_upstream())
}

/// The effective rules the server reported (empty until connected).
#[derive(Default, Clone)]
pub struct ServerKeybindings(pub Vec<ResolvedKeybinding>);

impl Global for ServerKeybindings {}

/// Identifiers a `when` clause can use that hold in this app: it has no
/// terminal or preview focus, and runs on the desktop.
fn context_holds(name: &str) -> bool {
    matches!(name, "isDesktop" | "true")
}

/// The shortcuts that currently trigger `command`, in upstream syntax.
pub fn shortcuts(command: Command, server: &[ResolvedKeybinding], prefs: &Prefs) -> Vec<String> {
    let Some(upstream) = command.upstream() else {
        return match prefs.keybindings.get(command.id()) {
            Some(custom) => custom.iter().map(|s| normalize(s)).collect(),
            None => command.defaults().iter().map(|s| normalize(s)).collect(),
        };
    };
    let rules: Vec<&ResolvedKeybinding> =
        server.iter().filter(|rule| rule.command == upstream).collect();
    if rules.is_empty() && server.is_empty() {
        // Not connected yet: the defaults, so shortcuts work offline.
        return command.defaults().iter().map(|s| normalize(s)).collect();
    }
    rules
        .into_iter()
        .filter(|rule| rule.when_holds(context_holds))
        .map(|rule| normalize(&rule.key()))
        .collect()
}

pub fn is_modified(command: Command, server: &[ResolvedKeybinding], prefs: &Prefs) -> bool {
    let current: BTreeSet<String> = shortcuts(command, server, prefs).into_iter().collect();
    let default: BTreeSet<String> = command.defaults().iter().map(|s| normalize(s)).collect();
    current != default
}

/// What already uses `shortcut`: another command's label, or a fixed one.
pub fn conflict(
    command: Command,
    shortcut: &Shortcut,
    server: &[ResolvedKeybinding],
    prefs: &Prefs,
) -> Option<(Option<Command>, &'static str)> {
    let wanted = shortcut.to_upstream();
    for other in Command::ALL {
        if other != command && shortcuts(other, server, prefs).contains(&wanted) {
            return Some((Some(other), other.label()));
        }
    }
    FIXED
        .iter()
        .find(|(fixed, _)| normalize(fixed) == wanted)
        .map(|(_, label)| (None, *label))
}

/// The rule to send for `command`'s primary shortcut after rebinding it.
fn upstream_rule(command: Command, key: &str) -> Option<Rule> {
    Some(Rule {
        key: key.to_owned(),
        command: command.upstream()?.to_owned(),
        when: command.upstream_when().map(str::to_owned),
    })
}

/// Server operations that make `shortcut` the only shortcut of a server
/// command. The server drops the command's defaults once it has a custom rule.
pub fn rebind_ops(
    command: Command,
    shortcut: &Shortcut,
    server: &[ResolvedKeybinding],
    prefs: &Prefs,
) -> Vec<KeybindingOp> {
    let current = shortcuts(command, server, prefs);
    let Some(rule) = upstream_rule(command, &shortcut.to_upstream()) else {
        return Vec::new();
    };
    let replace = current.first().and_then(|key| upstream_rule(command, key));
    vec![KeybindingOp::Upsert { rule, replace }]
}

/// Server operations that remove every custom rule of `command` (defaults
/// return once none is left).
pub fn reset_ops(command: Command, server: &[ResolvedKeybinding]) -> Vec<KeybindingOp> {
    let Some(upstream) = command.upstream() else { return Vec::new() };
    server
        .iter()
        .filter(|rule| rule.command == upstream)
        .map(|rule| {
            KeybindingOp::Remove(Rule {
                key: rule.key(),
                command: upstream.to_owned(),
                when: rule.when(),
            })
        })
        .collect()
}

pub fn set_server_keybindings(cx: &mut App, rules: Vec<ResolvedKeybinding>) {
    if cx.try_global::<ServerKeybindings>().is_some_and(|current| current.0 == rules) {
        return;
    }
    cx.set_global(ServerKeybindings(rules));
    apply(cx);
}

pub fn server_keybindings(cx: &App) -> Vec<ResolvedKeybinding> {
    cx.try_global::<ServerKeybindings>().map(|rules| rules.0.clone()).unwrap_or_default()
}

/// Sets a device-local override (an empty list is not a valid override).
pub fn set_device_override(cx: &mut App, command: Command, shortcut: Option<&Shortcut>) {
    Prefs::update(cx, |prefs| match shortcut {
        Some(shortcut) => {
            prefs.keybindings.insert(command.id().to_owned(), vec![shortcut.to_upstream()]);
        }
        None => {
            prefs.keybindings.remove(command.id());
        }
    });
    apply(cx);
}

/// Rebuilds the GPUI key bindings of every [`Command`]. Bindings registered
/// by other components are kept.
pub fn apply(cx: &mut App) {
    let server = server_keybindings(cx);
    let prefs = Prefs::global(cx).clone();
    let ours: HashMap<&'static str, ()> = [
        NewThread.name(),
        ToggleSidebar.name(),
        FocusComposer.name(),
        ToggleWorkspace.name(),
        ShowSettings.name(),
        DismissModal.name(),
    ]
    .into_iter()
    .map(|name| (name, ()))
    .collect();
    let keymap = cx.key_bindings();
    let mut bindings: Vec<KeyBinding> = keymap
        .borrow()
        .bindings()
        .filter(|binding| !ours.contains_key(binding.action().name()))
        .cloned()
        .collect();
    for command in Command::ALL {
        for shortcut in shortcuts(command, &server, &prefs) {
            let Some(shortcut) = Shortcut::parse_upstream(&shortcut) else { continue };
            let keystrokes = shortcut.to_gpui();
            if Keystroke::parse(&keystrokes).is_ok() {
                bindings.push(command.binding(&keystrokes));
            }
        }
    }
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    cx.bind_keys([KeyBinding::new("ctrl-i", crate::app::ToggleInfo, Some("T3App"))]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use serde_json::json;

    fn rule(command: &str, key: &str, when: Option<&str>) -> ResolvedKeybinding {
        let mut shortcut =
            json!({ "key": "", "metaKey": false, "ctrlKey": false, "shiftKey": false,
                     "altKey": false, "modKey": false });
        for token in key.split('+') {
            match token {
                "mod" => shortcut["modKey"] = true.into(),
                "shift" => shortcut["shiftKey"] = true.into(),
                "alt" => shortcut["altKey"] = true.into(),
                "ctrl" => shortcut["ctrlKey"] = true.into(),
                other => shortcut["key"] = other.into(),
            }
        }
        let mut value = json!({ "command": command, "shortcut": shortcut });
        if let Some(when) = when {
            value["whenAst"] = match when.strip_prefix('!') {
                Some(name) => json!({ "type": "not", "node": { "type": "identifier", "name": name } }),
                None => json!({ "type": "identifier", "name": when }),
            };
        }
        serde_json::from_value(value).unwrap()
    }

    fn defaults() -> Vec<ResolvedKeybinding> {
        vec![
            rule("chat.new", "mod+n", Some("!terminalFocus")),
            rule("chat.new", "mod+shift+o", Some("!terminalFocus")),
            rule("sidebar.toggle", "mod+b", None),
            rule("terminal.new", "mod+n", Some("terminalFocus")),
        ]
    }

    #[test]
    fn upstream_shortcuts_convert_to_gpui_and_back() {
        let cases = [
            ("mod+n", "ctrl-n", "cmd-n"),
            ("mod+shift+o", "ctrl-shift-o", "cmd-shift-o"),
            ("mod+alt+b", "ctrl-alt-b", "cmd-alt-b"),
            ("mod+,", "ctrl-,", "cmd-,"),
            ("mod+-", "ctrl--", "cmd--"),
            ("mod+=", "ctrl-=", "cmd-="),
            ("mod+shift+arrowup", "ctrl-shift-up", "cmd-shift-up"),
            ("escape", "escape", "escape"),
            ("meta+k", "cmd-k", "cmd-k"),
        ];
        for (upstream, windows, mac) in cases {
            let shortcut = Shortcut::parse_upstream(upstream).unwrap();
            assert_eq!(shortcut.to_gpui_for(false), windows, "{upstream}");
            assert_eq!(shortcut.to_gpui_for(true), mac, "{upstream}");
            assert!(Keystroke::parse(windows).is_ok(), "{windows}");
            assert_eq!(shortcut.to_upstream(), upstream);
            let typed = Keystroke::parse(windows).unwrap();
            assert_eq!(Shortcut::from_keystroke_for(&typed, false), Some(shortcut), "{windows}");
        }
        assert_eq!(Shortcut::parse_upstream("mod++").unwrap().key, "+");
        assert_eq!(Shortcut::parse_upstream("esc").unwrap().key, "escape");
        assert_eq!(Shortcut::parse_upstream("cmd+option+k").unwrap().to_upstream(), "meta+alt+k");
        assert!(Shortcut::parse_upstream("a+b").is_none());
        assert!(Shortcut::parse_upstream("").is_none());
        // On macOS, Ctrl stays Ctrl and Cmd becomes the primary modifier.
        let typed = Keystroke::parse("cmd-ctrl-k").unwrap();
        assert_eq!(Shortcut::from_keystroke_for(&typed, true).unwrap().to_upstream(), "mod+ctrl+k");
    }

    #[test]
    fn bare_modifiers_and_bare_letters_are_rejected() {
        let modifier = Keystroke { key: "shift".into(), key_char: None, modifiers: Modifiers::default() };
        assert!(Shortcut::from_keystroke(&modifier).is_none());
        assert!(Shortcut::parse_upstream("n").unwrap().problem().is_some());
        assert!(Shortcut::parse_upstream("shift+n").unwrap().problem().is_some());
        assert!(Shortcut::parse_upstream("f5").unwrap().problem().is_none());
        assert!(Shortcut::parse_upstream("mod+n").unwrap().problem().is_none());
    }

    #[test]
    fn effective_shortcuts_use_server_rules_for_shared_commands() {
        let prefs = Prefs::default();
        assert_eq!(shortcuts(Command::NewThread, &[], &prefs), ["mod+n", "mod+shift+o"]);
        assert_eq!(shortcuts(Command::NewThread, &defaults(), &prefs), ["mod+n", "mod+shift+o"]);
        let custom = vec![rule("chat.new", "mod+shift+n", Some("!terminalFocus"))];
        assert_eq!(shortcuts(Command::NewThread, &custom, &prefs), ["mod+shift+n"]);
        assert!(is_modified(Command::NewThread, &custom, &prefs));
        assert!(!is_modified(Command::NewThread, &defaults(), &prefs));
        assert!(!is_modified(Command::ToggleSidebar, &defaults(), &prefs));
        // Rules whose `when` needs terminal focus never fire here.
        let terminal = vec![rule("chat.new", "mod+t", Some("terminalFocus"))];
        assert!(shortcuts(Command::NewThread, &terminal, &prefs).is_empty());
    }

    #[test]
    fn native_commands_read_device_overrides() {
        let mut prefs = Prefs::default();
        assert_eq!(shortcuts(Command::FocusComposer, &[], &prefs), ["mod+l"]);
        prefs.keybindings.insert("focus-composer".into(), vec!["mod+shift+l".into()]);
        assert_eq!(shortcuts(Command::FocusComposer, &[], &prefs), ["mod+shift+l"]);
        assert!(is_modified(Command::FocusComposer, &[], &prefs));
    }

    #[test]
    fn conflicts_name_the_other_command_or_a_fixed_shortcut() {
        let prefs = Prefs::default();
        let server = defaults();
        let ctrl_b = Shortcut::parse_upstream("mod+b").unwrap();
        assert_eq!(
            conflict(Command::NewThread, &ctrl_b, &server, &prefs),
            Some((Some(Command::ToggleSidebar), "Toggle sidebar"))
        );
        // Rebinding a command to its own shortcut is not a conflict.
        assert_eq!(conflict(Command::ToggleSidebar, &ctrl_b, &server, &prefs), None);
        let slot = Shortcut::parse_upstream("mod+2").unwrap();
        assert_eq!(
            conflict(Command::NewThread, &slot, &server, &prefs).map(|(command, _)| command),
            Some(None)
        );
        let free = Shortcut::parse_upstream("mod+shift+y").unwrap();
        assert_eq!(conflict(Command::NewThread, &free, &server, &prefs), None);
        // Native-only commands are checked too.
        let ctrl_j = Shortcut::parse_upstream("mod+j").unwrap();
        assert_eq!(
            conflict(Command::NewThread, &ctrl_j, &server, &prefs).map(|(command, _)| command),
            Some(Some(Command::ToggleWorkspace))
        );
    }

    #[test]
    fn rebind_and_reset_build_server_operations() {
        let prefs = Prefs::default();
        let shortcut = Shortcut::parse_upstream("mod+shift+n").unwrap();
        let ops = rebind_ops(Command::NewThread, &shortcut, &defaults(), &prefs);
        assert_eq!(
            ops,
            vec![KeybindingOp::Upsert {
                rule: Rule {
                    key: "mod+shift+n".into(),
                    command: "chat.new".into(),
                    when: Some("!terminalFocus".into())
                },
                replace: Some(Rule {
                    key: "mod+n".into(),
                    command: "chat.new".into(),
                    when: Some("!terminalFocus".into())
                }),
            }]
        );
        assert!(rebind_ops(Command::FocusComposer, &shortcut, &[], &prefs).is_empty());
        let reset = reset_ops(Command::ToggleSidebar, &defaults());
        assert_eq!(reset.len(), 1);
        assert!(matches!(&reset[0], KeybindingOp::Remove(rule) if rule.key == "mod+b"));
        assert!(reset_ops(Command::ShowSettings, &defaults()).is_empty());
    }
}
