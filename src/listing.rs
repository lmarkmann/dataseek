//! `sources`: the registry as a table, with each source's key status read
//! from the environment (never the key itself).

use std::io::Write;

use anyhow::Result;
use serde::Serialize;

use crate::credentials::Credentials;
use crate::output::Out;
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
    writeln!(
        w,
        "{head}{:<22} {:<22} {:<22} {:<6} key{head:#}",
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
            "{:<22} {:<22} {:<22} {:<6} {key}",
            r.id, r.category, r.protocol, r.search
        )?;
    }
    writeln!(w)?;
    writeln!(
        w,
        "{muted}{} sources. `local` ones download their catalog once a week and search it on disk.{muted:#}",
        rows.len()
    )?;
    for r in &rows {
        if let Some(reason) = r.opt_in {
            writeln!(
                w,
                "{muted}{} is asked only when named with -s: {reason}.{muted:#}",
                r.id
            )?;
        }
    }
    Ok(())
}
