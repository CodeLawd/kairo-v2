//! Accessibility audit for UI tests (prompt 106).
//!
//! Reads the AccessKit tree a window last sent to the platform, the tree
//! VoiceOver and Narrator see, and checks the rules every Kairo surface must
//! follow:
//!
//! - every control (a pressable, checkable, selectable or editable role, or
//!   any node that takes a click) has a non-empty accessible name; an edit
//!   field may be named by its placeholder, as Chromium names one;
//! - every pressable role (button, check box, switch, tab, menu item, link,
//!   radio button, list or grid option) can be pressed from assistive
//!   technology (`Click`), so VoiceOver's VO-Space and Narrator's Invoke act;
//! - sibling controls do not share a name, so a list of rail buttons or slide
//!   cards can be told apart by ear.
//!
//! GPUI builds the tree only while assistive technology is attached. Test
//! binaries force it with `GPUI_A11Y_ACTIVE=1`, read once per process, so an
//! audit lives in its own test binary (`tests/a11y.rs`) whose every test calls
//! [`activate`] before it opens a window.

use gpui_kit::Window;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// Makes every window in this test process build its accessibility tree.
/// Call first in each test of an audit binary.
pub fn activate() {
    // SAFETY: called before any window exists in an audit binary, whose tests
    // all set the same value; GPUI reads it once per process.
    unsafe { std::env::set_var("GPUI_A11Y_ACTIVE", "1") };
}

/// One node of the tree.
#[derive(Clone, Debug, Default)]
pub struct A11yNode {
    /// The short id the debug dump gives the node.
    pub key: String,
    /// AccessKit role, as its `Debug` name (`Button`, `Navigation`, …).
    pub role: String,
    /// Accessible name.
    pub label: Option<String>,
    /// Accessible description.
    pub description: Option<String>,
    /// Value (text fields, sliders).
    pub value: Option<String>,
    /// Placeholder (text fields).
    pub placeholder: Option<String>,
    /// Actions assistive technology may request.
    pub actions: Vec<String>,
    /// Selected state.
    pub selected: Option<bool>,
    /// Expanded state.
    pub expanded: Option<bool>,
    /// Toggled state (`True`, `False`, `Mixed`).
    pub toggled: Option<String>,
    /// Disabled, as Kit's test observation reports it (the debug dump does
    /// not carry the flag). `false` when the element is not observed.
    pub disabled: bool,
    /// The creating element's id.
    pub element_id: Option<String>,
    /// Where the creating element was built (debug builds).
    pub source: Option<String>,
    /// Children keys, in order.
    pub children: Vec<String>,
}

impl A11yNode {
    /// Whether assistive technology can press it.
    pub fn pressable(&self) -> bool {
        self.actions.iter().any(|a| a == "Click")
    }

    /// Whether it can take keyboard focus.
    pub fn focusable(&self) -> bool {
        self.actions.iter().any(|a| a == "Focus")
    }

    fn name(&self) -> Option<&str> {
        fn named(s: &Option<String>) -> Option<&str> {
            s.as_deref().filter(|s| !s.trim().is_empty())
        }
        named(&self.label).or_else(|| {
            if EDITABLE.contains(&self.role.as_str()) {
                named(&self.placeholder)
            } else {
                None
            }
        })
    }

    fn describe(&self) -> String {
        format!(
            "{} {:?} (element {}, {})",
            self.role,
            self.label.as_deref().unwrap_or(""),
            self.element_id.as_deref().unwrap_or("?"),
            self.source.as_deref().unwrap_or("?"),
        )
    }
}

/// Roles assistive technology presses.
const PRESSABLE: &[&str] = &[
    "Button",
    "DefaultButton",
    "CheckBox",
    "Switch",
    "Tab",
    "MenuItem",
    "MenuItemCheckBox",
    "MenuItemRadio",
    "Link",
    "RadioButton",
    "ListBoxOption",
    "MenuListOption",
    "PopupButton",
    "DisclosureTriangle",
];

/// Roles a person types into.
const EDITABLE: &[&str] = &[
    "TextInput",
    "MultilineTextInput",
    "SearchInput",
    "EmailInput",
    "NumberInput",
    "PasswordInput",
    "PhoneNumberInput",
    "UrlInput",
    "ComboBox",
    "EditableComboBox",
    "SpinButton",
];

/// Other roles that are controls and so need a name.
const CONTROL: &[&str] = &[
    "Slider",
    "ListItem",
    "TreeItem",
    "Row",
    "Cell",
    "GridCell",
    "Canvas",
    "Image",
    "ScrollBar",
    "Splitter",
];

/// Controls whose missing press is inside the pinned GPUI Kit and cannot be
/// added from Kairo (DECISIONS 106): the colour picker's swatch button opens
/// its popover on a mouse press and the Kit `ColorPicker` takes no click or
/// accessibility-action handler. Its name is still checked.
fn kit_limit(node: &A11yNode) -> bool {
    node.source.as_deref().is_some_and(|s| {
        s.contains("gpui-base-") && s.ends_with(".rs:475:19") && s.contains("color_picker.rs")
    })
}

/// Kit's `DatePicker` combo box takes no accessible name from outside
/// (DECISIONS 113); its visible label sits beside it. Fixing it needs a Kit
/// change.
fn kit_unnamed(node: &A11yNode) -> bool {
    node.source.as_deref().is_some_and(|s| {
        s.contains("gpui-base-") && s.ends_with(".rs:34:19") && s.contains("date_picker.rs")
    })
}

/// The last tree a window built.
#[derive(Debug, Default)]
pub struct A11yTree {
    /// Nodes by key.
    pub nodes: BTreeMap<String, A11yNode>,
    /// The root's key.
    pub root: Option<String>,
    /// The focused node's key, as GPUI reports it.
    pub focus: Option<String>,
}

impl A11yTree {
    /// Reads the window's last tree. Panics when no tree was built (the audit
    /// binary did not call [`activate`] before its first window).
    pub fn of(window: &Window) -> Self {
        let json = window
            .debug_a11y_tree_json()
            .expect("no accessibility tree: call a11y_audit::activate() before opening a window");
        let mut tree = Self::parse(&json);
        tree.mark_disabled(window);
        tree
    }

    /// Marks the nodes of disabled controls, which Kit observes.
    fn mark_disabled(&mut self, window: &Window) {
        let disabled: std::collections::HashSet<String> =
            gpui_kit::base::test_support::snapshots(window)
                .into_iter()
                .filter(|s| s.disabled() == Some(true))
                .filter_map(|s| s.path().last().map(|id| format!("{id:?}")))
                .collect();
        for node in self.nodes.values_mut() {
            if node
                .element_id
                .as_ref()
                .is_some_and(|id| disabled.contains(id))
            {
                node.disabled = true;
            }
        }
    }

    fn parse(json: &str) -> Self {
        let value: Value = serde_json::from_str(json).expect("accessibility tree JSON");
        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let mut nodes = BTreeMap::new();
        if let Some(map) = value.get("nodes").and_then(Value::as_object) {
            for (key, node) in map {
                let aria = node.get("aria").cloned().unwrap_or(Value::Null);
                let list = |v: &Value, k: &str| -> Vec<String> {
                    v.get(k)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                nodes.insert(
                    key.clone(),
                    A11yNode {
                        key: key.clone(),
                        role: text(&aria, "role").unwrap_or_default(),
                        label: text(&aria, "label"),
                        description: text(&aria, "description"),
                        value: text(&aria, "value"),
                        placeholder: text(&aria, "placeholder"),
                        actions: list(&aria, "on_action"),
                        selected: aria.get("selected").and_then(Value::as_bool),
                        expanded: aria.get("expanded").and_then(Value::as_bool),
                        toggled: text(&aria, "toggled"),
                        disabled: false,
                        element_id: text(node, "element_id"),
                        source: text(node, "source_location"),
                        children: list(node, "children"),
                    },
                );
            }
        }
        Self {
            nodes,
            root: text(&value, "root"),
            focus: text(&value, "gpui_focus"),
        }
    }

    /// Nodes in depth-first order from the root (reading order).
    pub fn in_order(&self) -> Vec<&A11yNode> {
        let mut out = Vec::new();
        let mut stack: Vec<&str> = self.root.as_deref().into_iter().collect();
        while let Some(key) = stack.pop() {
            let Some(node) = self.nodes.get(key) else {
                continue;
            };
            out.push(node);
            stack.extend(node.children.iter().rev().map(String::as_str));
        }
        out
    }

    /// Nodes with this role, in reading order.
    pub fn with_role(&self, role: &str) -> Vec<&A11yNode> {
        self.in_order()
            .into_iter()
            .filter(|n| n.role == role)
            .collect()
    }

    /// The one node with this name; panics when absent or ambiguous.
    pub fn named(&self, label: &str) -> &A11yNode {
        let found: Vec<_> = self
            .in_order()
            .into_iter()
            .filter(|n| n.label.as_deref() == Some(label))
            .collect();
        assert_eq!(
            found.len(),
            1,
            "expected one node named {label:?}, found {}: {:#?}\nall names: {:?}",
            found.len(),
            found,
            self.names()
        );
        found[0]
    }

    /// Every name, in reading order.
    pub fn names(&self) -> Vec<String> {
        self.in_order()
            .into_iter()
            .filter_map(|n| n.label.clone())
            .collect()
    }

    /// Whether the focused node is the node named `label` or inside it.
    pub fn focus_within(&self, label: &str) -> bool {
        let Some(focus) = self.focus.as_deref() else {
            return false;
        };
        let mut stack = vec![self.named(label).key.as_str()];
        while let Some(key) = stack.pop() {
            if key == focus {
                return true;
            }
            if let Some(node) = self.nodes.get(key) {
                stack.extend(node.children.iter().map(String::as_str));
            }
        }
        false
    }

    /// The focused node.
    pub fn focused(&self) -> Option<&A11yNode> {
        self.focus.as_deref().and_then(|k| self.nodes.get(k))
    }

    /// The rule violations, one line each.
    pub fn violations(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut parents: HashMap<&str, &str> = HashMap::new();
        for node in self.nodes.values() {
            for child in &node.children {
                parents.insert(child.as_str(), node.key.as_str());
            }
        }
        for node in self.in_order() {
            let role = node.role.as_str();
            let control = PRESSABLE.contains(&role)
                || EDITABLE.contains(&role)
                || CONTROL.contains(&role)
                || node.pressable();
            if control && node.name().is_none() && !kit_unnamed(node) {
                let within = parents
                    .get(node.key.as_str())
                    .and_then(|p| self.nodes.get(*p))
                    .map(|p| format!(" within {}", p.describe()))
                    .unwrap_or_default();
                out.push(format!("no accessible name: {}{within}", node.describe()));
            }
            if PRESSABLE.contains(&role) && !node.pressable() && !node.disabled && !kit_limit(node)
            {
                out.push(format!(
                    "cannot be pressed from assistive technology: {}",
                    node.describe()
                ));
            }
        }
        out
    }

    /// Pressable siblings that share a name, so cannot be told apart by
    /// ear. Not part of [`Self::violations`]: Electron repeats per-row action
    /// names ("Reply" on every question) inside unnamed rows too. Use it for
    /// sets whose items must differ, such as the rail or a slide grid.
    pub fn twin_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        for node in self.nodes.values() {
            let mut seen: HashMap<&str, &A11yNode> = HashMap::new();
            for child in &node.children {
                let Some(c) = self.nodes.get(child) else {
                    continue;
                };
                if !(PRESSABLE.contains(&c.role.as_str()) || c.pressable()) {
                    continue;
                }
                if let Some(name) = c.name()
                    && let Some(first) = seen.insert(name, c)
                {
                    out.push(format!(
                        "siblings share the name {name:?}: {} and {}",
                        first.describe(),
                        c.describe()
                    ));
                }
            }
        }
        out
    }

    /// Panics with every violation.
    pub fn assert_accessible(&self) {
        let violations = self.violations();
        assert!(
            violations.is_empty(),
            "{} accessibility violation(s):\n  {}",
            violations.len(),
            violations.join("\n  ")
        );
    }
}

/// Audits the window's last frame from a test harness's draw step, so every
/// state a UI test reaches is checked. Panics on a violation, unless
/// `KAIRO_A11Y_AUDIT_LOG` names a file: then each new violation is appended
/// to it (for surveying a whole suite) and the test goes on.
pub fn check(window: &Window) {
    let Some(json) = window.debug_a11y_tree_json() else {
        return;
    };
    let mut tree = A11yTree::parse(&json);
    tree.mark_disabled(window);
    if let Some(path) = std::env::var_os("KAIRO_A11Y_AUDIT_LOG") {
        use std::io::Write as _;
        let violations = tree.violations();
        if violations.is_empty() {
            return;
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            for line in violations {
                let _ = writeln!(file, "{line}");
            }
        }
        return;
    }
    tree.assert_accessible();
}

/// Reads the window's tree and asserts it follows the rules.
pub fn assert_accessible(window: &Window) -> A11yTree {
    let tree = A11yTree::of(window);
    tree.assert_accessible();
    tree
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "root": "r", "gpui_focus": "a",
      "nodes": {
        "r": {"children": ["a", "b", "c", "d", "e"], "aria": {"role": "Window"}},
        "a": {"element_id": "x", "aria": {"role": "Button", "on_action": ["Click", "Focus"], "label": "Go"}},
        "b": {"aria": {"role": "Button", "on_action": ["Focus"], "label": "Menu"}},
        "c": {"aria": {"role": "Button", "on_action": ["Click"]}},
        "d": {"aria": {"role": "TextInput", "on_action": ["Focus"], "placeholder": "Search"}},
        "e": {"aria": {"role": "Button", "on_action": ["Click"], "label": "Go"}}
      }
    }"#;

    #[test]
    fn finds_unnamed_unpressable_and_twin_controls() {
        let tree = A11yTree::parse(SAMPLE);
        let v = tree.violations();
        assert_eq!(v.len(), 2, "{v:#?}");
        assert!(v[0].starts_with("cannot be pressed") && v[0].contains("Menu"));
        assert!(v[1].starts_with("no accessible name"));
        let twins = tree.twin_names();
        assert_eq!(twins.len(), 1, "{twins:#?}");
        assert!(twins[0].starts_with("siblings share the name \"Go\""));
        assert_eq!(tree.focused().map(|n| n.key.as_str()), Some("a"));
        assert_eq!(
            tree.in_order().iter().map(|n| &*n.key).collect::<Vec<_>>(),
            ["r", "a", "b", "c", "d", "e"]
        );
    }
}
