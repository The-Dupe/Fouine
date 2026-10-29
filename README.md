# Fouine

> Le dupe d'**Everything**, en version boostée aux hormones.

**Fouine** est une application de recherche de fichiers instantanée, écrite en Rust. Elle reprend le concept d'[Everything](https://www.voidtools.com/) (trouver n'importe quel fichier en quelques millisecondes) avec un projet **indépendant** : aucun code, texte, icône ni ressource d'Everything n'est utilisé.

> **Statut : en développement.** Testé sur Windows. macOS et Linux compilent mais n'ont pas été exécutés. Voir [`docs/PLAN.md`](docs/PLAN.md) pour l'état réel de chaque fonction.

## Pourquoi Fouine ?

- **Rapide à indexer** : sur un même PC, Fouine indexe C: et D: (3,25 millions d'entrées) et est prêt à chercher en **3,7 s**, contre **13,7 s** pour Everything 1.4.1.1032 dans les mêmes conditions (voir [Performances](#performances)). Recherches en temps réel pendant la frappe (0,1 à 6 ms sur 500 000 entrées synthétiques, `cargo bench`).
- **Multiplateforme visé** : Windows, macOS et Linux (le cœur compile aussi en WebAssembly). Seul Windows est testé à l'exécution.
- **Boostée aux hormones** : langage de requêtes riche (regex, dates de création/accès, contenu des fichiers, doublons…), surveillance en direct, arrière-plan et service Windows.

## Fonctionnalités

- Recherche instantanée sur les dossiers choisis. **Indexation NTFS en administrateur : lecture directe de la table des fichiers, 2,1 millions d'entrées en 1,0 s** (SSD, 8 cœurs ; l'énumération du système en demandait 5,6), avec taille et date incluses et tous les liens physiques. **Autres volumes (FAT, FAT32, exFAT, ext3/ext4) : parcours parallèle des dossiers**, environ 4 fois plus rapide que le parcours séquentiel (200 000 entrées en 0,6 s). Voir `docs/PLAN.md` pour la méthode de mesure et les limites
- **Volumes et dossiers au choix** (comme Everything) : fenêtre « Volumes et dossiers indexés » listant les volumes détectés (système de fichiers, taille, type, mode de lecture) avec une case à cocher chacun, inclusion automatique des nouveaux volumes fixes ou amovibles, dossiers ajoutés à la main
- **Exclusions** : fichiers et dossiers cachés, fichiers et dossiers système, dossiers (avec leur contenu), motifs de noms (`*.tmp`, `~$*`). Elles s'appliquent aussi à la lecture directe NTFS, sans ralentir l'indexation (mesuré : 3,5 s pour C: + D: avec toutes les exclusions actives), et à la surveillance en direct
- Index mis à jour en continu (fichiers ajoutés, modifiés, supprimés), enregistré toutes les 30 s
- Filtres avancés : nom, extension, type, taille, dates de modification, de création et d'accès, profondeur, contenu
- Sélection multiple et clic droit sur un résultat, n'importe où sur la ligne (Windows) : ouvrir, ouvrir le dossier, afficher dans le dossier, **couper, copier** (vrais fichiers, collables dans l'Explorateur), **coller** dans un dossier, copier le chemin ou le nom, supprimer (corbeille)
- Interface légère (egui + wgpu), thèmes clair/sombre/système, contrastes WCAG AA vérifiés par test
- Zone de notification, instance unique, service Windows (option, non testé)

## Technologie

Fouine est écrite entièrement en **Rust** (édition 2021), sans JavaScript ni moteur web : un seul exécutable natif, sans runtime à installer (sous Windows, le runtime C est lié statiquement : aucune DLL Visual C++ n'est requise).

**Organisation** : un espace de travail Cargo de deux crates.

| Crate | Rôle |
|---|---|
| `fouine-core` | Le moteur, **sans aucune interface** (il compile aussi en WebAssembly) : index, langage de requêtes, parcours des dossiers, lecture NTFS, surveillance, démon d'arrière-plan, historique, favoris, export. |
| `fouine-app` | L'application : interface graphique, zone de notification, service Windows, intégration au bureau. |

**Le moteur**
- **Index en colonnes** : noms contigus (original et minuscules), parents, drapeaux ; 98 octets par entrée. Tailles et dates sont lues sur le disque à la demande, pas stockées.
- **Recherche** : amorce par sous-chaîne avec [`memchr`](https://crates.io/crates/memchr), puis évaluation des autres termes sur les seuls candidats ; expressions régulières avec [`regex`](https://crates.io/crates/regex). Les termes coûteux (taille, dates, contenu) passent en dernier.
- **Lecture rapide de NTFS** (Windows, administrateur) : énumération de la table des fichiers du volume avec l'API Windows publique `FSCTL_ENUM_USN_DATA`, via [`windows-sys`](https://crates.io/crates/windows-sys).
- **Surveillance en direct** : [`notify`](https://crates.io/crates/notify) (ReadDirectoryChangesW sous Windows, FSEvents sous macOS, inotify sous Linux) ; l'index est mis à jour sans reconstruction.
- **Persistance** : [`bincode`](https://crates.io/crates/bincode) + compression LZ4 ([`lz4_flex`](https://crates.io/crates/lz4_flex)), écriture atomique (fichier temporaire puis renommage) ; les anciens fichiers gzip ([`flate2`](https://crates.io/crates/flate2)) restent lisibles. Historique, favoris et réglages en JSON ([`serde`](https://serde.rs/) / `serde_json`).
- Dates avec [`chrono`](https://crates.io/crates/chrono), erreurs avec [`thiserror`](https://crates.io/crates/thiserror).

**L'application**
- Interface avec [`egui`](https://github.com/emilk/egui) / `eframe` et le rendu GPU [`wgpu`](https://wgpu.rs/) (DirectX 12, Vulkan, Metal, OpenGL selon la machine) ; tableau virtualisé avec `egui_extras`. Thème « braise » maison, contrastes WCAG AA vérifiés par un test.
- Recherches dans des fils de travail : l'interface ne se fige pas pendant la frappe.
- Zone de notification avec [`tray-icon`](https://crates.io/crates/tray-icon), service Windows avec [`windows-service`](https://crates.io/crates/windows-service), icône et informations de version intégrées à l'exécutable avec [`winresource`](https://crates.io/crates/winresource).

**Qualité** : tests automatisés du moteur (matrice de compatibilité de requêtes, persistance, surveillance réelle du disque, démon), benchmarks reproductibles avec [`criterion`](https://crates.io/crates/criterion), `clippy` sans avertissement, vérification de compilation pour WebAssembly, Linux et macOS.

## Performances

Mesures du 29 octobre 2026, **sur un seul PC** : Windows 11, 8 cœurs, C: SSD NVMe de 922 Go (2,14 millions d'entrées), D: SSD SATA de 466 Go (1,11 million d'entrées), soit 3,25 millions de fichiers et dossiers. Session administrateur, machine sans autre charge, **index vide au départ**. Everything 1.4.1.1032 (instance neuve, réglages par défaut) contre Fouine 0.0.1.

| Index complet jusqu'à « prêt à chercher » | Everything | Fouine | Écart |
|---|---|---|---|
| **C: + D:** | **13,7 s** (13,67 / 13,72 / 13,81) | **3,7 s** (3,72 à 3,75 sur 5 essais, première recherche triée par nom comprise) | **3,7 fois plus rapide** (−73 % de temps) |
| C: seul (phase de balayage) | environ 2,8 s | 1,2 s | environ 2,4 fois |
| D: seul (phase de balayage) | environ 3,5 s | 3,3 s | quasi égal : limité par le débit du SSD SATA |

Au redémarrage, Fouine recharge un index de 3,25 millions d'entrées (62 Mo) en **0,64 s**.

**Pourquoi Fouine est plus rapide à l'indexation**
- Il lit la table des fichiers NTFS ($MFT) directement, par plages de 8 Mo lues en parallèle, au lieu de l'énumération du système (`FSCTL_ENUM_USN_DATA`) que le noyau exécute sur un seul fil (mesure : 4,3 s pour C: contre 1,0 s).
- Les volumes sont lus en parallèle, un disque chacun. Everything balaie C: puis D: l'un après l'autre (visible dans sa barre d'état : C: de 0,1 à 3,0 s, puis D: de 3,0 à 6,5 s).
- Pas de tri préalable : Everything passe environ 7 s à trier les noms avant d'être prêt (de 6,5 s à 13,6 s). Fouine trie à la demande ; la première recherche « dll » triée par nom sur 3,25 millions d'entrées (5 000 résultats) prend environ 150 ms.
- Les tailles et dates sont lues dans la table au passage, et chaque lien physique d'un fichier est indexé.

**Ce que ces chiffres ne disent pas.** Un seul PC et un seul scénario (première indexation, index vide). Je n'ai pas mesuré : la vitesse de recherche en régime établi d'Everything, l'occupation mémoire, le démarrage d'Everything sur un index existant, les mises à jour en continu, ni les systèmes FAT, exFAT et ext. Everything est un logiciel mûr avec beaucoup de fonctions que Fouine n'a pas (par exemple ses serveurs HTTP et ETP). Fouine est une version 0.0.1 en cours de développement.

**Reproduire** (administrateur requis) : `cargo run --release -p fouine-core --example indexation -- 'volumes:C:\;D:\' 5` (sous PowerShell) pour Fouine, `scripts/mesure-everything.ps1 -Fois 3` pour Everything (lance une instance séparée, sans toucher à la vôtre). Détails et méthode dans [`docs/PLAN.md`](docs/PLAN.md).

## Installation

**Version portable (Windows), sans installation** : téléchargez le zip de la [dernière release](https://github.com/The-Dupe/Fouine/releases) (`Fouine-<version>-portable-win64.zip` pour Windows 64 bits, `-portable-win32.zip` pour 32 bits), décompressez-le où vous voulez (dossier, clé USB…) et lancez `fouine.exe`. Il ne contient que cet exécutable : rien n'est écrit dans le registre ni dans Program Files. Les données (`index.db`, `reglages.json`, `favoris.json`) sont créées à côté de `fouine.exe` ; supprimez le dossier pour tout retirer. *(Aucune release publiée pour l'instant.)*

**Depuis les sources** (Rust stable requis) :

```
cargo run --release -p fouine-app         # l'application (indexe le dossier utilisateur au premier lancement)
cargo test -p fouine-core                 # matrice de compatibilité
cargo bench -p fouine-core --bench search # benchmarks (FOUINE_BENCH_N pour changer la taille)
cargo check -p fouine-core --target wasm32-unknown-unknown
```

Pour produire l'exécutable dans `dist/`, lancez le script de votre système (Rust requis) :

| Script | Résultat |
|---|---|
| `compil-win64.cmd` | `dist\fouine-win64.exe` (Windows 64 bits) |
| `compil-win32.cmd` | `dist\fouine-win32.exe` (Windows 32 bits) |
| `compil-mac.sh` | `dist/fouine-mac` (macOS, Apple Silicon et Intel) |
| `compil-linux.sh` | `dist/fouine-linux` (Linux) |

Sous Windows, il faut aussi les « Outils de génération Visual Studio » (charge de travail C++). Windows 10 et 11 sont visés ; les versions antérieures ne sont pas supportées par la chaîne Rust actuelle.

Données : à côté de `fouine.exe` (`index.db`, `reglages.json`, `favoris.json`, `exports\`), ce qui rend l'application portable. Si ce dossier n'est pas modifiable (ex. Program Files), les données vont dans `%APPDATA%\Fouine`. `FOUINE_HOME` impose un autre dossier.
`FOUINE_LOG=<fichier>` active un journal de diagnostic minimal.

## Requêtes

`rapport final` (ET), `a | b` ou `a OR b`, `!x` ou `NOT x`, `"phrase"`, `*.jpg`, `img?.png`, `regex:...`, `case:Aa`,
`ww:mot`, `path:dossier`, `ext:pdf;docx`, `folder:` / `file:`, `size:>1mb`, `size:1kb..10kb`,
`dm:today`, `dm:2024-05`, `dm:<2010`, `dm:2001..2003`, `dc:today` / `da:<2010` (création / accès), `type:image`,
`startwith:x`, `endwith:x`, `exact:nom.ext`, `depth:>=3`, `parents:2`, `infolder:chemin` (sous-dossiers compris),
`parent:chemin` (enfants directs), `childcount:>5`, `dupe:` (même nom), `content:texte` (début du fichier, 8 Mo max, lu à la demande).

## Arrière-plan

- Fermer la fenêtre la réduit dans la zone de notification (réglage désactivable) ; l'index reste surveillé. Relancer `fouine.exe` réaffiche la fenêtre ; menu de l'icône : Ouvrir / Quitter. `fouine.exe --tray` démarre directement réduit.
- Utilisateur ou administrateur : sans droits, parcours classique des dossiers ; en administrateur, lecture rapide des lecteurs NTFS (réglages : « Relancer en administrateur »).
- Service Windows (réglages, ou `fouine.exe --install-service` / `--uninstall-service` en administrateur) : indexation et surveillance sans session ouverte, démarrage automatique ; données dans `%ProgramData%\Fouine`. **Écrit mais pas encore testé sur une vraie installation.** `fouine.exe --daemon 60` exécute la même boucle sans fenêtre pendant 60 s.

## Interface et raccourcis

Filtres rapides (Tout, Dossiers, Images, Documents, Audio, Vidéos, Archives, Code), options (casse, regex, chemin, mot entier), tri par clic sur l'en-tête, correspondances surlignées, clic droit (ouvrir, couper, copier, coller, corbeille…), favoris, historique, export, aide F1.
Ctrl+L recherche · Échap efface · ↑/↓ navigation (Maj+↑/↓ étend la sélection) · Ctrl+clic / Maj+clic sélection multiple · Ctrl+A tout sélectionner · Entrée ouvre · Ctrl+Entrée affiche dans le dossier · **Ctrl+C / Ctrl+X** copient / coupent les fichiers · **Ctrl+V** colle dans le dossier sélectionné · **Suppr** corbeille · Ctrl+Maj+C copie le chemin · Ctrl+Q quitte. Les actions de fichiers s'appliquent à toute la sélection (Windows).

## Windows SmartScreen

Les versions publiées devraient être signées (voir [`docs/SIGNATURE.md`](docs/SIGNATURE.md)). Tant que ce n'est pas fait, Windows peut afficher « application non reconnue » : « Informations complémentaires » puis « Exécuter quand même ». Les sommes SHA-256 sont indiquées dans le texte de chaque release.

### Code signing policy
Les exécutables publiés dans les releases sont construits par GitHub Actions à partir de ce dépôt (`.github/workflows/release.yml`) puis signés par SignPath Foundation. Seuls les binaires issus de cette chaîne sont signés. Le propriétaire du dépôt approuve chaque signature. Fouine ne transmet aucune donnée : l'index reste sur la machine de l'utilisateur. *(À valider lors de la candidature SignPath.)*

## Contribuer

Les issues et les pull requests sont les bienvenues.

## Licence

Fouine est distribué sous double licence, [MIT](LICENSE-MIT) ou [Apache-2.0](LICENSE-APACHE), à votre choix. Copyright (c) 2026 Fouine contributors. Voir [NOTICE](NOTICE).

---

Un projet de l'organisation [The-Dupe](https://github.com/The-Dupe).
