# Fouine : audit, plan, matrice de compatibilité et état réel

Moteur de recherche de fichiers en **pure Rust**, conçu de façon indépendante (clean-room). Deux crates :
`fouine-core` (index, requêtes, favoris, historique, export ; compile en natif et en WebAssembly) et
`fouine-app` (interface egui/eframe + wgpu, identité visuelle « braise »).

## Méthode et limites juridiques
- Aucun code décompilé n'est repris, renommé ou réécrit. Aucun code, texte, icône ni ressource du logiciel de référence n'est utilisé.
- Sources : documentation publique de Windows (FSCTL_ENUM_USN_DATA), observation du comportement attendu d'un outil de recherche instantanée, conception propre. Un prototype Python (`PROJETS/rapido`) a servi de référence de comportement ; il est écrit par nous.
- Ghidra : voir `GHIDRA.md`.

## Architecture
| Module | Fichier | Rôle |
|---|---|---|
| Index en colonnes | `core/src/index.rs` | noms contigus (original + minuscules), parents, drapeaux, tailles/dates lues à la demande, mises à jour incrémentales, compactage, persistance LZ4 (lecture gzip conservée) |
| Requêtes | `core/src/query.rs` | analyseur (ET, OU/`\|`, NON/`!`, jokers, regex, filtres) et exécution (amorce par sous-chaîne `memchr`) |
| Indexation | `core/src/scan.rs`, `core/src/ntfs.rs` | parcours de dossiers (toutes plateformes) ; table des fichiers NTFS (Windows, administrateur) |
| Historique, favoris, export | `core/src/store.rs`, `export.rs` | JSON atomique ; CSV / JSON / texte |
| Interface | `app/src/*.rs` | table virtuelle, recherche en tâche de fond, surveillance (`notify`), raccourcis |
| Tests, benchmarks | `core/tests/matrix.rs`, `core/benches/search.rs` | matrice de compatibilité ; criterion, graine fixe |

## État réel (rien n'est déclaré fini sans test)
| Fonction | État | Preuve |
|---|---|---|
| Sous-chaîne, ET/OU/NON, jokers, regex, casse, mot entier, chemin, ext, type, taille, dates | fait | `cargo test -p fouine-core` : 17 tests passent (+1 test NTFS réservé à l administrateur) |
| Tri (nom naturel, chemin, taille, date, extension) | fait | `tri_naturel_et_par_taille` |
| Index incrémental (ajout, suppression d'arbre, compactage), persistance | fait | `incremental_*`, `persistance` |
| Surveillance en direct (`core/src/watch.rs`, crate `notify`) | fait | `surveillance_reelle_creation_renommage_suppression` + `scripts/smoke.ps1` sur l application : 25 entrées attendues et obtenues |
| Historique, favoris, export | fait (cœur) ; menus dans l'application | `historique_et_favoris`, `export_formats` ; menus non testés à l'écran |
| Interface egui (refonte ergonomique : barre de recherche, filtres rapides, tableau triable, surlignage, dates relatives, états vides, aide F1, thèmes clair/sombre) | démarre, indexe, suit le disque, sauvegarde, se ferme proprement ; **captures d'écran réelles** des thèmes clair et sombre, de l'état vide, de la recherche et de l'aide (`scripts/capture-ui.ps1`) | contrastes WCAG AA vérifiés par test (`theme::tests`) ; menus déroulants (menu, historique, favoris), menu contextuel, filtre rapide, saisie, sélection au clavier **exercés à l'écran** (`scripts/capture-menus.ps1`) ; tri par clic sur l'en-tête et Entrée/Ctrl+Entrée (ouverture réelle) **non vérifiés à l'écran** |
| Indexation NTFS : **lecture directe de la table des fichiers ($MFT)**, parallèle ; repli sur l'énumération du système (`FSCTL_ENUM_USN_DATA`) | fait | Mesures (`cargo run --release -p fouine-core --example indexation -- ntfs:C`, SSD, 8 cœurs, cache chaud, volume système C: de 2,1 millions d'entrées) : **1,01 s** (lecture directe) contre 5,62 s avant et 4,4 s pour l'énumération du système, soit ×5,6. Justesse (`indexation -- compare:C`, `ntfs_lecture_directe_equivaut_systeme`) : 8 chemins seulement vus par l'ancienne voie sur 2 millions ; 148 522 chemins de plus (liens physiques), 3 032 sur 3 032 vérifiés présents sur le disque ; tailles et dates justes sur 3 428 fichiers. L'énumération du système ne se parallélise pas dans le noyau (1 fil : 4,15 s, 8 fils : 4,8 s). Dans l'application : indexation de C: en 1,0 s |
| **Comparaison à Everything 1.4.1.1032** (index vide, C: + D:, 3,25 millions d'entrées, administrateur, machine seule, 29/10/2026) | mesuré | Everything : 13,81 / 13,72 / 13,67 s jusqu'au total complet affiché (phases relevées dans sa barre d'état : C: 0,13 -> 2,97 s, D: -> 6,47 s, tri des noms -> 13,6 s). Fouine : index complet 3,57-3,59 s, prêt à chercher (1re recherche « dll » triée par nom, 5 000 résultats, 150-180 ms) 3,72-3,76 s sur 5 essais, **soit 3,7 fois plus rapide**. Volumes lus en parallèle (C: seul 1,18 s, D: seul 3,33 s, limité par le SSD SATA). Rechargement d'un index de 3,25 M entrées : 0,64 s (62 Mo). Méthode : `indexation -- "volumes:C:;D:"` et `scripts/mesure-everything.ps1` (instance séparée `-instance fouine-bench`, sa propre base et sa propre configuration ; lecture de la barre d'état par WM_GETTEXT avec délai maximum). **Non mesuré** : recherche en régime établi d'Everything, mémoire, démarrage d'Everything sur index existant, mises à jour en continu, FAT/exFAT/ext |
| **Volumes et exclusions** (fenêtre « Volumes et dossiers indexés ») : volumes détectés, inclusion automatique fixes/amovibles, dossiers exclus, motifs de noms, caches, systeme | fait | `exclusions::tests` (4), `exclusions_parcours_dossiers_motifs_caches`, `ntfs_exclusions` (administrateur : sur le vrai volume, aucun contenu d'un dossier exclu, aucun `*.dll`, au plus 0,5 % d'entrees cachees ou systeme sur un echantillon de plus de 1 000) ; de bout en bout (`fouine --daemon`) : C: + D: 3,25 M d'entrees, 3,6 s ; avec toutes les exclusions 2,40 M d'entrees, 3,5 s ; volume decoche non re-ajoute. **Non fait** : detection d'un volume branche pendant que l'application tourne (les volumes sont releves au lancement et a la reindexation) ; propriete « dates de creation et d'acces indexees » d'Everything |
| Parcours de dossiers parallèle (FAT, FAT32, exFAT, ext3/ext4, tout volume sans table lisible) | fait, **non testé sur ces systèmes de fichiers** | Mesuré sur NTFS en parcours de dossiers : `tools` (200 255 entrées) 2,73 s -> 0,63 s (×4,3) ; `C:\Windows` (227 884 entrées) 4,62 s -> 1,14 s (×4,0). Test `parcours_parallele_gros_arbre`. Pas de volume FAT/exFAT/ext disponible ; sous Linux et macOS, la taille et la date ne sont plus lues à l'indexation (lecture à la demande). Lecture directe de FAT/exFAT : non faite |
| (ancienne ligne) Indexation NTFS rapide | fait | `ntfs_volume_reel_administrateur` (exécuté en administrateur) : **1 963 114 entrées sur C: en 18,7 s** (profil debug), `ntoskrnl.exe` retrouvé, au moins 90 % des chemins reconstruits existent sur le disque ; sans droits : erreur propre (`ntfs_sans_droits_ou_avec`) |
| Compilation WebAssembly du cœur | `cargo check --target wasm32-unknown-unknown -p fouine-core` passe | pas d'interface web |
| Compilation macOS / Linux | `cargo check --workspace --target x86_64-unknown-linux-gnu` et `aarch64-apple-darwin` passent | **compilation seulement** : pas d'édition de liens ni d'exécution sur ces systèmes |
| Mémoire de l'index | mesurée (`cargo run --release -p fouine-core --example memoire`) : **98 octets par entrée** (46,6 Mo pour 500 000 ; 186,5 Mo pour 2 000 000) | allocateur compteur ; hors mémoire de l'interface (environ 180 Mo de processus pour egui/wgpu) |
| Arrière-plan : zone de notification (icône, clic gauche = afficher, menu Ouvrir / Quitter), fermeture = réduction, instance unique (une 2e instance réaffiche la fenêtre), sauvegarde automatique de l'index (30 s, seulement s'il a changé), droits utilisateur ou administrateur (lecture rapide NTFS si administrateur ; « Relancer en administrateur » dans les réglages) | fait, **vérifié par `scripts/verifie-arriere-plan.ps1`** (fenêtre cachée à la fermeture, processus vivant, base mise à jour pendant ce temps, réaffichage par une 2e instance) ; test `demon_surveille_et_enregistre` | clic sur l'icône elle-même et menu de l'icône **non exercés** (seul le signal de la 2e instance l'est) ; relance UAC non testée |
| Service Windows « Fouine » (`fouine.exe --service`, démarrage automatique, indexe et surveille sans session, données dans %ProgramData%/Fouine ; l'interface relit l'index du service) : installer / désinstaller depuis les réglages ou `--install-service` / `--uninstall-service` | écrit et compile ; **non installé ni testé** (modifierait les services de l'hôte) ; le mode sans fenêtre `--daemon N` (même boucle que le service) est testé | à tester par l'utilisateur : installation, démarrage au boot, arrêt, désinstallation |
| API locale, tube nommé, serveur réseau, raccourci global, icône de notification, métadonnées audio/image | **non faits** | |
| `dc:` `da:` (création, accès), `content:`, `parents:`, `infolder:`, `childcount:`, `dupe:` | fait | `dates_creation_et_acces`, `contenu`, `parents_infolder_childcount`, `doublons_par_nom` |
| Interactions à l'écran : tri par clic (Taille, Nom), Entrée, Ctrl+Entrée, double-clic | vérifiées (clics et touches réels, preuve dans le journal `FOUINE_LOG`) | `scripts/verifie-interactions.ps1` ; correctif : toute la cellule d'en-tête est cliquable |
| `type:` (catégories d'extensions), `startwith:` `endwith:` `exact:` `depth:` | fait | `categories_de_fichiers`, `debut_fin_nom_exact_profondeur` |
| Qualité statique | `cargo clippy --workspace --all-targets` sans avertissement ; `cargo fmt` appliqué | |

## Mesures (`cargo bench -p fouine-core --bench search`, 500 000 entrées synthétiques, graine fixe, 1 000 résultats max, sans tri)
| Requête | Médiane |
|---|---|
| `rapport` | 112 µs |
| `facture 2024` (rare) | 5,7 ms |
| `ext:pdf budget` | 1,3 ms |
| `photo !final` | 105 µs |
| `regex:^plan.*\d{3}` | 733 µs |
| `*.mp4` | 184 µs |
| `path:projet client` | 3,9 ms |
| `contrat \| devis` | 165 µs |
| aucun résultat | 699 µs |

À comparer au prototype Python : 1,6 ms à 79 ms sur les mêmes types de requêtes. Machine de développement unique, non représentative.

## Risques
1. L'enumeration NTFS exige l'administrateur ; repli sur parcours (plus lent).
2. Une double indexation a été vue une fois au démarrage (466 puis 467 éléments) ; elle n'a pas été reproduite sur 2 lancements suivants (le journal trace chaque démarrage d'indexation). La surveillance peut perdre des événements en cas de rafale ; un réindexage manuel reste nécessaire.
3. `remove_tree` et `lookup` sont linéaires (parcours de l'index) : acceptable pour des mises à jour ponctuelles, pas pour des rafales de millions d'événements.
4. Les tailles/dates sont lues à la demande : un tri par taille/date sur beaucoup de résultats lit le disque.

## Matrice de compatibilité fonctionnelle
| Fonction | Comportement attendu | Exemple | Résultat attendu | Cas limites / erreurs | Test |
|---|---|---|---|---|---|
| Sous-chaîne | insensible à la casse | `rapport` | tous les noms contenant « rapport » | accents conservés | `sous_chaine_insensible_a_la_casse` |
| ET | espace | `rapport final` | noms contenant les deux | ordre indifférent ; `AND` accepté | `et_ou_non` |
| OU | `\|` ou `OR` | `plage \| notes` | l'un ou l'autre | groupe vide ignoré | `et_ou_non` |
| NON | `!x` ou `NOT x` | `rapport !final` | exclut | | `et_ou_non` |
| Jokers | `*` `?` | `*.jpg`, `img?.png` | nom entier | | `casse_regex_jokers_mot_entier` |
| Regex | `regex:` | `regex:^r.*docx$` | recherche regex sur le nom | regex invalide : erreur claire | `casse_regex_jokers_mot_entier` |
| Casse | `case:` | `case:Rapport` | sensible à la casse | | `casse_regex_jokers_mot_entier` |
| Mot entier | `ww:` | `ww:final` | délimité par non-lettres | `ww:fin` ne trouve rien | `casse_regex_jokers_mot_entier` |
| Chemin | `\` dans le terme, `path:` | `alpha\ rapport`, `path:photos plage` | chemin complet | `/` accepté | `chemin` |
| Phrases | guillemets | `"rapport-final"` | sous-chaîne littérale avec espaces | | `chemin` |
| Extension | `ext:a;b` | `ext:jpg`, `ext:.pdf;docx` | fichiers seulement | | `extension_et_type` |
| Type | `folder:` `file:` | `folder: pho` | dossiers / fichiers | | `extension_et_type` |
| Taille | `size:` + `> >= < <= a..b`, unités | `size:>1mb` | filtre | unité inconnue : erreur ; dossiers exclus | `taille` |
| Date | `dm:` mots-clés, `AAAA[-MM[-JJ]]`, comparaisons, intervalles | `dm:thisyear`, `dm:<2010` | filtre | date invalide : erreur | `dates` |
| Tri naturel | nombres comparés par valeur | `img2` avant `img10` | | zéros de tête | `tri_naturel_et_par_taille` |
| Limite, requête vide | | `e` limite 3 ; `` | 3 ; aucun résultat | | `limite_et_requete_vide` |
| Index incrémental | création, suppression d'arbre | | | descendants retirés | `incremental_ajout_et_suppression`, `incremental_dossier_et_compactage` |
| Persistance | sauvegarde / rechargement | | entrées supprimées restent supprimées | | `persistance` |
| Historique | 100 requêtes uniques, récentes d'abord | | | fichier corrompu : vide | `historique_et_favoris` |
| Favoris | requêtes nommées | | ajout, remplacement, suppression | nom vide : erreur | `historique_et_favoris` |
| Export | CSV (`;`, UTF-8) / JSON / texte | | une ligne par résultat | `;` et `"` dans les noms | `export_formats` |
| NTFS | énumération du volume | | | sans droits : erreur propre | `ntfs_sans_droits_ou_avec` (partiel) |

## Opérateurs ajoutés d'après l'inventaire de comportement (voir `GHIDRA.md`)
| Fonction | Comportement | Exemple | Test |
|---|---|---|---|
| Début / fin / nom exact | `startwith:` `endwith:` `exact:` (= `wfn:` = `wholefilename:`), insensibles à la casse | `startwith:rapport ext:docx` | `debut_fin_nom_exact_profondeur` |
| Profondeur | nombre de dossiers parents ; comparaisons `> >= < <= a..b` | `depth:>=3 file:` | `debut_fin_nom_exact_profondeur` |

Nouveaux opérateurs (tous lus sur le disque à la demande, évalués en dernier sur les candidats déjà filtrés) :
`dc:`/`datecreated:` et `da:`/`dateaccessed:` (mêmes formats que `dm:` ; 0 si le système ne donne pas la date) ; `content:` (octets du début du fichier, au plus 8 Mo ; casse ASCII ignorée sauf option « Aa » ; pas d'index du contenu) ; `parents:` (= `depth:`) ; `infolder:` (tout l'arbre sous le dossier, dossier lui-même exclu) ; `parent:` (enfants directs) ; `childcount:` (entrées directes d'un dossier, dossiers seulement) ; `dupe:` (même nom, casse ignorée, que d'autres entrées ; table construite à la première évaluation de la requête).
Limites : `content:` et `childcount:` sont lents sur de très nombreux candidats (lecture disque) ; `dupe:` compare les noms seulement (pas la taille ni le contenu).

**À faire** : métadonnées audio/image, API locale, raccourci global, icône de notification, serveur réseau.
