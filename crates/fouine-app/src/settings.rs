//! Reglages (volumes et dossiers indexes, exclusions, theme, arriere-plan) et emplacement des donnees.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fouine_core::exclusions::Exclusions;
use fouine_core::volumes;
use serde_json::{json, Value};

pub struct Settings {
    /// Volumes entiers (`C:\`) et dossiers indexes.
    pub roots: Vec<String>,
    /// Dossiers exclus, avec tout leur contenu.
    pub exclude: Vec<String>,
    /// Motifs de noms de fichiers exclus (`*.tmp`, `~$*`).
    pub exclude_patterns: Vec<String>,
    pub exclude_hidden: bool,
    pub exclude_system: bool,
    /// Ajoute automatiquement les volumes fixes / amovibles detectes (comme Everything).
    pub auto_fixed: bool,
    pub auto_removable: bool,
    /// Volumes que l'utilisateur a decoches : l'inclusion automatique ne les remet pas.
    pub volumes_unchecked: Vec<String>,
    /// `systeme`, `clair` ou `sombre`.
    pub theme: String,
    /// Fermer la fenetre la reduit dans la zone de notification au lieu de quitter (surveillance continue).
    pub tray: bool,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Dossier de donnees impose (mode service) ; prioritaire sur `FOUINE_HOME` et `%APPDATA%`.
static HOME_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

pub fn set_home(dir: PathBuf) {
    let _ = HOME_OVERRIDE.set(dir);
}

/// Donnees partagees avec le service : `%ProgramData%\Fouine` (`FOUINE_SERVICE_HOME` pour les tests).
pub fn service_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("FOUINE_SERVICE_HOME") {
        return PathBuf::from(d);
    }
    std::env::var_os("ProgramData")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Fouine")
}

pub fn service_db() -> PathBuf {
    service_dir().join("index.db")
}

/// `C:\` ou `C:` : un volume entier ?
pub fn is_volume_root(r: &str) -> bool {
    let r = r.trim_end_matches(['\\', '/']);
    r.len() == 2 && r.ends_with(':') && r.as_bytes()[0].is_ascii_alphabetic()
}

fn same_ci(a: &str, b: &str) -> bool {
    a.trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.trim_end_matches(['\\', '/']))
}

impl Settings {
    /// Dossier des donnees : `FOUINE_HOME`, sinon le dossier de l'executable s'il est modifiable,
    /// sinon `%APPDATA%\Fouine` / `~/.local/share/fouine`.
    pub fn dir(&self) -> PathBuf {
        data_dir()
    }

    pub fn db_path(&self) -> PathBuf {
        data_dir().join("index.db")
    }

    pub fn load() -> Settings {
        let _ = std::fs::create_dir_all(data_dir());
        let v: Value = std::fs::read_to_string(data_dir().join("reglages.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let list = |k: &str| -> Option<Vec<String>> {
            v.get(k)?.as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
        };
        let flag = |k: &str, defaut: bool| v.get(k).and_then(Value::as_bool).unwrap_or(defaut);
        Settings {
            roots: list("dossiers")
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| vec![home().to_string_lossy().into_owned()]),
            exclude: list("exclusions").unwrap_or_default(),
            exclude_patterns: list("exclusions_motifs").unwrap_or_default(),
            exclude_hidden: flag("exclure_caches", false),
            exclude_system: flag("exclure_systeme", false),
            auto_fixed: flag("volumes_auto_fixes", false),
            auto_removable: flag("volumes_auto_amovibles", false),
            volumes_unchecked: list("volumes_decoches").unwrap_or_default(),
            theme: v
                .get("theme")
                .and_then(Value::as_str)
                .unwrap_or("systeme")
                .to_string(),
            tray: flag("zone_notification", true),
        }
    }

    pub fn save(&self) {
        self.save_in(&data_dir());
    }

    /// Ecrit les reglages dans `dir` (utilise aussi pour preparer les donnees du service).
    pub fn save_in(&self, dir: &Path) {
        let v = json!({
            "dossiers": self.roots,
            "exclusions": self.exclude,
            "exclusions_motifs": self.exclude_patterns,
            "exclure_caches": self.exclude_hidden,
            "exclure_systeme": self.exclude_system,
            "volumes_auto_fixes": self.auto_fixed,
            "volumes_auto_amovibles": self.auto_removable,
            "volumes_decoches": self.volumes_unchecked,
            "theme": self.theme,
            "zone_notification": self.tray,
        });
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(
            dir.join("reglages.json"),
            serde_json::to_string_pretty(&v).unwrap_or_default(),
        );
    }

    /// Regles d'exclusion de l'indexation.
    pub fn exclusions(&self) -> Exclusions {
        Exclusions::new(
            &self.exclude,
            &self.exclude_patterns,
            self.exclude_hidden,
            self.exclude_system,
        )
    }

    /// Racines reellement indexees : les racines choisies, plus les volumes ajoutes automatiquement (sauf ceux
    /// decoches), sans les dossiers deja contenus dans un volume entier retenu (pas de double indexation).
    pub fn effective_roots(&self) -> Vec<String> {
        let mut roots: Vec<String> = Vec::new();
        for r in &self.roots {
            if !roots.iter().any(|x| same_ci(x, r)) {
                roots.push(r.clone());
            }
        }
        for v in volumes::list() {
            let auto = (v.fixed && self.auto_fixed) || (v.removable && self.auto_removable);
            let ajoute = roots.iter().any(|r| same_ci(r, &v.root));
            let decoche = self.volumes_unchecked.iter().any(|r| same_ci(r, &v.root));
            if auto && !ajoute && !decoche {
                roots.push(v.root.clone());
            }
        }
        let entiers: Vec<String> = roots
            .iter()
            .filter(|r| is_volume_root(r))
            .cloned()
            .collect();
        roots.retain(|r| {
            is_volume_root(r)
                || !entiers.iter().any(|v| {
                    let v = v.trim_end_matches(['\\', '/']).to_lowercase();
                    let p = r.to_lowercase();
                    p == v || p.starts_with(&format!("{v}\\")) || p.starts_with(&format!("{v}/"))
                })
        });
        roots
    }
}

fn data_dir() -> PathBuf {
    if let Some(d) = HOME_OVERRIDE.get() {
        return d.clone();
    }
    if let Some(d) = std::env::var_os("FOUINE_HOME") {
        return PathBuf::from(d);
    }
    if let Some(d) = portable_dir() {
        return d.clone();
    }
    if let Some(a) = std::env::var_os("APPDATA") {
        return PathBuf::from(a).join("Fouine");
    }
    home().join(".local").join("share").join("fouine")
}

/// Dossier de l'executable, s'il est modifiable : la base (`index.db`), les reglages et les favoris
/// s'y trouvent alors, a cote de `fouine.exe`. Sinon (ex. Program Files), on retombe sur `%APPDATA%`.
fn portable_dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        let probe = dir.join(".fouine-ecriture");
        std::fs::write(&probe, b"").ok()?;
        let _ = std::fs::remove_file(&probe);
        Some(dir)
    })
    .as_ref()
}
