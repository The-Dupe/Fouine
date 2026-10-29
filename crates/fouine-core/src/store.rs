//! Historique (100 requetes uniques, la plus recente d'abord) et favoris, en JSON.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Error;

pub const HISTORY_MAX: usize = 100;

#[derive(Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    history: Vec<String>,
    #[serde(default)]
    bookmarks: BTreeMap<String, String>,
}

pub struct Store {
    path: PathBuf,
    data: Data,
}

impl Store {
    /// Ouvre le fichier ; un fichier absent ou corrompu donne un magasin vide.
    pub fn open(path: impl Into<PathBuf>) -> Store {
        let path = path.into();
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Store { path, data }
    }

    fn save(&self) -> Result<(), Error> {
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&self.data)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn history(&self) -> &[String] {
        &self.data.history
    }

    pub fn record(&mut self, query: &str) -> Result<(), Error> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(());
        }
        self.data.history.retain(|h| h != q);
        self.data.history.insert(0, q.to_string());
        self.data.history.truncate(HISTORY_MAX);
        self.save()
    }

    pub fn clear_history(&mut self) -> Result<(), Error> {
        self.data.history.clear();
        self.save()
    }

    pub fn bookmarks(&self) -> &BTreeMap<String, String> {
        &self.data.bookmarks
    }

    pub fn set_bookmark(&mut self, name: &str, query: &str) -> Result<(), Error> {
        if name.trim().is_empty() || query.trim().is_empty() {
            return Err(Error::Query("nom et requete requis".into()));
        }
        self.data
            .bookmarks
            .insert(name.trim().to_string(), query.trim().to_string());
        self.save()
    }

    pub fn delete_bookmark(&mut self, name: &str) -> Result<bool, Error> {
        let found = self.data.bookmarks.remove(name).is_some();
        if found {
            self.save()?;
        }
        Ok(found)
    }
}
