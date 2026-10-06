//! `--jq`: run a jq expression over the JSON a command would print, so a
//! script can pick a field without a second process. jaq implements the
//! language; strings come out raw, like `jq -r`, and every other value as
//! one line of JSON.

use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data, unwrap_valr};
use jaq_json::Val;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "the --jq expression `{0}` does not parse\n  Try:   check its brackets and quotes; jq's manual is at https://jqlang.org/manual/"
    )]
    Syntax(String),
    #[error(
        "the --jq expression uses {0}, which jq does not define\n  Try:   check the spelling and the number of arguments"
    )]
    Undefined(String),
    #[error(
        "the --jq expression failed on this output: {0}\n  Try:   run with --json first to see the shape it gets"
    )]
    Run(String),
}

/// Parse and compile without running, so a bad expression fails before a
/// search spends seconds on the network.
pub fn check(code: &str) -> Result<(), Error> {
    evaluate(code, None).map(|_| ())
}

pub fn run(code: &str, json: &[u8]) -> Result<Vec<String>, Error> {
    let input = jaq_json::read::parse_single(json)
        .map_err(|e| Error::Run(e.to_string()))?;
    evaluate(code, Some(input))
}

/// Compile `code`, then run it on `input` when there is one.
fn evaluate(code: &str, input: Option<Val>) -> Result<Vec<String>, Error> {
    let loader = Loader::new(
        jaq_core::defs().chain(jaq_std::defs()).chain(jaq_json::defs()),
    );
    let arena = Arena::default();
    let modules = loader
        .load(&arena, File { code, path: () })
        .map_err(|_| Error::Syntax(code.to_owned()))?;
    let filter = Compiler::default()
        .with_funs(
            jaq_core::funs().chain(jaq_std::funs()).chain(jaq_json::funs()),
        )
        .compile(modules)
        .map_err(|errors| {
            let names: Vec<String> = errors
                .iter()
                .flat_map(|(_, undefined)| undefined)
                .map(|(name, _)| format!("`{name}`"))
                .collect();
            Error::Undefined(names.join(", "))
        })?;
    let Some(input) = input else { return Ok(Vec::new()) };
    let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));
    filter
        .id
        .run((ctx, input))
        .map(unwrap_valr)
        .map(|value| match value {
            Ok(Val::TStr(text)) => Ok(inert(&jaq_json::bstr(&*text))),
            Ok(other) => Ok(inert(&other)),
            Err(e) => Err(Error::Run(inert(&e))),
        })
        .collect()
}

/// A raw string can carry text a remote page put there, so every control
/// character except newline and tab becomes a space before it can reach a
/// terminal.
fn inert(text: &impl std::fmt::Display) -> String {
    text.to_string()
        .chars()
        .map(
            |c| if c.is_control() && c != '\n' && c != '\t' { ' ' } else { c },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCES: &[u8] =
        br#"{"sources":[{"id":"zenodo","n":1},{"id":"hf","n":2}]}"#;

    #[test]
    fn strings_come_out_raw_and_values_as_json() {
        assert_eq!(run(".sources[].id", SOURCES).unwrap(), ["zenodo", "hf"]);
        assert_eq!(
            run(".sources[0]", SOURCES).unwrap(),
            [r#"{"id":"zenodo","n":1}"#]
        );
        assert_eq!(run(".sources | map(.n) | add", SOURCES).unwrap(), ["3"]);
    }

    // inspect passes a page's own JSON-LD through, so a title may carry ESC.
    #[test]
    fn raw_strings_cannot_drive_the_terminal() {
        let page = br#"{"name":"rain\u001b[2J\u0007 data\nset"}"#;
        assert_eq!(run(".name", page).unwrap(), ["rain [2J  data\nset"]);
    }

    #[test]
    fn bad_expressions_are_named_before_anything_runs() {
        assert!(matches!(check(".["), Err(Error::Syntax(_))));
        assert!(
            matches!(check("nope(1)"), Err(Error::Undefined(n)) if n.contains("nope"))
        );
        assert!(check(".sources[] | select(.n > 1)").is_ok());
    }
}
