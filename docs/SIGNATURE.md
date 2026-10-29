# Signature des versions publiées

But : que les utilisateurs lancent `fouine.exe` par double-clic sans l'écran SmartScreen « application non reconnue ».
Un certificat local ne sert qu'au PC de l'auteur ; pour le public, il faut un certificat d'éditeur reconnu.

La licence du projet (`MIT OR Apache-2.0`) est une licence open source reconnue par l'OSI : le programme gratuit **SignPath Foundation** pour les projets open source est donc une piste possible (conditions à vérifier sur signpath.org avant de candidater : elles peuvent avoir changé). Une alternative payante est Azure Trusted Signing (environ 10 €/mois, vérification d'identité) ou un certificat de signature de code acheté auprès d'une autorité reconnue. **Rien n'est soumis ni testé.**

## Ce qui est prêt dans le dépôt
- `.github/workflows/release.yml` : workflow **manuel** (onglet Actions, « Run workflow », avec le numéro de version) qui compile Fouine pour Windows 64 et 32 bits et publie deux zips (`fouine.exe` seul) dans une release. **Il ne signe pas encore** : une étape de signature est à ajouter entre « Compilation » et « Emballage ». Il n'a jamais été exécuté.
- La publication habituelle se fait depuis le PC de l'auteur (script local, non versionné).

## À faire par le propriétaire du dépôt pour signer avec SignPath (dans cet ordre)
1. Vérifier les conditions actuelles du programme « SignPath Foundation » : dépôt public, licence OSI, projet actif, pas de logiciel malveillant, build reproductible depuis les sources en CI.
2. Compléter la section « Code signing policy » du `README` : qui approuve les signatures, que seuls les binaires construits par la CI depuis ce dépôt sont signés, déclaration de confidentialité (Fouine ne transmet aucune donnée).
3. Déposer la candidature, puis créer dans SignPath un projet (slug `fouine`), une politique de signature (`release-signing`) et une configuration d'artefact (`initial`) pour `fouine.exe`.
4. Dans les réglages GitHub du dépôt : secret `SIGNPATH_API_TOKEN` et variable `SIGNPATH_ORGANIZATION_ID`. Ne jamais les écrire dans le dépôt.
5. Ajouter l'étape de signature au workflow (action `signpath/github-action-submit-signing-request`), puis le lancer.

## Limites à connaître
- Un certificat reconnu n'efface pas tout de suite l'avertissement : SmartScreen tient compte de la réputation de l'éditeur, qui se construit avec les téléchargements. Les premières versions peuvent encore afficher l'écran.
- Entre-temps, le `README` explique « Informations complémentaires » puis « Exécuter quand même ».
- Autre piste complémentaire : soumettre `fouine.exe` à l'analyse de Microsoft (Security Intelligence, « Submit a file ») pour qu'il ne soit pas signalé à tort.
