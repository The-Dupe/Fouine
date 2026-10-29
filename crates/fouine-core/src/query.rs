//! Langage de requete : termes ET, `|` / `OR` OU, `!` / `NOT` NON, "phrases", jokers, regex et filtres.

use std::cmp::Ordering;

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use regex::{Regex, RegexBuilder};

use crate::index::{Id, Index};
use crate::Error;

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub case: bool,
    pub regex: bool,
    pub path: bool,
    pub whole_word: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    Path,
    Size,
    Date,
    Extension,
    /// Ordre de l'index, sans tri : permet de s'arreter des que la limite est atteinte.
    None,
}

#[derive(Debug, Clone, Copy)]
enum Cmp {
    Range(f64, f64),
}

impl Cmp {
    fn test(&self, v: f64) -> bool {
        let Cmp::Range(lo, hi) = *self;
        v >= lo && v <= hi
    }
}

#[derive(Clone, Copy)]
enum NameMatch {
    Start,
    End,
    Exact,
}

enum Matcher {
    /// Sous-chaine insensible a la casse (le candidat vient deja de l'index).
    Substring,
    Regex(Regex),
    /// Sous-chaine sensible a la casse.
    Exact(String),
}

enum Kind {
    Text {
        matcher: Matcher,
        on_path: bool,
    },
    Ext(Vec<String>),
    IsDir(bool),
    /// Le nom (minuscules) commence par, finit par, ou vaut exactement la valeur.
    Name(NameMatch, String),
    Depth(Cmp),
    Size(Cmp),
    Date(Cmp),
    Created(Cmp),
    Accessed(Cmp),
    /// Octets recherches dans le contenu du fichier (minuscules si `case` est faux).
    Content {
        needle: Vec<u8>,
        case: bool,
    },
    Parent(String),
    /// Sous le dossier (minuscules, sans separateur final), sous-dossiers compris.
    InFolder(String),
    /// Nombre d'entrees directes d'un dossier, lu sur le disque.
    ChildCount(Cmp),
    /// Meme nom (minuscules) que d'autres entrees ; table construite a la premiere evaluation.
    Dupe(std::sync::OnceLock<std::collections::HashSet<String>>),
}

struct Term {
    kind: Kind,
    /// Sous-chaine (minuscules) que tout nom correspondant contient : sert d'amorce a l'index.
    seed: String,
    negate: bool,
}

impl Term {
    /// Termes qui lisent le disque ou construisent des chemins : evalues en dernier.
    fn costly(&self) -> bool {
        matches!(
            self.kind,
            Kind::Depth(_)
                | Kind::Size(_)
                | Kind::Date(_)
                | Kind::Created(_)
                | Kind::Accessed(_)
                | Kind::Content { .. }
                | Kind::Parent(_)
                | Kind::InFolder(_)
                | Kind::ChildCount(_)
                | Kind::Dupe(_)
                | Kind::Text { on_path: true, .. }
        )
    }

    fn eval(&self, ix: &Index, id: Id) -> bool {
        let r = match &self.kind {
            Kind::Text { matcher, on_path } => {
                let owned;
                let hay: &str = if *on_path {
                    owned = ix.path(id);
                    &owned
                } else {
                    ix.name(id)
                };
                match matcher {
                    Matcher::Substring => {
                        if *on_path {
                            hay.to_lowercase().contains(&self.seed)
                        } else {
                            ix.lower_name(id).contains(self.seed.as_str())
                        }
                    }
                    Matcher::Regex(re) => re.is_match(hay),
                    Matcher::Exact(s) => hay.contains(s.as_str()),
                }
            }
            Kind::Ext(exts) => {
                !ix.is_dir(id) && {
                    let low = ix.lower_name(id);
                    exts.iter().any(|e| low.ends_with(e.as_str()))
                }
            }
            Kind::IsDir(want) => ix.is_dir(id) == *want,
            Kind::Name(m, v) => {
                let n = ix.lower_name(id);
                match m {
                    NameMatch::Start => n.starts_with(v.as_str()),
                    NameMatch::End => n.ends_with(v.as_str()),
                    NameMatch::Exact => n == v,
                }
            }
            Kind::Depth(c) => {
                let (mut d, mut p) = (0u32, ix.parent(id));
                while p != crate::index::NO_PARENT && d < 4096 {
                    d += 1;
                    p = ix.parent(p);
                }
                c.test(f64::from(d))
            }
            Kind::Size(c) => !ix.is_dir(id) && c.test(ix.stat(id).0 as f64),
            Kind::Date(c) => c.test(ix.stat(id).1 as f64),
            Kind::Created(c) => c.test(ix.times(id).0 as f64),
            Kind::Accessed(c) => c.test(ix.times(id).1 as f64),
            Kind::Content { needle, case } => {
                !ix.is_dir(id) && file_contains(&ix.path(id), needle, *case)
            }
            Kind::InFolder(want) => {
                let p = ix.path(id).to_lowercase();
                p.len() > want.len()
                    && p.starts_with(want.as_str())
                    && p.as_bytes()[want.len()] == b'\\'
            }
            Kind::ChildCount(c) => {
                ix.is_dir(id)
                    && std::fs::read_dir(ix.path(id)).is_ok_and(|d| c.test(d.count() as f64))
            }
            Kind::Dupe(names) => {
                let set = names.get_or_init(|| {
                    let mut seen = std::collections::HashMap::<String, u32>::new();
                    for i in ix.candidates("") {
                        *seen.entry(ix.name(i).to_lowercase()).or_default() += 1;
                    }
                    seen.into_iter()
                        .filter(|&(_, n)| n > 1)
                        .map(|(k, _)| k)
                        .collect()
                });
                set.contains(&ix.name(id).to_lowercase())
            }
            Kind::Parent(want) => {
                let p = ix.parent(id);
                p != crate::index::NO_PARENT
                    && ix.path(p).trim_end_matches(['\\', '/']).to_lowercase() == *want
            }
        };
        r != self.negate
    }
}

pub struct Query {
    groups: Vec<Vec<Term>>,
}

fn parse_size(text: &str) -> Result<f64, Error> {
    let t = text.trim().to_lowercase();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let mult: f64 = match unit.trim() {
        "" | "b" | "o" => 1.0,
        "k" | "kb" | "ko" => 1024.0,
        "m" | "mb" | "mo" => 1024.0 * 1024.0,
        "g" | "gb" | "go" => 1024f64.powi(3),
        "t" | "tb" | "to" => 1024f64.powi(4),
        _ => return Err(Error::Query(format!("taille invalide : {text}"))),
    };
    num.parse::<f64>()
        .map(|n| n * mult)
        .map_err(|_| Error::Query(format!("taille invalide : {text}")))
}

/// Intervalle [debut, fin[ en secondes Unix d'un mot-cle ou d'une date AAAA[-MM[-JJ]].
fn date_range(text: &str) -> Result<(f64, f64), Error> {
    let bad = || Error::Query(format!("date invalide : {text}"));
    let today = Local::now().date_naive();
    let ts = |d: NaiveDate| {
        Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .map_or(0.0, |t| t.timestamp() as f64)
    };
    let day = Duration::days(1);
    let (a, b) = match text.to_lowercase().as_str() {
        "today" => (today, today + day),
        "yesterday" => (today - day, today),
        "thisweek" => (
            today - Duration::days(today.weekday().num_days_from_monday() as i64),
            today + day,
        ),
        "thismonth" => (today.with_day(1).unwrap(), today + day),
        "thisyear" => (
            NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap(),
            today + day,
        ),
        _ => {
            let parts: Vec<&str> = text.split('-').collect();
            let num = |s: &str| s.parse::<u32>().map_err(|_| bad());
            match parts.as_slice() {
                [y] if y.len() == 4 => {
                    let y = num(y)? as i32;
                    (
                        NaiveDate::from_ymd_opt(y, 1, 1).ok_or_else(bad)?,
                        NaiveDate::from_ymd_opt(y + 1, 1, 1).ok_or_else(bad)?,
                    )
                }
                [y, m] => {
                    let (y, m) = (num(y)? as i32, num(m)?);
                    let a = NaiveDate::from_ymd_opt(y, m, 1).ok_or_else(bad)?;
                    let b = if m == 12 {
                        NaiveDate::from_ymd_opt(y + 1, 1, 1)
                    } else {
                        NaiveDate::from_ymd_opt(y, m + 1, 1)
                    };
                    (a, b.ok_or_else(bad)?)
                }
                [y, m, d] => {
                    let a = NaiveDate::from_ymd_opt(num(y)? as i32, num(m)?, num(d)?)
                        .ok_or_else(bad)?;
                    (a, a + day)
                }
                _ => return Err(bad()),
            }
        }
    };
    Ok((ts(a), ts(b)))
}

/// `>x`, `>=x`, `<x`, `<=x`, `a..b` ou `x`. `dates` : une valeur seule couvre tout son intervalle.
type NumParser<'a> = &'a dyn Fn(&str) -> Result<(f64, f64), Error>;

fn parse_cmp(value: &str, num: NumParser) -> Result<Cmp, Error> {
    if let Some((lo, hi)) = value.split_once("..") {
        let a = if lo.is_empty() {
            f64::NEG_INFINITY
        } else {
            num(lo)?.0
        };
        let b = if hi.is_empty() {
            f64::INFINITY
        } else {
            num(hi)?.0
        };
        return Ok(Cmp::Range(a, b));
    }
    for (op, f) in [
        (
            ">=",
            (|v: (f64, f64)| Cmp::Range(v.0, f64::INFINITY)) as fn((f64, f64)) -> Cmp,
        ),
        ("<=", |v| {
            Cmp::Range(f64::NEG_INFINITY, v.1 - f64::from(v.1 > v.0))
        }),
        (">", |v| {
            Cmp::Range(if v.1 > v.0 { v.1 } else { v.1.next_up() }, f64::INFINITY)
        }),
        ("<", |v| Cmp::Range(f64::NEG_INFINITY, v.0 - 1.0)),
        ("=", |v| Cmp::Range(v.0, v.1)),
    ] {
        if let Some(rest) = value.strip_prefix(op) {
            return Ok(f(num(rest)?));
        }
    }
    let v = num(value)?;
    Ok(Cmp::Range(v.0, v.1))
}

/// Extensions d'une categorie (`type:image`...). Noms francais et anglais acceptes.
pub fn category_extensions(name: &str) -> Option<&'static [&'static str]> {
    Some(match name.to_lowercase().as_str() {
        "image" | "images" | "picture" | "pic" | "photo" | "photos" => &[
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "svg", "heic", "tif", "tiff", "ico", "avif",
        ],
        "doc" | "docs" | "document" | "documents" => &[
            "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "xls", "xlsx", "ods", "csv", "ppt",
            "pptx", "odp",
        ],
        "audio" | "musique" | "music" => {
            &["mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "wma"]
        }
        "video" | "videos" | "film" => &["mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "flv"],
        "archive" | "archives" | "zip" | "compressed" => {
            &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "iso"]
        }
        "code" | "source" => &[
            "rs", "py", "js", "ts", "tsx", "jsx", "c", "h", "cpp", "hpp", "cs", "java", "go",
            "html", "css", "json", "toml", "yaml", "yml", "sh", "ps1", "sql",
        ],
        "exe" | "executable" | "app" | "programme" => {
            &["exe", "msi", "bat", "cmd", "com", "app", "dmg"]
        }
        _ => return None,
    })
}

fn wildcard_to_regex(pat: &str) -> String {
    let mut out = String::from("^");
    for c in pat.chars() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            c => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out.push('$');
    out
}

fn text_term(text: &str, o: Options) -> Result<Term, Error> {
    let build = |p: &str| {
        RegexBuilder::new(p)
            .case_insensitive(!o.case)
            .build()
            .map_err(|e| Error::Query(e.to_string()))
    };
    let (matcher, seed) = if o.regex {
        (Matcher::Regex(build(text)?), String::new())
    } else if text.contains('*') || text.contains('?') {
        let seed = text
            .to_lowercase()
            .split(['*', '?'])
            .max_by_key(|s| s.len())
            .unwrap_or("")
            .to_string();
        (Matcher::Regex(build(&wildcard_to_regex(text))?), seed)
    } else if o.whole_word {
        let p = format!(
            r"(?:^|[^\p{{L}}\p{{N}}_]){}(?:$|[^\p{{L}}\p{{N}}_])",
            regex::escape(text)
        );
        (Matcher::Regex(build(&p)?), text.to_lowercase())
    } else if o.case {
        (Matcher::Exact(text.to_string()), text.to_lowercase())
    } else {
        (Matcher::Substring, text.to_lowercase())
    };
    // Sur le chemin complet, l'amorce porte sur le chemin, pas sur le nom : on ne s'en sert pas.
    let (matcher, seed) = if o.path {
        match matcher {
            Matcher::Substring => (Matcher::Regex(build(&regex::escape(text))?), String::new()),
            m => (m, String::new()),
        }
    } else {
        (matcher, seed)
    };
    Ok(Term {
        kind: Kind::Text {
            matcher,
            on_path: o.path,
        },
        seed,
        negate: false,
    })
}

/// Taille maximale lue par fichier pour `content:` : au-dela, seul le debut est examine.
pub const CONTENT_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Le debut du fichier (jusqu'a `CONTENT_MAX_BYTES`) contient-il `needle` ? Insensible a la casse ASCII
/// si `case` est faux (`needle` est alors deja en minuscules). Fichier illisible : faux.
fn file_contains(path: &str, needle: &[u8], case: bool) -> bool {
    use std::io::Read;
    let Ok(f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = Vec::new();
    if f.take(CONTENT_MAX_BYTES).read_to_end(&mut buf).is_err() {
        return false;
    }
    if !case {
        buf.make_ascii_lowercase();
    }
    memchr::memmem::find(&buf, needle).is_some()
}

fn parse_term(tok: &str, o: Options) -> Result<Term, Error> {
    let (negate, tok) = match tok.strip_prefix('!') {
        Some(rest) if !rest.is_empty() => (true, rest),
        _ => (false, tok),
    };
    let low = tok.to_lowercase();
    let after = |n: usize| &tok[n..];
    let mut t = match low.as_str() {
        "folder:" | "folders:" => Term {
            kind: Kind::IsDir(true),
            seed: String::new(),
            negate: false,
        },
        "file:" | "files:" => Term {
            kind: Kind::IsDir(false),
            seed: String::new(),
            negate: false,
        },
        _ if low.starts_with("ext:") => {
            let exts = after(4)
                .split(';')
                .filter(|e| !e.is_empty())
                .map(|e| format!(".{}", e.trim_start_matches('.').to_lowercase()))
                .collect();
            Term {
                kind: Kind::Ext(exts),
                seed: String::new(),
                negate: false,
            }
        }
        _ if low.starts_with("type:") => {
            let exts = category_extensions(after(5))
                .ok_or_else(|| Error::Query(format!("type inconnu : {}", after(5))))?;
            Term {
                kind: Kind::Ext(exts.iter().map(|e| format!(".{e}")).collect()),
                seed: String::new(),
                negate: false,
            }
        }
        _ if low.starts_with("startwith:")
            || low.starts_with("endwith:")
            || low.starts_with("exact:")
            || low.starts_with("wholefilename:")
            || low.starts_with("wfn:") =>
        {
            let (name, value) = tok.split_once(':').unwrap();
            let kind = match name.to_lowercase().as_str() {
                "startwith" => NameMatch::Start,
                "endwith" => NameMatch::End,
                _ => NameMatch::Exact,
            };
            let v = value.to_lowercase();
            Term {
                kind: Kind::Name(kind, v.clone()),
                seed: v,
                negate: false,
            }
        }
        _ if low.starts_with("depth:") || low.starts_with("parents:") => {
            let n = tok.split_once(':').unwrap().0.len() + 1;
            let c = parse_cmp(after(n), &|s| {
                s.parse::<f64>()
                    .map(|v| (v, v))
                    .map_err(|_| Error::Query(format!("profondeur invalide : {s}")))
            })?;
            Term {
                kind: Kind::Depth(c),
                seed: String::new(),
                negate: false,
            }
        }
        _ if low.starts_with("size:") => {
            let c = parse_cmp(after(5), &|s| parse_size(s).map(|v| (v, v)))?;
            Term {
                kind: Kind::Size(c),
                seed: String::new(),
                negate: false,
            }
        }
        _ if low.starts_with("dm:") || low.starts_with("datemodified:") => {
            let value = tok.split_once(':').unwrap().1;
            let c = parse_cmp(value, &|s| date_range(s))?;
            Term {
                kind: Kind::Date(c),
                seed: String::new(),
                negate: false,
            }
        }
        _ if low.starts_with("content:") => {
            let value = after(8);
            if value.is_empty() {
                return Err(Error::Query("content: demande un texte".into()));
            }
            let needle = if o.case {
                value.to_string()
            } else {
                value.to_lowercase()
            };
            Term {
                kind: Kind::Content {
                    needle: needle.into_bytes(),
                    case: o.case,
                },
                seed: String::new(),
                negate: false,
            }
        }
        _ if low.starts_with("dc:") || low.starts_with("datecreated:") => {
            let value = tok.split_once(':').unwrap().1;
            let c = parse_cmp(value, &|s| date_range(s))?;
            Term {
                kind: Kind::Created(c),
                seed: String::new(),
                negate: false,
            }
        }
        _ if low.starts_with("da:") || low.starts_with("dateaccessed:") => {
            let value = tok.split_once(':').unwrap().1;
            let c = parse_cmp(value, &|s| date_range(s))?;
            Term {
                kind: Kind::Accessed(c),
                seed: String::new(),
                negate: false,
            }
        }
        _ if low.starts_with("path:") => text_term(after(5), Options { path: true, ..o })?,
        _ if low.starts_with("regex:") => text_term(after(6), Options { regex: true, ..o })?,
        _ if low.starts_with("case:") => text_term(after(5), Options { case: true, ..o })?,
        _ if low.starts_with("ww:") => text_term(
            after(3),
            Options {
                whole_word: true,
                ..o
            },
        )?,
        _ if low.starts_with("wholeword:") => text_term(
            after(10),
            Options {
                whole_word: true,
                ..o
            },
        )?,
        _ if low.starts_with("childcount:") => {
            let c = parse_cmp(after(11), &|s| {
                s.parse::<f64>()
                    .map(|v| (v, v))
                    .map_err(|_| Error::Query(format!("nombre invalide : {s}")))
            })?;
            Term {
                kind: Kind::ChildCount(c),
                seed: String::new(),
                negate: false,
            }
        }
        "dupe:" | "namedupe:" => Term {
            kind: Kind::Dupe(Default::default()),
            seed: String::new(),
            negate: false,
        },
        _ if low.starts_with("infolder:") => Term {
            kind: Kind::InFolder(
                after(9)
                    .trim_end_matches(['\\', '/'])
                    .replace('/', "\\")
                    .to_lowercase(),
            ),
            seed: String::new(),
            negate: false,
        },
        _ if low.starts_with("parent:") => Term {
            kind: Kind::Parent(
                after(7)
                    .trim_end_matches(['\\', '/'])
                    .replace('/', "\\")
                    .to_lowercase(),
            ),
            seed: String::new(),
            negate: false,
        },
        _ => {
            let sep = tok.contains('\\') || tok.contains('/');
            text_term(
                &tok.replace('/', "\\"),
                Options {
                    path: o.path || sep,
                    ..o
                },
            )?
        }
    };
    t.negate = negate;
    Ok(t)
}

/// Decoupe en mots en respectant les guillemets (qui peuvent etre au milieu : `path:"a b"`).
fn tokenize(q: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    for c in q.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

impl Query {
    pub fn parse(text: &str, mut o: Options) -> Result<Query, Error> {
        let mut groups: Vec<Vec<Term>> = vec![Vec::new()];
        let mut negate_next = false;
        for tok in tokenize(text) {
            match tok.as_str() {
                "|" | "OR" => groups.push(Vec::new()),
                "AND" => {}
                "NOT" => negate_next = true,
                "case:" | "nocase:" => o.case = tok == "case:",
                "regex:" | "noregex:" => o.regex = tok == "regex:",
                "path:" | "nopath:" => o.path = tok == "path:",
                "wholeword:" | "ww:" | "nowholeword:" | "noww:" => {
                    o.whole_word = !tok.starts_with("no")
                }
                _ => {
                    let mut t = parse_term(&tok, o)?;
                    if negate_next {
                        t.negate = !t.negate;
                        negate_next = false;
                    }
                    groups.last_mut().unwrap().push(t);
                }
            }
        }
        groups.retain(|g| !g.is_empty());
        Ok(Query { groups })
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Identifiants correspondants, tries ; au plus `limit`.
    pub fn run(&self, ix: &Index, limit: usize, sort: Sort) -> Vec<Id> {
        let mut out: Vec<Id> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for conj in &self.groups {
            let seed = conj
                .iter()
                .filter(|t| !t.negate)
                .map(|t| t.seed.as_str())
                .max_by_key(|s| s.len())
                .unwrap_or("");
            let mut order: Vec<&Term> = conj.iter().collect();
            order.sort_by_key(|t| t.costly());
            for id in ix.candidates(seed) {
                if self.groups.len() > 1 && seen.contains(&id) {
                    continue;
                }
                if order.iter().all(|t| t.eval(ix, id)) {
                    if self.groups.len() > 1 {
                        seen.insert(id);
                    }
                    out.push(id);
                    if sort == Sort::None && out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        sort_ids(ix, &mut out, sort);
        out.truncate(limit);
        out
    }
}

/// Comparaison « naturelle » : les nombres se comparent par valeur (`a2` avant `a10`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (mut na, mut nb) = (String::new(), String::new());
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub fn sort_ids(ix: &Index, ids: &mut [Id], sort: Sort) {
    match sort {
        Sort::None => {}
        Sort::Name => ids.sort_by(|&a, &b| {
            ix.is_dir(b)
                .cmp(&ix.is_dir(a))
                .then_with(|| natural_cmp(ix.lower_name(a), ix.lower_name(b)))
        }),
        Sort::Path => ids.sort_by_cached_key(|&i| ix.path(i).to_lowercase()),
        Sort::Size => ids.sort_by_cached_key(|&i| std::cmp::Reverse(ix.stat(i).0)),
        Sort::Date => ids.sort_by_cached_key(|&i| std::cmp::Reverse(ix.stat(i).1)),
        Sort::Extension => ids.sort_by_cached_key(|&i| {
            let n = ix.lower_name(i);
            (
                n.rsplit_once('.')
                    .map(|(_, e)| e.to_string())
                    .unwrap_or_default(),
                n.to_string(),
            )
        }),
    }
}

/// Raccourci : analyse et execute.
pub fn search(
    ix: &Index,
    text: &str,
    o: Options,
    limit: usize,
    sort: Sort,
) -> Result<Vec<Id>, Error> {
    Ok(Query::parse(text, o)?.run(ix, limit, sort))
}
