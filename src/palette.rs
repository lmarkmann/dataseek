//! The colorway: the one place every color in the tool is defined.
//!
//! Re-theme the whole CLI by editing the role constants below. Nothing else
//! hardcodes a color; help text, stdout styling, and the stderr progress layer
//! all read these roles. Roles are semantic (`accent`, `success`, `danger`),
//! never literal ("cyan"), so swapping a color is never a hunt for the right
//! shade at each call site.
//!
//! Colors are `anstyle` values (re-exported through clap, so no extra
//! dependency). A role can be a portable named ANSI color via `ansi(..)` or a
//! 24-bit hex via `rgb(0x7a, 0xa2, 0xf7)`. Both render exactly everywhere,
//! including the indicatif progress layer, which parses its color token with
//! `console::Style::from_dotted_str` and understands names, `#rrggbb`, and a
//! bare 256-color index alike. To apply a role to text, lean on anstyle's
//! inline form: `write!(w, "{s}{value}{s:#}", s = palette::success())`, where
//! `{s:#}` emits the reset. `anstream` strips these escapes when the stream is
//! not a terminal.

use clap::builder::styling::{AnsiColor, Color, RgbColor, Style, Styles};

const fn ansi(c: AnsiColor) -> Color {
    Color::Ansi(c)
}

#[expect(
    dead_code,
    reason = "colorway knob: swap a role to rgb(..) to re-theme"
)]
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(RgbColor(r, g, b))
}

// The colorway. Edit these five lines to recolor the whole tool.
const ACCENT: Color = ansi(AnsiColor::Blue); // headers, the signature hue
const SUCCESS: Color = ansi(AnsiColor::Green); // values, "it worked"
const WARNING: Color = ansi(AnsiColor::Yellow); // soft alerts, placeholders
const DANGER: Color = ansi(AnsiColor::Red); // errors, destructive prompts
const MUTED: Color = ansi(AnsiColor::BrightBlack); // secondary detail

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

/// The accent as an indicatif progress-template color token. indicatif hands
/// the token to `console::Style::from_dotted_str`, which understands named
/// colors, `#rrggbb` truecolor, and a bare 256-color index, so every role
/// shape renders exactly and the bar matches help and stdout.
pub fn accent_token() -> String {
    token_for(ACCENT)
}

fn token_for(color: Color) -> String {
    match color {
        Color::Ansi(c) => ansi_name(c).to_owned(),
        Color::Rgb(RgbColor(r, g, b)) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Ansi256(c) => c.index().to_string(),
    }
}

fn ansi_name(c: AnsiColor) -> &'static str {
    match c {
        AnsiColor::Black | AnsiColor::BrightBlack => "black",
        AnsiColor::Red | AnsiColor::BrightRed => "red",
        AnsiColor::Green | AnsiColor::BrightGreen => "green",
        AnsiColor::Yellow | AnsiColor::BrightYellow => "yellow",
        AnsiColor::Blue | AnsiColor::BrightBlue => "blue",
        AnsiColor::Magenta | AnsiColor::BrightMagenta => "magenta",
        AnsiColor::Cyan | AnsiColor::BrightCyan => "cyan",
        AnsiColor::White | AnsiColor::BrightWhite => "white",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_accent_maps_exactly() {
        assert_eq!(ansi_name(AnsiColor::Cyan), "cyan");
        assert_eq!(ansi_name(AnsiColor::BrightCyan), "cyan");
    }

    // Every shape a role can take has to survive console's parser, which is
    // what indicatif runs the token through; one it rejects drops the color
    // silently.
    #[test]
    fn every_role_shape_produces_a_token_console_accepts() {
        use clap::builder::styling::Ansi256Color;

        assert_eq!(token_for(Color::Ansi(AnsiColor::Blue)), "blue");
        assert_eq!(
            token_for(Color::Rgb(RgbColor(0x7a, 0xa2, 0xf7))),
            "#7aa2f7"
        );
        assert_eq!(token_for(Color::Ansi256(Ansi256Color(42))), "42");
    }

    #[test]
    fn roles_carry_a_foreground_color() {
        assert!(accent().get_fg_color().is_some());
        assert!(danger().get_fg_color().is_some());
    }
}
