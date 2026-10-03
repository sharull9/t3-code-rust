//! Server-owned keybindings (`server.upsertKeybinding` /
//! `server.removeKeybinding`). Shortcut strings use upstream's `mod+shift+n`
//! syntax; `packages/contracts/src/keybindings.ts` defines the wire format.

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Shortcut {
    key: String,
    #[serde(default)]
    meta_key: bool,
    #[serde(default)]
    ctrl_key: bool,
    #[serde(default)]
    shift_key: bool,
    #[serde(default)]
    alt_key: bool,
    #[serde(default)]
    mod_key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum WhenNode {
    Identifier { name: String },
    Not { node: Box<WhenNode> },
    And { left: Box<WhenNode>, right: Box<WhenNode> },
    Or { left: Box<WhenNode>, right: Box<WhenNode> },
}

/// One effective keybinding rule as the server reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedKeybinding {
    pub command: String,
    shortcut: Shortcut,
    #[serde(default)]
    when_ast: Option<WhenNode>,
}

impl ResolvedKeybinding {
    /// The shortcut in upstream's config syntax, e.g. `mod+shift+n`.
    pub fn key(&self) -> String {
        let s = &self.shortcut;
        let mut parts = Vec::new();
        for (on, name) in [
            (s.mod_key, "mod"),
            (s.meta_key, "meta"),
            (s.ctrl_key, "ctrl"),
            (s.alt_key, "alt"),
            (s.shift_key, "shift"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(if s.key == " " { "space" } else { &s.key });
        parts.join("+")
    }

    /// The `when` clause as an expression (`!terminalFocus`).
    pub fn when(&self) -> Option<String> {
        self.when_ast.as_ref().map(when_expression)
    }

    /// Whether the clause holds given which identifiers are true.
    pub fn when_holds(&self, is_true: impl Fn(&str) -> bool) -> bool {
        fn eval(node: &WhenNode, is_true: &dyn Fn(&str) -> bool) -> bool {
            match node {
                WhenNode::Identifier { name } => is_true(name),
                WhenNode::Not { node } => !eval(node, is_true),
                WhenNode::And { left, right } => eval(left, is_true) && eval(right, is_true),
                WhenNode::Or { left, right } => eval(left, is_true) || eval(right, is_true),
            }
        }
        self.when_ast.as_ref().is_none_or(|node| eval(node, &is_true))
    }
}

fn when_expression(node: &WhenNode) -> String {
    match node {
        WhenNode::Identifier { name } => name.clone(),
        WhenNode::Not { node } => match &**node {
            WhenNode::Identifier { name } => format!("!{name}"),
            inner => format!("!({})", when_expression(inner)),
        },
        WhenNode::And { left, right } => {
            format!("({} && {})", when_expression(left), when_expression(right))
        }
        WhenNode::Or { left, right } => {
            format!("({} || {})", when_expression(left), when_expression(right))
        }
    }
}

/// A shortcut rule as sent to the server: the `{key, command, when}` triple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub key: String,
    pub command: String,
    pub when: Option<String>,
}

impl Rule {
    fn to_value(&self) -> Value {
        let mut value = json!({ "key": self.key, "command": self.command });
        if let Some(when) = &self.when {
            value["when"] = json!(when);
        }
        value
    }
}

/// One step of a keybinding edit, applied in order by
/// [`crate::Connection::apply_keybinding_ops`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeybindingOp {
    /// Saves `rule`; `replace` names the custom rule it supersedes.
    Upsert { rule: Rule, replace: Option<Rule> },
    /// Drops a custom rule by exact match (a no-op for a default rule).
    Remove(Rule),
}

impl KeybindingOp {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Upsert { .. } => "server.upsertKeybinding",
            Self::Remove(_) => "server.removeKeybinding",
        }
    }

    pub fn payload(&self) -> Value {
        match self {
            Self::Upsert { rule, replace } => {
                let mut value = rule.to_value();
                if let Some(replace) = replace {
                    value["replace"] = replace.to_value();
                }
                value
            }
            Self::Remove(rule) => rule.to_value(),
        }
    }
}

/// Decodes the `keybindings` of an upsert/remove result, dropping rules this
/// client cannot represent (the server's command set grows over time).
pub fn decode_result(value: &Value) -> Vec<ResolvedKeybinding> {
    value["keybindings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|rule| serde_json::from_value(rule.clone()).ok())
        .collect()
}

impl crate::Connection {
    /// Runs `ops` in order and returns the effective keybindings after the last.
    pub async fn apply_keybinding_ops(
        &self,
        ops: &[KeybindingOp],
    ) -> Result<Vec<ResolvedKeybinding>, crate::RpcError> {
        let mut keybindings = Vec::new();
        for op in ops {
            let value = self.rpc().call::<Value>(op.method(), op.payload()).await?;
            keybindings = decode_result(&value);
        }
        Ok(keybindings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(key: &str, command: &str, when: Option<&str>) -> Rule {
        Rule { key: key.into(), command: command.into(), when: when.map(Into::into) }
    }

    #[test]
    fn resolved_rules_decode_and_encode_upstream_syntax() {
        let rule: ResolvedKeybinding = serde_json::from_value(json!({
            "command": "chat.new",
            "shortcut": { "key": "n", "metaKey": false, "ctrlKey": false, "shiftKey": true,
                          "altKey": false, "modKey": true },
            "whenAst": { "type": "not", "node": { "type": "identifier", "name": "terminalFocus" } }
        }))
        .unwrap();
        assert_eq!(rule.key(), "mod+shift+n");
        assert_eq!(rule.when().as_deref(), Some("!terminalFocus"));
        assert!(rule.when_holds(|_| false));
        assert!(!rule.when_holds(|name| name == "terminalFocus"));
        let space: ResolvedKeybinding = serde_json::from_value(json!({
            "command": "x", "shortcut": { "key": " ", "metaKey": false, "ctrlKey": true,
            "shiftKey": false, "altKey": false, "modKey": false }
        }))
        .unwrap();
        assert_eq!(space.key(), "ctrl+space");
        assert_eq!(space.when(), None);
    }

    #[test]
    fn config_drops_rules_it_cannot_decode() {
        let config: crate::ServerConfig = serde_json::from_value(json!({ "keybindings": [
            { "command": "sidebar.toggle", "shortcut": { "key": "b", "metaKey": false,
              "ctrlKey": false, "shiftKey": false, "altKey": false, "modKey": true } },
            { "command": "future", "shortcut": 7 }
        ]}))
        .unwrap();
        assert_eq!(config.keybindings.len(), 1);
    }

    #[test]
    fn keybindings_updated_event_replaces_rules() {
        let mut current = None;
        let applied = crate::quotas::apply_config_event(
            &mut current,
            json!({ "type": "keybindingsUpdated", "payload": { "issues": [], "keybindings": [
                { "command": "sidebar.toggle", "shortcut": { "key": "x", "metaKey": false,
                  "ctrlKey": true, "shiftKey": false, "altKey": false, "modKey": false } }
            ]}}),
        )
        .unwrap();
        assert!(applied);
        assert_eq!(current.unwrap().keybindings[0].key(), "ctrl+x");
    }

    #[test]
    fn ops_build_upsert_with_replace_and_remove_payloads() {
        let upsert = KeybindingOp::Upsert {
            rule: rule("mod+shift+n", "chat.new", Some("!terminalFocus")),
            replace: Some(rule("mod+n", "chat.new", Some("!terminalFocus"))),
        };
        assert_eq!(upsert.method(), "server.upsertKeybinding");
        assert_eq!(
            upsert.payload(),
            json!({ "key": "mod+shift+n", "command": "chat.new", "when": "!terminalFocus",
                    "replace": { "key": "mod+n", "command": "chat.new", "when": "!terminalFocus" } })
        );
        let remove = KeybindingOp::Remove(rule("mod+b", "sidebar.toggle", None));
        assert_eq!(remove.method(), "server.removeKeybinding");
        assert_eq!(remove.payload(), json!({ "key": "mod+b", "command": "sidebar.toggle" }));
    }

    #[test]
    fn results_skip_unknown_rules() {
        let result = json!({ "keybindings": [1, { "command": "a", "shortcut": { "key": "a" } }] });
        assert_eq!(decode_result(&result).len(), 1);
    }
}
