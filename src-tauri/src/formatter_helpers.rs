//! Shared text-formatting helpers for INI value patches.
//!
//! Star Citizen's `global.ini` rendering pipeline interprets a small
//! markup vocabulary: emphasis tags `<EM0>`..`<EM4>` for color, and
//! `\n` literals for line breaks (the INI parser sees the two
//! characters `\` `n`, not a real newline byte). The helpers here
//! centralise that vocabulary so module code reads in the same shape
//! the player ultimately sees, and color/line-break choices live in
//! one place.

// ── Line break literals ────────────────────────────────────────────────────
//
// INI values are single-line, so a literal `\n` (backslash + n) is what
// the renderer uses to break lines on screen. Rust's escape `"\\n"`
// produces those two characters.

/// Single in-value line break — renders as a newline in the game.
pub const NEWLINE: &str = "\\n";

/// Blank line / paragraph break between sections.
pub const PARAGRAPH_BREAK: &str = "\\n\\n";

// ── Color tags ─────────────────────────────────────────────────────────────

/// In-game emphasis levels. Maps to the `<EMn>` tag set the SC HUD
/// renderer recognises.
///
/// Variants are named by **player-visible intent in the contracts
/// panel** — that's where the mission-enhancer writes, and the
/// only context where these markers reliably render distinctly.
/// The same tag does also show up in chat / notifications with
/// different colors, but that's an incidental rendering pass we
/// don't target.
///
/// | Variant      | Tag   | Contracts panel | Chat (incidental) |
/// |--------------|-------|-----------------|-------------------|
/// | `Plain`      | `EM0` | default         | white             |
/// | `Faint`      | `EM1` | default         | cyan              |
/// | `Soft`       | `EM2` | default         | green             |
/// | `Underline`  | `EM3` | underlined      | yellow            |
/// | `Highlight`  | `EM4` | blue accent     | red               |
///
/// In contracts, only `Underline` and `Highlight` render distinctly
/// from `Plain`. `Faint` and `Soft` exist for completeness; their
/// chat-context colors are observable but the contracts panel
/// treats them as default text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Plain,
    Faint,
    Soft,
    Underline,
    Highlight,
}

impl Color {
    fn to_tag(&self) -> &'static str {
        match self {
            Self::Plain => "EM0",
            Self::Faint => "EM1",
            Self::Soft => "EM2",
            Self::Underline => "EM3",
            Self::Highlight => "EM4",
        }
    }

    fn tag_open(&self) -> String {
        format!("<{}>", self.to_tag())
    }

    fn tag_close(&self) -> String {
        format!("</{}>", self.to_tag())
    }
}

/// Wrap `text` in a color tag pair. Accepts anything string-like for
/// caller ergonomics (`&str`, `String`, `&String`).
pub fn apply_color(color: Color, text: impl AsRef<str>) -> String {
    format!("{}{}{}", color.tag_open(), text.as_ref(), color.tag_close())
}

// ── Compound helpers ───────────────────────────────────────────────────────

/// Standard section-header label — wrapped in `Color::Highlight`,
/// used for `Mission Info`, `Encounters`, `Variants`, etc.
/// Centralised so the emphasis choice can move in one place if we
/// ever revisit it.
pub fn header(label: impl AsRef<str>) -> String {
    apply_color(Color::Highlight, label)
}

/// Wrap text in square brackets — the title-tag convention
/// (`[BP]`, `[Solo]`, `[Uniq]`, `[~]`).
pub fn bracket(label: impl AsRef<str>) -> String {
    format!("[{}]", label.as_ref())
}

/// A list-item line: `"- {text}"`. The leading hyphen + space is the
/// shape the renderer uses for bullet lists in mission descriptions.
pub fn bullet(text: impl AsRef<str>) -> String {
    format!("- {}", text.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_wraps_in_red() {
        assert_eq!(header("Mission Info"), "<EM4>Mission Info</EM4>");
    }

    #[test]
    fn bracket_wraps_in_square_brackets() {
        assert_eq!(bracket("BP"), "[BP]");
        assert_eq!(bracket("CS Risk!"), "[CS Risk!]");
    }

    #[test]
    fn bullet_prefixes_dash_space() {
        assert_eq!(bullet("Bracer"), "- Bracer");
    }

    #[test]
    fn apply_color_works_on_owned_and_borrowed() {
        assert_eq!(apply_color(Color::Highlight, "x"), "<EM4>x</EM4>");
        let s = String::from("y");
        assert_eq!(apply_color(Color::Underline, &s), "<EM3>y</EM3>");
    }
}
