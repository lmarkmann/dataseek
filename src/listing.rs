//! `sources`: the registry as a table, with each source's key status read
//! from the environment (never the key itself).

use std::io::Write;

use anyhow::Result;
use serde::Serialize;

use crate::credentials::Credentials;
use crate::output::{self, Out};
use crate::palette;
use crate::sources::{Need, SOURCES};

#[derive(Serialize)]
struct Row {
    id: &'static str,
    name: &'static str,
    category: String,
    protocol: &'static str,
    search: &'static str,
    /// `none`, `set`, `optional` (not set) or `missing` (required, not set).
    key: &'static str,
    key_env: Option<&'static str>,
    docs: &'static str,
    /// Why the source is asked only when `--source` names it.
    opt_in: Option<&'static str>,
}

pub fn run(out: &Out) -> Result<()> {
    let dirs = crate::paths::resolve()?;
    let creds = Credentials::load(&dirs.config);
    let rows: Vec<Row> = SOURCES
        .iter()
        .map(|s| Row {
            id: s.id,
            name: s.name,
            category: s.category.to_string(),
            protocol: s.protocol,
            search: if s.is_catalog() { "local" } else { "live" },
            key: match s.key {
                None => "none",
                Some((key, _)) if creds.get(key).is_some() => "set",
                Some((_, Need::Optional)) => "optional",
                Some((_, Need::Required)) => "missing",
            },
            key_env: s.key.map(|(key, _)| key.env_var()),
            docs: s.docs,
            opt_in: s.opt_in,
        })
        .collect();

    if out.json {
        return out.json(&serde_json::json!({
            "schema": "dataseek-sources/1",
            "sources": rows,
        }));
    }
    let mut w = out.stdout();
    if out.plain {
        for r in &rows {
            writeln!(
                w,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                r.id,
                r.category,
                r.protocol,
                r.search,
                r.key,
                r.docs,
                r.opt_in.unwrap_or_default()
            )?;
        }
        return Ok(());
    }
    let (head, muted, warn) =
        (palette::accent(), palette::muted(), palette::warning());
    let column = |title: &str, value: &dyn Fn(&Row) -> usize| {
        rows.iter().map(value).max().unwrap_or(0).max(title.len())
    };
    let id = column("id", &|r| r.id.len());
    let category = column("category", &|r| r.category.len());
    let protocol = column("protocol", &|r| r.protocol.len());
    let search = column("search", &|r| r.search.len());
    writeln!(
        w,
        "{head}{:<id$} {:<category$} {:<protocol$} {:<search$} key{head:#}",
        "id", "category", "protocol", "search"
    )?;
    for r in &rows {
        let key = match (r.key, r.key_env) {
            ("missing", Some(var)) => format!("{warn}missing ${var}{warn:#}"),
            ("optional", Some(var)) => {
                format!("{muted}optional ${var}{muted:#}")
            }
            ("set", Some(var)) => format!("set ${var}"),
            _ if r.opt_in.is_some() => format!("{muted}opt-in{muted:#}"),
            _ => String::new(),
        };
        writeln!(
            w,
            "{:<id$} {:<category$} {:<protocol$} {:<search$} {key}",
            r.id, r.category, r.protocol, r.search
        )?;
    }
    writeln!(w)?;
    let mut notes = vec![format!(
        "{} sources. `local` ones download their catalog once a week and search it on disk.",
        rows.len()
    )];
    notes.extend(rows.iter().filter_map(|r| {
        r.opt_in.map(|reason| {
            format!("{} is asked only when named with -s: {reason}.", r.id)
        })
    }));
    // A note that wraps continues two columns in, so each note still
    // starts at the margin.
    let room = output::width().map(|w| w.saturating_sub(2).max(20));
    for note in notes {
        for (i, line) in output::wrap(&note, room).iter().enumerate() {
            let indent = if i == 0 { "" } else { "  " };
            writeln!(w, "{muted}{indent}{line}{muted:#}")?;
        }
    }
    Ok(())
}
