//! Parcours classique d'un dossier (toutes plateformes, sans droits particuliers).
//!
//! C'est la voie utilisee pour FAT, FAT32, exFAT, ext3/ext4 et tout volume sans table de fichiers lisible
//! directement : seul le parcours des dossiers est possible.
//!
//! Il est parallele : chaque fil lit lui-meme des sous-arbres entiers (jusqu'a `BUDGET` entrees) et renvoie un
//! bloc plat ; un seul fil, le coordinateur, fusionne les blocs dans l'index et redistribue les dossiers
//! restants. Les echanges entre fils ont lieu par bloc, pas par dossier.
//!
//! Mesure sous Windows : une lecture par gros blocs de chaque dossier (API de bas niveau) n'est pas plus rapide
//! que la lecture standard ; le temps est surtout celui du systeme pour ouvrir chaque dossier.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::exclusions::{Exclusions, ATTR_HIDDEN};
use crate::index::{Id, Index, NO_PARENT};

/// Entrees lues par un fil avant de rendre la main au coordinateur.
const BUDGET: usize = 4096;
/// Parent « externe » : l'entree est dans le dossier de depart du bloc.
const NO_LOCAL: u32 = u32::MAX;

/// Un dossier a lire, et l'identifiant de son entree dans l'index.
struct Task {
    path: PathBuf,
    parent: Id,
}

/// Une entree lue dans un dossier.
struct Item {
    name: String,
    is_dir: bool,
    /// Vrai si l'on doit descendre dedans (dossier, hors lien symbolique ou point d'analyse).
    descend: bool,
    size: Option<u64>,
    mtime: Option<i64>,
}

/// Une entree d'un bloc ; `parent_idx` est l'indice du parent dans le meme bloc (ou `NO_LOCAL`).
struct Entry {
    item: Item,
    parent_idx: u32,
}

struct Chunk {
    /// Identifiant (dans l'index) du dossier de depart du bloc.
    parent: Id,
    entries: Vec<Entry>,
    /// Dossiers du bloc restant a lire : chemin et indice local de leur entree.
    unfinished: Vec<(PathBuf, u32)>,
}

/// Nombre de fils de lecture : `FOUINE_THREADS`, sinon 2 par coeur (entre 4 et 32).
fn thread_count() -> usize {
    if let Some(n) = std::env::var("FOUINE_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        return n.clamp(1, 64);
    }
    let cpus = std::thread::available_parallelism().map_or(4, |n| n.get());
    (cpus * 2).clamp(4, 32)
}

/// Lecture d'un dossier, avec les exclusions. Sous Windows, taille, date et attributs viennent gratuitement de
/// l'enumeration ; ailleurs ils ne sont pas lus ici (un appel systeme par fichier) : l'index les lit a la demande.
fn read_dir_items(dir: &Path, excl: &Exclusions) -> Vec<Item> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        let is_dir = ft.is_dir();
        #[cfg(windows)]
        let (size, mtime, attrs) = {
            use std::os::windows::fs::MetadataExt;
            let meta = e.metadata().ok();
            (
                meta.as_ref().map(|m| if is_dir { 0 } else { m.len() }),
                meta.as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64),
                meta.as_ref().map_or(0, |m| m.file_attributes()),
            )
        };
        #[cfg(not(windows))]
        let (size, mtime, attrs) = (
            if is_dir { Some(0) } else { None },
            None,
            if name.starts_with('.') {
                ATTR_HIDDEN
            } else {
                0
            },
        );
        #[cfg(windows)]
        let _ = ATTR_HIDDEN;
        if !excl.is_empty()
            && (excl.attrs_excluded(attrs)
                || (!is_dir && excl.name_excluded(&name))
                || (is_dir
                    && excl.has_folders()
                    && excl.folder_name_candidate(&name)
                    && excl.folder_excluded(&dir.join(&name).to_string_lossy())))
        {
            continue;
        }
        items.push(Item {
            descend: is_dir && !ft.is_symlink(),
            name,
            is_dir,
            size,
            mtime,
        });
    }
    items
}

/// Lit le sous-arbre de `task` jusqu'a `BUDGET` entrees ; les dossiers non lus sont rendus dans `unfinished`.
fn read_chunk(task: Task, excl: &Exclusions) -> Chunk {
    let mut entries: Vec<Entry> = Vec::new();
    let mut unfinished = Vec::new();
    let mut stack: Vec<(PathBuf, u32)> = vec![(task.path, NO_LOCAL)];
    while let Some((path, parent_idx)) = stack.pop() {
        if entries.len() >= BUDGET {
            unfinished.push((path, parent_idx));
            continue;
        }
        for item in read_dir_items(&path, excl) {
            let idx = entries.len() as u32;
            if item.descend {
                stack.push((path.join(&item.name), idx));
            }
            entries.push(Entry { item, parent_idx });
        }
    }
    Chunk {
        parent: task.parent,
        entries,
        unfinished,
    }
}

fn worker(tasks: Arc<Mutex<Receiver<Task>>>, out: Sender<Chunk>, excl: Arc<Exclusions>) {
    loop {
        // Un seul fil attend a la fois sur la file ; les autres attendent le verrou.
        let task = match tasks.lock() {
            Ok(rx) => match rx.recv() {
                Ok(t) => t,
                Err(_) => return, // plus de travail : le coordinateur a ferme la file
            },
            Err(_) => return,
        };
        if out.send(read_chunk(task, &excl)).is_err() {
            return;
        }
    }
}

/// Indexe `root` et tout ce qu'il contient ; `exclude` : prefixes de chemins a ignorer (insensible a la casse).
pub fn scan_tree(ix: &mut Index, root: &Path, exclude: &[String]) -> usize {
    scan_tree_with(ix, root, &Exclusions::from_folders(exclude))
}

/// Comme `scan_tree`, avec toutes les regles d'exclusion (dossiers, motifs de noms, caches, systeme).
pub fn scan_tree_with(ix: &mut Index, root: &Path, exclusions: &Exclusions) -> usize {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let shown = root
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    let top = ix.add(&shown, NO_PARENT, true, Some(0), None);
    let excl: Arc<Exclusions> = Arc::new(exclusions.clone());

    let (task_tx, task_rx) = channel::<Task>();
    let (chunk_tx, chunk_rx) = channel::<Chunk>();
    let task_rx = Arc::new(Mutex::new(task_rx));
    let handles: Vec<_> = (0..thread_count())
        .map(|_| {
            let (t, o, e) = (Arc::clone(&task_rx), chunk_tx.clone(), Arc::clone(&excl));
            std::thread::spawn(move || worker(t, o, e))
        })
        .collect();
    drop(chunk_tx);

    let mut count = 1;
    let mut pending = 1usize; // blocs demandes aux fils dont le resultat n'est pas encore revenu
    let _ = task_tx.send(Task {
        path: root,
        parent: top,
    });
    let profil = std::env::var_os("FOUINE_PROFIL").is_some();
    let (mut attente, mut ajout) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    let (mut nb_blocs, t_debut) = (0usize, std::time::Instant::now());
    while pending > 0 {
        let t_attente = std::time::Instant::now();
        let Ok(chunk) = chunk_rx.recv() else { break };
        let t_ajout = std::time::Instant::now();
        attente += t_ajout - t_attente;
        nb_blocs += 1;
        pending -= 1;
        // Les entrees d'un bloc sont ajoutees dans l'ordre : l'identifiant de l'entree k est `first + k`.
        let first = ix.capacity_ids() as Id;
        for e in &chunk.entries {
            let parent = if e.parent_idx == NO_LOCAL {
                chunk.parent
            } else {
                first + e.parent_idx
            };
            ix.add(
                &e.item.name,
                parent,
                e.item.is_dir,
                e.item.size,
                e.item.mtime,
            );
        }
        count += chunk.entries.len();
        for (path, idx) in chunk.unfinished {
            pending += 1;
            let _ = task_tx.send(Task {
                path,
                parent: first + idx,
            });
        }
        ajout += t_ajout.elapsed();
    }
    if profil {
        eprintln!(
            "profil : {nb_blocs} blocs ; total {:.2} s ; fil constructeur : attente {:.2} s, ajout {:.2} s",
            t_debut.elapsed().as_secs_f64(),
            attente.as_secs_f64(),
            ajout.as_secs_f64()
        );
    }
    drop(task_tx); // ferme la file : les fils se terminent
    for h in handles {
        let _ = h.join();
    }
    count
}
