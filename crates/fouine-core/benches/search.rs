//! Benchmark reproductible : index synthetique (graine fixe). `cargo bench -p fouine-core`.

use criterion::{criterion_group, criterion_main, Criterion};
use fouine_core::index::NO_PARENT;
use fouine_core::{search, Index, Options, Sort};

const WORDS: [&str; 24] = [
    "rapport",
    "facture",
    "photo",
    "projet",
    "notes",
    "budget",
    "client",
    "contrat",
    "image",
    "video",
    "musique",
    "cours",
    "devis",
    "plan",
    "schema",
    "archive",
    "sauvegarde",
    "modele",
    "export",
    "import",
    "config",
    "journal",
    "test",
    "final",
];
const EXTS: [&str; 12] = [
    ".txt", ".pdf", ".docx", ".jpg", ".png", ".mp4", ".rs", ".js", ".zip", ".xlsx", ".csv", ".md",
];

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

pub fn build(n: usize) -> Index {
    let mut r = Lcg(42);
    let mut ix = Index::new();
    let root = ix.add("C:\\", NO_PARENT, true, Some(0), None);
    let mut dirs = vec![root];
    for _ in 0..n / 20 {
        let name = format!("{}{}", WORDS[r.below(24)], r.below(1000));
        let parent = dirs[r.below(dirs.len())];
        dirs.push(ix.add(&name, parent, true, Some(0), Some(1_700_000_000)));
    }
    while ix.len() < n {
        let name = format!(
            "{}-{}-{}{}",
            WORDS[r.below(24)],
            WORDS[r.below(24)],
            r.below(10_000),
            EXTS[r.below(12)]
        );
        let parent = dirs[r.below(dirs.len())];
        ix.add(
            &name,
            parent,
            false,
            Some(r.next() % (1 << 24)),
            Some(1_700_000_000 + (r.next() % (1 << 26)) as i64),
        );
    }
    ix
}

fn bench(c: &mut Criterion) {
    let n: usize = std::env::var("FOUINE_BENCH_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500_000);
    let ix = build(n);
    for (name, q) in [
        ("sous-chaine", "rapport"),
        ("rare", "facture 2024"),
        ("ext+terme", "ext:pdf budget"),
        ("negation", "photo !final"),
        ("regex", "regex:^plan.*\\d{3}"),
        ("joker", "*.mp4"),
        ("chemin", "path:projet client"),
        ("ou", "contrat | devis"),
        ("aucun resultat", "zzzzzzz"),
    ] {
        c.bench_function(&format!("{name} ({n} entrees)"), |b| {
            b.iter(|| search(&ix, q, Options::default(), 1000, Sort::None).unwrap())
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
