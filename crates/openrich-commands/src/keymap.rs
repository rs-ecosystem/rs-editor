//! Key bindings (`prosemirror-keymap`) and the base keymap.

use std::collections::HashMap;
use std::fmt;

use openrich_state::{
    Command, Dispatch, EditorState, ViewContext, chain_commands, command, reborrow,
};

use crate::base::*;

/// Which platform conventions a keymap follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyPlatform {
    /// macOS and iOS: `Mod` is Command.
    Apple,
    /// Windows: Ctrl-Alt may be AltGr.
    Windows,
    /// Linux, Android, ChromeOS and others: `Mod` is Ctrl.
    Other,
}

/// Modifier state of a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Command on Apple keyboards, the Windows/Super key elsewhere.
    pub meta: bool,
}

/// A key press as reported by a platform adapter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// W3C `KeyboardEvent.key` value: `"a"`, `"A"`, `"Enter"`, `"ArrowLeft"`, …
    pub key: String,
    /// W3C `KeyboardEvent.code` (physical key), e.g. `"KeyA"`, if known.
    pub code: Option<String>,
    pub modifiers: Modifiers,
}

impl KeyEvent {
    pub fn new(key: &str, modifiers: Modifiers) -> Self {
        Self {
            key: key.to_owned(),
            code: None,
            modifiers,
        }
    }
}

/// Normalizes a binding such as `"Mod-Shift-z"` to the canonical
/// `"Shift-Meta-z"` (Apple) or `"Shift-Ctrl-z"` form.
pub fn normalize_key_name(name: &str, platform: KeyPlatform) -> Result<String, String> {
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = name;
    // Split on '-' except a trailing one, so "Mod--" binds the minus key.
    while let Some(index) = rest.find('-') {
        if index + 1 == rest.len() {
            break;
        }
        parts.push(&rest[..index]);
        rest = &rest[index + 1..];
    }
    let mut result = if rest == "Space" {
        " ".to_owned()
    } else {
        rest.to_owned()
    };
    let (mut alt, mut ctrl, mut shift, mut meta) = (false, false, false, false);
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "cmd" | "meta" | "m" => meta = true,
            "a" | "alt" => alt = true,
            "c" | "ctrl" | "control" => ctrl = true,
            "s" | "shift" => shift = true,
            "mod" => {
                if platform == KeyPlatform::Apple {
                    meta = true;
                } else {
                    ctrl = true;
                }
            }
            other => return Err(format!("unrecognized modifier name: {other}")),
        }
    }
    if alt {
        result = format!("Alt-{result}");
    }
    if ctrl {
        result = format!("Ctrl-{result}");
    }
    if meta {
        result = format!("Meta-{result}");
    }
    if shift {
        result = format!("Shift-{result}");
    }
    Ok(result)
}

fn with_modifiers(name: &str, modifiers: Modifiers, include_shift: bool) -> String {
    let mut name = name.to_owned();
    if modifiers.alt {
        name = format!("Alt-{name}");
    }
    if modifiers.ctrl {
        name = format!("Ctrl-{name}");
    }
    if modifiers.meta {
        name = format!("Meta-{name}");
    }
    if include_shift && modifiers.shift {
        name = format!("Shift-{name}");
    }
    name
}

/// The unshifted ASCII character of a physical key code, used when a
/// modifier or keyboard layout changed the produced character.
fn base_name_of(code: &str) -> Option<String> {
    if let Some(letter) = code.strip_prefix("Key")
        && letter.len() == 1
    {
        return Some(letter.to_ascii_lowercase());
    }
    if let Some(digit) = code.strip_prefix("Digit")
        && digit.len() == 1
    {
        return Some(digit.to_owned());
    }
    let named = match code {
        "Minus" => "-",
        "Equal" => "=",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "Backquote" => "`",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        _ => return None,
    };
    Some(named.to_owned())
}

/// A set of key bindings.
#[derive(Clone)]
pub struct Keymap {
    bindings: HashMap<String, Command>,
    platform: KeyPlatform,
}

impl fmt::Debug for Keymap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut keys: Vec<_> = self.bindings.keys().collect();
        keys.sort();
        f.debug_struct("Keymap")
            .field("platform", &self.platform)
            .field("bindings", &keys)
            .finish()
    }
}

impl Keymap {
    /// Builds a keymap; binding names use ProseMirror syntax (`"Mod-b"`).
    /// Later bindings for the same key are chained after earlier ones.
    pub fn new(bindings: Vec<(&str, Command)>, platform: KeyPlatform) -> Result<Self, String> {
        let mut map: HashMap<String, Vec<Command>> = HashMap::new();
        for (name, cmd) in bindings {
            map.entry(normalize_key_name(name, platform)?)
                .or_default()
                .push(cmd);
        }
        let bindings = map
            .into_iter()
            .map(|(name, mut cmds)| {
                let cmd = if cmds.len() == 1 {
                    cmds.remove(0)
                } else {
                    chain_commands(cmds)
                };
                (name, cmd)
            })
            .collect();
        Ok(Self { bindings, platform })
    }

    pub fn platform(&self) -> KeyPlatform {
        self.platform
    }

    pub fn get(&self, name: &str) -> Option<&Command> {
        self.bindings.get(name)
    }

    /// Runs the binding for `event`, if one applies.
    pub fn handle(
        &self,
        event: &KeyEvent,
        state: &EditorState,
        mut dispatch: Dispatch,
        view: Option<&dyn ViewContext>,
    ) -> bool {
        let name = &event.key;
        let mut run = |key: &str| {
            self.bindings
                .get(key)
                .is_some_and(|cmd| cmd(state, reborrow(&mut dispatch), view))
        };
        if run(&with_modifiers(name, event.modifiers, true)) {
            return true;
        }
        if name.chars().count() == 1 && name != " " {
            if event.modifiers.shift && run(&with_modifiers(name, event.modifiers, false)) {
                return true;
            }
            let m = event.modifiers;
            let alt_gr = self.platform == KeyPlatform::Windows && m.ctrl && m.alt;
            if (m.alt || m.meta || m.ctrl)
                && !alt_gr
                && let Some(base) = event.code.as_deref().and_then(base_name_of)
                && &base != name
                && run(&with_modifiers(&base, m, true))
            {
                return true;
            }
        }
        false
    }
}

fn backspace() -> Command {
    chain_commands(vec![
        command(delete_selection),
        command(join_backward),
        command(select_node_backward),
    ])
}

fn delete() -> Command {
    chain_commands(vec![
        command(delete_selection),
        command(join_forward),
        command(select_node_forward),
    ])
}

/// Enter, Backspace, Delete and select-all bindings every editor needs,
/// plus the Emacs-style bindings Apple platforms expect.
pub fn base_keymap(platform: KeyPlatform) -> Keymap {
    let enter = chain_commands(vec![
        command(newline_in_code),
        command(create_paragraph_near),
        command(lift_empty_block),
        command(split_block),
    ]);
    let mut bindings: Vec<(&str, Command)> = vec![
        ("Enter", enter),
        ("Mod-Enter", command(exit_code)),
        ("Backspace", backspace()),
        ("Mod-Backspace", backspace()),
        ("Shift-Backspace", backspace()),
        ("Delete", delete()),
        ("Mod-Delete", delete()),
        ("Mod-a", command(select_all)),
    ];
    if platform == KeyPlatform::Apple {
        bindings.extend([
            ("Ctrl-h", backspace()),
            ("Alt-Backspace", backspace()),
            ("Ctrl-d", delete()),
            ("Ctrl-Alt-Backspace", delete()),
            ("Alt-Delete", delete()),
            ("Alt-d", delete()),
            ("Ctrl-a", command(select_textblock_start)),
            ("Ctrl-e", command(select_textblock_end)),
        ]);
    }
    Keymap::new(bindings, platform).unwrap_or_else(|_| Keymap {
        bindings: HashMap::new(),
        platform,
    })
}
