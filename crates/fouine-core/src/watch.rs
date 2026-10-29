//! Surveillance du systeme de fichiers : applique les changements a un index partage.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::exclusions::Exclusions;
use crate::Index;

/// Reflete l'etat actuel d'un chemin dans l'index : present sur le disque -> ajoute ou rafraichi,
/// absent -> retire avec ses descendants. Retourne vrai si l'index a change.
pub fn sync_path(ix: &mut Index, path: &Path) -> bool {
    if path.exists() {
        ix.add_path(path).is_some()
    } else if let Some(id) = ix.lookup(&path.to_string_lossy()) {
        ix.remove_tree(id) > 0
    } else {
        false
    }
}

/// Garde un index a jour tant que l'objet existe. Les dossiers surveilles doivent deja etre indexes.
pub struct Watcher {
    _watchers: Vec<RecommendedWatcher>,
    pub failed: Vec<(PathBuf, String)>,
}

impl Watcher {
    pub fn start(
        index: Arc<RwLock<Index>>,
        roots: &[PathBuf],
        exclusions: Arc<Exclusions>,
        on_change: impl Fn() + Send + Sync + 'static,
    ) -> Watcher {
        let on_change = Arc::new(on_change);
        let (mut watchers, mut failed) = (Vec::new(), Vec::new());
        for root in roots {
            let (ix, cb, ex) = (
                Arc::clone(&index),
                Arc::clone(&on_change),
                Arc::clone(&exclusions),
            );
            let handler = move |res: notify::Result<notify::Event>| {
                let Ok(ev) = res else { return };
                if matches!(ev.kind, EventKind::Access(_) | EventKind::Other) {
                    return;
                }
                let mut changed = false;
                // Les chemins exclus (dossier, motif, cache, systeme) ne sont pas suivis ; verifie hors du verrou.
                let paths: Vec<&PathBuf> =
                    ev.paths.iter().filter(|p| !ex.path_excluded(p)).collect();
                if let Ok(mut ix) = ix.write() {
                    for p in paths {
                        changed |= sync_path(&mut ix, p);
                    }
                }
                if changed {
                    cb();
                }
            };
            match notify::recommended_watcher(handler)
                .and_then(|mut w| w.watch(root, RecursiveMode::Recursive).map(|()| w))
            {
                Ok(w) => watchers.push(w),
                Err(e) => failed.push((root.clone(), e.to_string())),
            }
        }
        Watcher {
            _watchers: watchers,
            failed,
        }
    }
}
