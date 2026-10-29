//! Export des resultats : CSV (`;`, UTF-8), JSON ou texte (un chemin par ligne).

use chrono::{Local, TimeZone};

use crate::index::{Id, Index};
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Json,
    Txt,
}

impl Format {
    pub fn from_extension(ext: &str) -> Option<Format> {
        match ext.to_lowercase().as_str() {
            "csv" => Some(Format::Csv),
            "json" => Some(Format::Json),
            "txt" | "" => Some(Format::Txt),
            _ => None,
        }
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn date(mtime: i64) -> String {
    Local
        .timestamp_opt(mtime, 0)
        .single()
        .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_default()
}

pub fn export(ix: &Index, ids: &[Id], fmt: Format) -> Result<String, Error> {
    Ok(match fmt {
        Format::Txt => ids.iter().map(|&i| ix.path(i) + "\n").collect(),
        Format::Csv => {
            let mut out = String::from("nom;chemin;type;taille;modifie\r\n");
            for &i in ids {
                let (size, mt) = ix.stat(i);
                let dir = ix.is_dir(i);
                out.push_str(&format!(
                    "{};{};{};{};{}\r\n",
                    csv_field(ix.name(i)),
                    csv_field(&ix.path(i)),
                    if dir { "dossier" } else { "fichier" },
                    if dir {
                        String::from("-1")
                    } else {
                        size.to_string()
                    },
                    if mt > 1 { date(mt) } else { String::new() }
                ));
            }
            out
        }
        Format::Json => {
            let rows: Vec<_> = ids
                .iter()
                .map(|&i| {
                    let (size, mt) = ix.stat(i);
                    serde_json::json!({
                        "nom": ix.name(i), "chemin": ix.path(i),
                        "type": if ix.is_dir(i) { "dossier" } else { "fichier" },
                        "taille": if ix.is_dir(i) { -1 } else { size as i64 },
                        "modifie": if mt > 1 { date(mt) } else { String::new() },
                    })
                })
                .collect();
            serde_json::to_string_pretty(&rows)?
        }
    })
}
