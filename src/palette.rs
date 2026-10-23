//! The colorway: every color in the tool is defined here, as semantic roles
//! (`accent`, `success`, ...) read by help text, stdout styling and the stderr
//! progress layer. Re-theme by editing the constants; a role is either a named
//! ANSI color via `ansi(..)` or 24-bit via `rgb(..)`.

use clap::builder::styling::{AnsiColor, Color, RgbColor, Style, Styles};

const fn ansi(c: AnsiColor) -> Color {
    Color::Ansi(c)
}

#[expect(dead_code, reason = "available for a 24-bit role")]
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(RgbColor(r, g, b))
}

const ACCENT: Color = ansi(AnsiColor::Blue);
const SUCCESS: Color = ansi(AnsiColor::Green);
const WARNING: Color = ansi(AnsiColor::Yellow);
const DANGER: Color = ansi(AnsiColor::Red);
const MUTED: Color = ansi(AnsiColor::BrightBlack);

/// Headers and the tool's signature accent.
pub fn accent() -> Style {
    Style::new().fg_color(Some(ACCENT)).bold()
}

/// Resolved values and success markers.
pub fn success() -> Style {
    Style::new().fg_color(Some(SUCCESS)).bold()
}

/// Soft alerts and warning markers.
pub fn warning() -> Style {
    Style::new().fg_color(Some(WARNING))
}

/// Errors and destructive prompts.
pub fn danger() -> Style {
    Style::new().fg_color(Some(DANGER)).bold()
}

/// Secondary detail that should recede.
pub fn muted() -> Style {
    Style::new().fg_color(Some(MUTED))
}

/// clap's help styling, derived from the same roles as everything else.
pub fn help() -> Styles {
    Styles::styled()
        .header(accent())
        .usage(accent())
        .literal(success())
        .placeholder(warning())
        .error(danger())
        .valid(success())
        .invalid(warning())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_carry_a_foreground_color() {
        assert!(accent().get_fg_color().is_some());
        assert!(danger().get_fg_color().is_some());
    }
}
