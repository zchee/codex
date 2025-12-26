use ansi_to_tui::Error;
use ansi_to_tui::IntoText;
use ratatui::text::Line;
use ratatui::text::Text;

// Expand tabs in a best-effort way for transcript rendering.
// Tabs can interact poorly with left-gutter prefixes in our TUI and CLI
// transcript views (e.g., `nl` separates line numbers from content with a tab).
// Replacing tabs with spaces avoids odd visual artifacts without changing
// semantics for our use cases.
pub const TAB_WIDTH: usize = 4;

fn expand_tabs(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\t') {
        // Keep it simple: replace each tab with fixed-width spaces.
        // We do not try to align to tab stops since most usages (like `nl`)
        // look acceptable with a fixed substitution and this avoids stateful math
        // across spans.
        let spaces = " ".repeat(TAB_WIDTH);
        std::borrow::Cow::Owned(s.replace('\t', &spaces))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// This function should be used when the contents of `s` are expected to match
/// a single line. If multiple lines are found, a warning is logged and only the
/// first line is returned.
pub fn ansi_escape_line(s: &str) -> Line<'static> {
    // Normalize tabs to spaces to avoid odd gutter collisions in transcript mode.
    let s = expand_tabs(s);
    let text = ansi_escape(&s);
    match text.lines.as_slice() {
        [] => "".into(),
        [only] => only.clone(),
        [first, rest @ ..] => {
            tracing::warn!("ansi_escape_line: expected a single line, got {first:?} and {rest:?}");
            first.clone()
        }
    }
}

pub fn ansi_escape(s: &str) -> Text<'static> {
    // to_text() claims to be faster, but introduces complex lifetime issues
    // such that it's not worth it.
    match s.into_text() {
        Ok(text) => text,
        Err(err) => match err {
            Error::NomError(message) => {
                tracing::error!(
                    "ansi_to_tui NomError docs claim should never happen when parsing `{s}`: {message}"
                );
                panic!();
            }
            Error::Utf8Error(utf8error) => {
                tracing::error!("Utf8Error: {utf8error}");
                panic!();
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::TAB_WIDTH;
    use super::expand_tabs;

    #[test]
    fn expand_tabs_replaces_with_four_spaces() {
        let expanded = expand_tabs("a\tb\tc");
        let spaces = " ".repeat(TAB_WIDTH);
        assert_eq!(expanded.as_ref(), format!("a{spaces}b{spaces}c"));
    }

    #[test]
    fn expand_tabs_no_tabs_returns_original() {
        let expanded = expand_tabs("no tabs here");
        assert_eq!(expanded.as_ref(), "no tabs here");
    }
}
