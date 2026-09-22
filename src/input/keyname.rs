//! Converting between key codes and the names used in the config file.
//!
//! Bindings are stored as readable strings ("left", "space", "x") rather than a
//! serialised key structure, so the config file stays hand-editable — which was
//! the point of putting it in TOML.

use crossterm::event::KeyCode;

pub fn key_name(code: KeyCode) -> String {
    match code {
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::F(n) => format!("f{n}"),
        other => format!("{other:?}").to_lowercase(),
    }
}

pub fn parse_key(name: &str) -> Option<KeyCode> {
    let name = name.trim().to_lowercase();
    let code = match name.as_str() {
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        other => {
            if let Some(digits) = other.strip_prefix('f') {
                if let Ok(n) = digits.parse::<u8>() {
                    return Some(KeyCode::F(n));
                }
            }
            let mut chars = other.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyCode::Char(first)
        }
    };
    Some(code)
}

/// How a key reads in the controls list.
pub fn display_name(code: KeyCode) -> String {
    match code {
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Char(' ') => "Space".into(),
        other => key_name(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_round_trip() {
        let codes = [
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::Char(' '),
            KeyCode::Char('x'),
            KeyCode::Char('z'),
            KeyCode::F(5),
        ];
        for code in codes {
            assert_eq!(parse_key(&key_name(code)), Some(code), "{code:?}");
        }
    }

    #[test]
    fn parsing_is_case_insensitive_and_tolerates_whitespace() {
        assert_eq!(parse_key("  LEFT "), Some(KeyCode::Left));
        assert_eq!(parse_key("Space"), Some(KeyCode::Char(' ')));
        assert_eq!(parse_key("X"), Some(KeyCode::Char('x')));
    }

    #[test]
    fn common_aliases_are_accepted() {
        assert_eq!(parse_key("escape"), Some(KeyCode::Esc));
        assert_eq!(parse_key("return"), Some(KeyCode::Enter));
        assert_eq!(parse_key("del"), Some(KeyCode::Delete));
    }

    #[test]
    fn nonsense_names_are_rejected_rather_than_guessed() {
        assert_eq!(parse_key("wibble"), None);
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("ctrl+x"), None);
    }

    #[test]
    fn arrows_display_as_symbols() {
        assert_eq!(display_name(KeyCode::Left), "←");
        assert_eq!(display_name(KeyCode::Char(' ')), "Space");
        assert_eq!(display_name(KeyCode::Char('z')), "z");
    }
}
