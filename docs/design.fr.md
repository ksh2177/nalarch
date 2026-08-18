# nalarch — comment ça marche

Le README dit ce que fait nalarch et comment le lancer. Ceci dit pourquoi il
fonctionne ainsi : les écrans en détail, l'analyse qui les alimente, et le
raisonnement derrière les choix que le code ne montre pas.

*[English version](design.md) — version de référence · [retour au README](../README.fr.md).*

## Les trois écrans

**Tableau** → **Résumé** → **Exécution**. Rien ne se lance sans passer par le résumé.

### L'écran de résumé

C'est la raison d'être de nalarch. Un gestionnaire de paquets affiche bien tout ce qu'il
faut savoir, mais mêlé à des centaines de lignes de journal : on finit par ne plus rien lire
et valider en espérant. Ici tout tient sur un écran.


Les **nouveaux paquets** sont l'apport le moins visible et le plus utile : `checkupdates`
ne montre que les paquets déjà installés qui ont une version plus récente, jamais les
dépendances qu'une mise à jour tire au passage. nalarch les obtient avec
`pacman -Sup --print-format`, exécuté sur la base temporaire de `checkupdates` — données
fraîches, aucun privilège, aucune écriture.

Les catégories reprennent celles de nala (`Operation` dans `src/libnala/transaction.rs`),
adaptées à pacman : mises à jour, nouveaux paquets, retours arrière, suppressions, retraits
en cascade, et paquets figés — l'équivalent de son `Held`.

Les **retraits en cascade** sont le pendant, côté suppression, des nouvelles dépendances :
`-Rns` emporte les dépendances devenues inutiles, et c'est souvent le gros de l'opération.
Retirer `asciiquarium` emporte `perl-term-animation` et `perl-curses` — 789 Kio au lieu des
28 Kio du paquet coché. La liste vient de `pacman -Rsp`, un essai à blanc sans privilège.
(`-n` et `-p` sont incompatibles dans pacman ; l'omettre est sans effet sur la liste, `-n`
ne portant que sur les fichiers de configuration.)

Les **points d'attention** sont calculés, pas décoratifs. Sont détectés : compilation depuis
l'AUR (un PKGBUILD est du code non relu exécuté sous ton compte), mise à jour partielle,
mise à jour du noyau et modules DKMS à recompiler, composants système essentiels, retour à
une version antérieure, nouvelles dépendances, mises à jour figées par `IgnorePkg`. Pour une
suppression : paquets encore réclamés par d'autres, et le rappel que les greffons chargés à
l'exécution sont invisibles.

Règle suivie : n'annoncer que ce qui est **vérifié**. Un avertissement approximatif
s'ignore vite, et une liste qu'on ignore ne protège de rien.

### L'écran d'exécution

Structuré en blocs, comme celui de nala : ce qui se télécharge, ce qui s'exécute, puis ce
qu'il reste à savoir. Chaque bloc porte sa propre barre, parce qu'il n'existe pas
d'avancement unique — téléchargement et installation sont deux décomptes distincts, et les
mêler donnerait un chiffre qui ne veut rien dire.


Avant tout cela, paru a parfois sa propre résolution à montrer. Installer un paquet AUR est
le cas où le plan de nalarch est sciemment incomplet : pacman ne sait pas résoudre
`plakar-git`, donc le plan annonce « 1 paquet, tailles inconnues » pendant que paru établit
que `go` doit venir avec pour le compiler. paru l'imprime dans une table et demande de
confirmer — et tant qu'elle n'était pas lue, la transcription affichait `Opérations · 0` et la
confirmation demandait d'approuver ce que rien à l'écran n'avait montré. Elle est désormais
racontée comme le reste, les dépendances de compilation signalées comme telles :

```
┌ paru a résolu · 2 paquet(s) ─────────────────────────────────────────────┐
│ ⚒ extra      go                    2:1.26.6-1  pour la compilation seule │
│ + aur        plakar-git            1.0.3.r384.gd77c14a2-1                │
└──────────────────────────────────────────────────────────────────────────┘
```

Les colonnes de cette table sont alignées et non délimitées : ses lignes se lisent donc par la
forme — le premier champ porte `dépôt/nom`, un `Yes`/`No` final est l'indicateur de
compilation seule, et ce qui reste est une version pour une installation, deux pour une mise
à jour.

Le bloc **À noter** rassemble ce qui demande une action de ta part et que la sortie noie :
fichiers `.pacnew` à fusionner, redémarrage nécessaire, avertissements, erreurs. Chaque
entrée dit quoi faire, pas seulement ce qui s'est passé — un `.pacnew` s'accompagne de la
commande `sudo pacdiff -s` et du rappel que sans elle, la nouvelle configuration ne
s'applique jamais.

La sortie détaillée de paru reste accessible par `j`, y compris une fois l'opération
terminée. Le
redémarrage se déduit du plan — pacman ne le signale pas, et le symptôme arrive bien plus
tard, quand plus rien ne le relie à la mise à jour.

### Locale figée

Le processus tourne avec `LC_ALL=C`. Analyser une sortie traduite serait intenable : chaque
langue change les verbes, et une mise à jour de traduction casserait le parsing en silence.
En anglais le vocabulaire est stable, tiré directement des chaînes de pacman.

Rien de cet anglais n'atteint l'écran : `src/journal.rs` réécrit tout — les phases, les
actions, et les avertissements courants. Un message non reconnu passe tel quel, parce qu'une
phrase anglaise vaut mieux qu'une information perdue.

C'est cette analyse qui permet la transcription. nalarch ne retenait auparavant qu'un
compteur et une phase : de quoi remplir une barre, pas de quoi raconter l'opération.

## Voir ce que change une mise à jour

Touche `c` sur un paquet de l'onglet **Mises à jour**.

Une transition de version ne dit rien de ce qu'elle apporte, et les paquets Arch n'embarquent
presque jamais de changelog — `pacman -Qc` est vide la plupart du temps. L'information existe
ailleurs, à deux endroits complémentaires :

```
 fastfetch   2.66.0-1 → 2.67.1-1
 amont : https://github.com/fastfetch-cli/fastfetch
┌ Changements ────────────────────────────────────────────────────┐
│ Journal de packaging Arch                                       │
│  ▸ 2026-08-14  2.67.1-1: New upstream release                   │
│  ▸ 2026-08-06  2.67.0-1: New upstream release                   │
│    2026-07-10  2.66.0-1: New upstream release                   │
│    ─── version installée ───                                    │
│                                                                 │
│ Notes de version amont · 2.67.1                                 │
│  Bugfixes:                                                      │
│  • Fixed a `Symbol not found` error on macOS 10.15              │
└─────────────────────────────────────────────────────────────────┘
```

Le **journal de packaging** dit *pourquoi* le paquet bouge : nouvelle version amont, ou
simple reconstruction contre une bibliothèque. C'est souvent la réponse la plus utile — une
reconstruction n'apporte aucune fonctionnalité et explique une mise à jour qui paraissait
gratuite. Les entrées marquées `▸` sont celles qu'apporte la mise à jour ; sous le trait,
c'est déjà installé.

Les **notes de version amont** viennent de GitHub quand le projet y est hébergé, ce qui
couvre l'essentiel des paquets d'Arch. Un paquet AUR n'a pas de dépôt de packaging : c'est le
PKGBUILD qui fait foi, et paru propose de le relire avant de compiler.

Les requêtes passent par `curl` dans un fil séparé — l'interface ne se fige pas, et ça évite
d'emporter une pile TLS pour deux requêtes optionnelles. Ce qui n'a pas pu être récupéré est
dit explicitement plutôt que laissé vide.

## Thème

Aucune couleur n'est codée en valeur absolue : nalarch n'utilise que les emplacements ANSI
du terminal et ne peint jamais son propre fond. Il suit donc le thème du terminal, clair
comme sombre, y compris lors d'un basculement à chaud.

Les fonds colorés (pastilles, ligne sélectionnée) passent par l'inversion vidéo : c'est la
seule façon d'obtenir un contraste correct sans connaître le thème actif. Un gris fixe
rendrait le texte discret illisible — gris sur gris — dès qu'on bascule.

## Mode `--demo`

```bash
nalarch --demo
```

Rejoue une session ressemblant à paru : invite de mot de passe, question `[O/n]`, barres
réécrites par retours chariot, compteurs, puis une compilation AUR sans avancement
chiffrable. **Aucune commande pacman n'est appelée, rien n'est installé ni supprimé.**

Ce n'est pas une simulation de l'interface : le script tourne réellement dans le
pseudo-terminal et traverse le même émulateur et la même analyse de flux que paru. Seule la
source du texte change. Sans quoi l'écran d'exécution serait intestable dès que le système
est à jour — c'est-à-dire la plupart du temps.

## Mode `--dump`

```bash
nalarch --dump [écran] [largeur] [hauteur]
```

Rend l'interface dans un tampon mémoire et l'écrit en texte brut, sans TTY. Utile pour
vérifier la mise en page, produire une capture, ou déboguer depuis un script.

| écran | contenu |
|---|---|
| `0`–`4` | les cinq onglets du tableau |
| `4` | écran de plan, comme si tout était coché |
| `5` | écran d'exécution (lance `pacman -Qi`, lecture seule) |
| `6` | tableau avec tout coché |
| `7` | exécution en cours, progression chiffrable |
| `8` | exécution en cours, compilation AUR (non chiffrable) |
| `9` | exécution terminée en échec |
| `17` | historique, sur la transaction la plus fournie (4ᵉ nombre = index) |
| `20` | l'onglet de recherche (`--query <terme>`, 4ᵉ nombre = quel résultat) |
| `21` | le plan d'installation de ce résultat, dépendances comprises |
| `22` | une transcription longue (4ᵉ nombre = premier événement, 5ᵉ = sortie brute) |
| `23` | la table de résolution de paru avec sa demande de confirmation |
| `18` | plan de retour arrière construit à partir de celle-ci |
| `19` | sortie brute de paru en fin d'exécution (4ᵉ nombre = lignes remontées) |
