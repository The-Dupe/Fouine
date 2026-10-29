//! Regles d'exclusion de l'indexation, sur le modele des options « Exclure » d'Everything :
//! dossiers (avec tout leur contenu), motifs de noms de fichiers (jokers `*` et `?`), fichiers et dossiers
//! caches, fichiers et dossiers systeme. Les memes regles servent a la lecture directe de la table des fichiers
//! NTFS, au parcours de dossiers et a la surveillance en direct.

use std::path::Path;

/// Attribut Windows « cache ».
pub const ATTR_HIDDEN: u32 = 0x2;
/// Attribut Windows « systeme ».
pub const ATTR_SYSTEM: u32 = 0x4;

/// Un motif de nom de fichier, avec un chemin rapide pour le cas courant `*.ext`.
#[derive(Debug, Clone)]
struct Motif {
    /// Motif en minuscules.
    glob: String,
    /// Cas `*suffixe` sans autre joker (ASCII) : simple comparaison de fin de nom.
    suffixe: Option<String>,
}

impl Motif {
    fn new(glob: String) -> Motif {
        let suffixe = glob
            .strip_prefix('*')
            .filter(|r| !r.is_empty() && r.is_ascii() && !r.contains(['*', '?']))
            .map(String::from);
        Motif { glob, suffixe }
    }

    fn correspond(&self, nom: &str) -> bool {
        if let Some(suf) = &self.suffixe {
            let (n, l) = (nom.as_bytes(), suf.as_bytes());
            return n.len() >= l.len() && n[n.len() - l.len()..].eq_ignore_ascii_case(l);
        }
        if nom.is_ascii() {
            return glob_match_ascii(self.glob.as_bytes(), nom.as_bytes());
        }
        glob_match(&self.glob, &nom.to_lowercase())
    }
}

/// Comme `glob_match`, sur des octets ASCII, sans tenir compte de la casse du texte (motif deja en minuscules).
fn glob_match_ascii(m: &[u8], t: &[u8]) -> bool {
    let (mut i, mut j) = (0usize, 0usize);
    let (mut etoile, mut reprise) = (None, 0usize);
    while j < t.len() {
        if i < m.len() && (m[i] == b'?' || m[i] == t[j].to_ascii_lowercase()) {
            i += 1;
            j += 1;
        } else if i < m.len() && m[i] == b'*' {
            etoile = Some(i);
            i += 1;
            reprise = j;
        } else if let Some(e) = etoile {
            i = e + 1;
            reprise += 1;
            j = reprise;
        } else {
            return false;
        }
    }
    while i < m.len() && m[i] == b'*' {
        i += 1;
    }
    i == m.len()
}

#[derive(Debug, Clone, Default)]
pub struct Exclusions {
    /// Dossiers exclus (minuscules, separateurs normalises, sans separateur final) avec tout leur contenu.
    folders: Vec<String>,
    /// Dernier composant de chaque dossier exclu (minuscules) : filtre rapide avant de construire un chemin complet.
    folder_names: Vec<String>,
    /// Motifs de noms de fichiers (minuscules) : `*` = suite de caracteres, `?` = un caractere.
    patterns: Vec<Motif>,
    pub hidden: bool,
    pub system: bool,
}

/// Normalise un chemin pour la comparaison : minuscules, `/` remplace par `\` (chemins Windows), sans separateur final.
fn normalise(p: &str) -> String {
    let mut s = p.trim().to_lowercase();
    if cfg!(windows) {
        s = s.replace('/', "\\");
        // forme longue de Windows (\\?\C:\...) : on compare toujours la forme usuelle
        if let Some(reste) = s.strip_prefix(r"\\?\") {
            s = reste.to_string();
        }
    }
    let racine = s.len() == 3 && s.ends_with(":\\"); // « c:\ » garde son separateur
    if !racine {
        while s.ends_with(['\\', '/']) {
            s.pop();
        }
    }
    s
}

/// Correspondance d'un nom avec un motif a jokers (`*`, `?`), sans tenir compte de la casse (deja en minuscules).
pub fn glob_match(motif: &str, texte: &str) -> bool {
    let (m, t): (Vec<char>, Vec<char>) = (motif.chars().collect(), texte.chars().collect());
    let (mut i, mut j) = (0usize, 0usize);
    let (mut etoile, mut reprise) = (None, 0usize);
    while j < t.len() {
        if i < m.len() && (m[i] == '?' || m[i] == t[j]) {
            i += 1;
            j += 1;
        } else if i < m.len() && m[i] == '*' {
            etoile = Some(i);
            i += 1;
            reprise = j;
        } else if let Some(e) = etoile {
            i = e + 1;
            reprise += 1;
            j = reprise;
        } else {
            return false;
        }
    }
    while i < m.len() && m[i] == '*' {
        i += 1;
    }
    i == m.len()
}

impl Exclusions {
    pub fn new(folders: &[String], patterns: &[String], hidden: bool, system: bool) -> Self {
        let folders: Vec<String> = folders
            .iter()
            .map(|f| normalise(f))
            .filter(|f| !f.is_empty())
            .collect();
        let folder_names = folders
            .iter()
            .map(|f| f.rsplit(['\\', '/']).next().unwrap_or(f).to_string())
            .collect();
        Exclusions {
            folders,
            folder_names,
            patterns: patterns
                .iter()
                .flat_map(|p| p.split(';'))
                .map(|p| p.trim().to_lowercase())
                .filter(|p| !p.is_empty())
                .map(Motif::new)
                .collect(),
            hidden,
            system,
        }
    }

    /// Exclusions limitees a des dossiers.
    pub fn from_folders(folders: &[String]) -> Self {
        Self::new(folders, &[], false, false)
    }

    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.patterns.is_empty() && !self.hidden && !self.system
    }

    pub fn has_folders(&self) -> bool {
        !self.folders.is_empty()
    }

    /// Le nom d'un fichier correspond-il a un motif exclu ? (les motifs ne concernent pas les dossiers)
    pub fn name_excluded(&self, name: &str) -> bool {
        if self.patterns.is_empty() {
            return false;
        }
        self.patterns.iter().any(|p| p.correspond(name))
    }

    /// Les attributs (cache, systeme) font-ils exclure l'entree ?
    pub fn attrs_excluded(&self, attrs: u32) -> bool {
        (self.hidden && attrs & ATTR_HIDDEN != 0) || (self.system && attrs & ATTR_SYSTEM != 0)
    }

    /// Un dossier de ce nom pourrait-il etre exclu ? Rapide (aucune allocation) : n'appeler `folder_excluded`,
    /// qui exige le chemin complet, que si c'est vrai.
    pub fn folder_name_candidate(&self, name: &str) -> bool {
        self.folder_names
            .iter()
            .any(|f| f.eq_ignore_ascii_case(name))
            || !name.is_ascii()
    }

    /// Ce chemin est-il, ou est-il dans, un dossier exclu ?
    pub fn folder_excluded(&self, path: &str) -> bool {
        if self.folders.is_empty() {
            return false;
        }
        let p = normalise(path);
        let sep = if cfg!(windows) { '\\' } else { '/' };
        self.folders.iter().any(|f| {
            p == *f
                || (p.starts_with(f.as_str())
                    && (f.ends_with(['\\', '/']) || p[f.len()..].starts_with(sep)))
        })
    }

    /// Un chemin du disque doit-il etre exclu ? Sert a la surveillance en direct (verifie aussi les attributs).
    pub fn path_excluded(&self, path: &Path) -> bool {
        if self.is_empty() {
            return false;
        }
        if self.folder_excluded(&path.to_string_lossy()) {
            return true;
        }
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return false; // supprime : la surveillance doit pouvoir le retirer de l'index
        };
        if !meta.is_dir() {
            if let Some(n) = path.file_name() {
                if self.name_excluded(&n.to_string_lossy()) {
                    return true;
                }
            }
        }
        self.attrs_excluded(attributs(&meta, &nom_fichier(path)))
    }
}

fn nom_fichier(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Attributs cache/systeme d'une entree : Windows les fournit ; ailleurs, un nom commencant par un point est « cache ».
#[cfg(windows)]
pub fn attributs(meta: &std::fs::Metadata, _nom: &str) -> u32 {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes()
}

#[cfg(not(windows))]
pub fn attributs(_meta: &std::fs::Metadata, nom: &str) -> u32 {
    if nom.starts_with('.') {
        ATTR_HIDDEN
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jokers() {
        assert!(glob_match("*.tmp", "a.tmp"));
        assert!(glob_match("*.tmp", ".tmp"));
        assert!(!glob_match("*.tmp", "a.tmpx"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
        assert!(glob_match("~$*", "~$doc.docx"));
    }

    #[test]
    fn dossiers_et_motifs() {
        let e = Exclusions::new(
            &["C:\\Windows\\WinSxS\\".into(), "d:/temp".into()],
            &["*.TMP; *.log".into()],
            false,
            false,
        );
        assert!(e.folder_excluded("C:\\Windows\\WinSxS"));
        assert!(e.folder_excluded("c:\\windows\\winsxs\\amd64\\x.dll"));
        assert!(!e.folder_excluded("C:\\Windows\\WinSxSExtra"));
        assert!(!e.folder_excluded("C:\\Windows"));
        if cfg!(windows) {
            assert!(e.folder_excluded("D:\\Temp\\a.txt"));
        }
        assert!(e.name_excluded("Journal.LOG"));
        assert!(e.name_excluded("a.tmp"));
        assert!(!e.name_excluded("a.txt"));
        assert!(!Exclusions::default().name_excluded("a.tmp"));
    }

    #[test]
    fn attributs_caches_et_systeme() {
        let e = Exclusions::new(&[], &[], true, false);
        assert!(e.attrs_excluded(ATTR_HIDDEN));
        assert!(!e.attrs_excluded(ATTR_SYSTEM));
        let e = Exclusions::new(&[], &[], false, true);
        assert!(e.attrs_excluded(ATTR_SYSTEM | 0x20));
        assert!(!e.attrs_excluded(ATTR_HIDDEN));
        assert!(Exclusions::default().is_empty());
        assert!(!e.is_empty());
    }

    #[test]
    fn motifs_chemin_rapide_et_general() {
        let e = Exclusions::new(
            &[],
            &["*.DLL".into(), "a?c*".into(), "*é.txt".into(), "~$*".into()],
            false,
            false,
        );
        assert!(e.name_excluded("Kernel32.dll"));
        assert!(e.name_excluded("X.Dll"));
        assert!(!e.name_excluded("dll"));
        assert!(!e.name_excluded("a.dllx"));
        assert!(e.name_excluded("ABCdef.bin")); // a?c*, sans tenir compte de la casse
        assert!(!e.name_excluded("ac.bin"));
        assert!(e.name_excluded("café.txt")); // suffixe non ASCII : chemin general
        assert!(e.name_excluded("CAFÉ.TXT"));
        assert!(e.name_excluded("~$rapport.docx"));
        assert!(!e.name_excluded("rapport.docx"));
    }
}
