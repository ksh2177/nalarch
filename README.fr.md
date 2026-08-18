# nalarch

Gestionnaire de paquets **TUI** pour Arch Linux, dans l'esprit de [nala](https://github.com/volitank/nala)
côté Debian : voir ce qui va changer, se déplacer dedans, décider, puis appliquer.

*[English version](README.md) — c'est la version de référence ; celle-ci peut avoir un temps de retard.*

![L'onglet Mises à jour : la liste des paquets à gauche, le détail du paquet sélectionné à droite, les mises à jour figées et la légende des touches en bas.](docs/screenshots/updates.png)

## Principe

**libalpm en lecture, paru en écriture.**

Aucune logique de résolution de dépendances ni de compilation AUR n'est réimplémentée.
`nalarch` lit la base de données via `libalpm` (la même bibliothèque que pacman et paru),
affiche l'état, puis **délègue chaque action à `paru`**, qui reste seul maître de la
transaction. C'est ce qui rend l'outil court et sûr : il ne peut pas diverger du
comportement de paru, puisque c'est paru qui agit.

paru ne tourne pas *à côté* de nalarch : il tourne **dedans**, dans un pseudo-terminal.
On ne quitte jamais l'interface.

```
             ┌─ libalpm ────────────► état, plan, tailles, dépendances inverses
   nalarch ──┤
             └─ PTY ──► paru ──► vt100 ──► panneau encadré, étape, invites
```

Le pseudo-terminal est ce qui rend l'intégration possible : paru et pacman testent si leur
sortie est un terminal. Sur un simple tube ils suppriment couleurs et barres de progression,
et `sudo` refuse de lire un mot de passe. Avec un PTY, paru se comporte exactement comme
dans un vrai terminal, et nalarch récupère sa sortie complète. Cette sortie n'étant pas du
texte linéaire — les barres de progression se réécrivent par retours chariot, pacman déplace
le curseur pour afficher plusieurs téléchargements simultanés — elle passe par un émulateur
de terminal (`vt100`) qui reconstitue l'écran réel.

Les touches sont transmises telles quelles à paru pendant l'exécution, Ctrl-C compris : on
répond à ses questions et on saisit le mot de passe sudo sans sortir de nalarch.

Pour la détection des mises à jour, `nalarch` s'appuie sur `checkupdates` plutôt que sur
`pacman -Sy`. `checkupdates` synchronise dans une base temporaire : aucun risque de laisser
le système en *partial upgrade*.

## Langue

L'interface parle anglais par défaut, et français quand l'environnement le demande.
`LC_ALL`, `LC_MESSAGES` et `LANG` sont consultés dans cet ordre ; `--lang en` / `--lang fr`
force le choix.

L'anglais est la langue source : chaque message apparaît en anglais sur son site d'appel dans
le code, et `src/i18n.rs` porte la table française, indexée par le texte anglais. Une
traduction manquante retombe sur l'anglais plutôt que sur un nom de clé — et un test balaie
les sources à la recherche des appels `t()` / `tf()` et fait échouer la compilation quand
l'un d'eux n'a pas d'entrée. Une interface française ne peut donc pas se remplir de phrases
anglaises en silence.

Rien à voir avec la locale imposée à paru : voir [Locale figée](#locale-figée).

## Fonctionnalités

| Onglet | Contenu | Action (`u`) |
|---|---|---|
| **Mises à jour** | dépôts + AUR, ancienne → nouvelle version, taille de téléchargement | `paru -Syu` |
| **Installés** | paquets explicites dont rien ne dépend (≈ `pacman -Qett`) | `paru -Rns` |
| **Orphelins** | tirés comme dépendance, plus réclamés par rien (= `pacman -Qdt`) | `paru -Rns` |
| **Historique** | toutes les transactions passées, et ce qu'un retour arrière rétablirait | `pacman -U` depuis le cache |
| **Cache** | volume, anciennes versions, paquets désinstallés | `paccache -rk<N>` / `U` → `-ruk0` |

La rétention `<N>` n'est pas codée en dur : elle est lue dans `PACCACHE_ARGS`
(`/etc/conf.d/pacman-contrib`), la même que celle appliquée par `paccache.timer`. Sans quoi
le cache oscillerait entre deux politiques à chaque passage.

Le panneau de détails affiche systématiquement **qui dépend du paquet sélectionné**
(`Requis par` / `Optionnel pour`) — l'information à regarder avant toute suppression.

## Les trois écrans

**Tableau** → **Résumé** → **Exécution**. Rien ne se lance sans passer par le résumé.

### L'écran de résumé

C'est la raison d'être de nalarch. Un gestionnaire de paquets affiche bien tout ce qu'il
faut savoir, mais mêlé à des centaines de lignes de journal : on finit par ne plus rien lire
et valider en espérant. Ici tout tient sur un écran.

![L'écran de plan : un bloc Résumé qui compte les opérations par nature et leur coût, à côté d'un bloc Points d'attention qui explique ce qui mérite un regard.](docs/screenshots/plan.png)

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

![L'écran d'exécution : le bloc Opérations qui liste chaque vérification, mise à jour et crochet au fil de l'opération, une barre de progression avec le temps écoulé, et le bloc À noter en dessous.](docs/screenshots/run.png)

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

## Historique et retour arrière

L'onglet **Historique** ne lit pas un journal que nalarch aurait tenu : il lit
`/var/log/pacman.log`. C'est délibéré. nala tient son propre historique, qui ne voit donc
que ce que nala a fait ; sur Arch, toute opération sur les paquets passe par libalpm et
atterrit dans ce fichier — `pacman`, `paru`, une dépendance tirée par un script. Le lire
donne un historique **complet et rétroactif**, y compris ce qui a été fait avant que nalarch
n'existe, sans rien avoir à enregistrer.

Chaque transaction affiche quand elle a eu lieu, ce qui l'a déclenchée, sa durée, ses
opérations une par une, et les avertissements que pacman a émis à ce moment-là (`.pacnew`
compris). Le déclencheur est **décrit**, pas recopié : `pacman --sync -y -u --` devient
« mise à jour du système », et `pacman -U /var/cache/…/fastfetch-2.66.0-1-x86_64.pkg.tar.zst`
devient « installation depuis fichier · fastfetch ». La commande brute reste dans le détail.

Le filtre `/` porte sur les noms de paquets : il répond à « quand est-ce que *ce* paquet a
changé, et vers quoi ? ».

![L'onglet Historique : les transactions passées à gauche avec leur date et ce qu'elles ont changé, et à droite le détail de la transaction sélectionnée avec le verdict de retour arrière.](docs/screenshots/history.png)

### Ce qu'un retour arrière fait, et ce qu'il ne fait pas

`u` construit la **transaction inverse** : ce qui a été installé est retiré, ce qui a été mis
à jour redescend, ce qui a été supprimé revient. Les paquets viennent des caches locaux —
`/var/cache/pacman/pkg` pour les dépôts, `~/.cache/paru/clone` pour l'AUR compilé, qui n'y
passe jamais. Rien n'est téléchargé, aucun dépôt n'est interrogé.

Les caches sont **indexés une fois** au démarrage : la question « cette version est-elle
encore là ? » se pose pour chaque paquet de chaque transaction, et parcourir six mille
fichiers à chaque image serait absurde.

Le verdict est affiché **avant** la liste des opérations, pas après : sur une mise à jour de
cinq cents paquets, savoir si le retour est possible compte plus que de faire défiler la
liste jusqu'en bas pour l'apprendre. Chaque paquet dont la version a été élaguée par
`paccache` est marqué `hors cache` sur sa propre ligne.

Trois choses qu'un retour de paquets ne fait pas, et que l'écran de plan énonce :

- **Il ne revient pas sur l'état du système.** Les fichiers posés par les paquets
  redescendent ; ce qu'un scriptlet ou un hook a écrit depuis — migration de base, config
  réécrite — reste tel quel. Pour un vrai retour d'état, il faut un instantané (snapper/Btrfs).
- **Il ne tient pas.** La prochaine mise à jour complète remontera les versions restaurées,
  sauf `IgnorePkg`.
- **Il peut fabriquer un état inédit.** Quand moins de la moitié d'une grosse transaction est
  récupérable, le système se retrouve avec un mélange des deux versions, jamais livré ni testé
  ainsi. C'est signalé comme risque sérieux, en tête de liste.

## La liste de protection

`~/.config/nalarch/keep.list`

libalpm ne connaît que les dépendances **déclarées**. Un paquet chargé dynamiquement —
plugin Qt, backend Wayland, greffon GStreamer — apparaît donc comme orphelin alors qu'il
est vital. Sur une machine Hyprland, `qt6-wayland` est le cas d'école : `pacman -Qdtq` le
liste, et un `pacman -Rns $(pacman -Qdtq)` en aveugle casse le shell graphique.

Les paquets de cette liste s'affichent en jaune avec la marque `[·]` et **ne peuvent pas
être cochés pour suppression**. La touche `p` ajoute ou retire la protection du paquet
sélectionné. Le fichier est créé au premier lancement avec `qt6-wayland` et
`qt6-avif-image-plugin` déjà protégés.

![L'onglet Orphelins : cinq paquets que plus rien ne réclame, dont deux marqués d'un point jaune signalant qu'ils sont protégés contre la suppression.](docs/screenshots/orphans.png)

## Raccourcis

| Touche | Effet |
|---|---|
| `↑` `↓` / `j` `k` | naviguer (`PgUp`/`PgDn` par 10, `g`/`G` début/fin) |
| `PgUp` `PgDn` | (onglet Historique) faire défiler le détail de la transaction |
| `←` `→` / `h` `l` / `Tab` | changer d'onglet |
| `espace` | cocher / décocher |
| `a` / `n` | tout cocher / tout décocher |
| `p` | protéger / déprotéger |
| `/` | filtrer (nom et description ; Historique : par paquet), `Échap` annule |
| `r` | recharger l'état |
| `c` | voir ce que change la mise à jour sélectionnée |
| `u` | ouvrir le plan de l'action de l'onglet (Historique : le retour arrière) |
| `U` | (onglet Cache) purger les paquets désinstallés |
| `q` | quitter |

Sur l'écran de plan : `Entrée` lance, `Échap` annule, `↑` `↓` parcourent le détail.

Pendant l'exécution, les frappes vont à paru, `Ctrl-C` compris. Deux exceptions.

Les **touches de déplacement ne lui sont jamais transmises** : une invite `[O/n]` ne sait pas
les interpréter et les réafficherait en clair (`^[[B^[[A…`) au milieu de la réponse. Elles
font défiler le panneau — `↑` `↓`, `PgUp` `PgDn`, `Début` remonte tout, `Fin` redescend. Ce
défilement reste actif **après la fin de l'exécution**, qui est justement le moment où l'on
ouvre la sortie détaillée pour la relire de bout en bout. Le titre du cadre indique de
combien de lignes on est remonté.

`j` bascule entre la transcription et la sortie brute de paru, avant comme après la fin.

Une fois paru terminé, `Entrée` revient au tableau et recharge l'état.

Le pseudo-terminal est dimensionné sur le panneau **réellement rendu**, pas sur la hauteur de
la fenêtre. Les blocs « Téléchargement » et « À noter » apparaissent en cours de route et
rétrécissent ce panneau ; sans cet ajustement, les dernières lignes produites par paru —
celles qu'on cherche — tombaient hors cadre.

La ligne de statut sous la liste résume en permanence ce qui est coché (nombre, volume à
télécharger, paquets AUR). Elle est distincte de la ligne de légende, qui ne disparaît
jamais.

## Thème

Aucune couleur n'est codée en valeur absolue : nalarch n'utilise que les emplacements ANSI
du terminal et ne peint jamais son propre fond. Il suit donc le thème du terminal, clair
comme sombre, y compris lors d'un basculement à chaud.

Les fonds colorés (pastilles, ligne sélectionnée) passent par l'inversion vidéo : c'est la
seule façon d'obtenir un contraste correct sans connaître le thème actif. Un gris fixe
rendrait le texte discret illisible — gris sur gris — dès qu'on bascule.

## Construction

```bash
cargo build --release
install -Dm755 target/release/nalarch ~/.local/bin/nalarch
```

Dépendances système : `pacman`, `paru`, `pacman-contrib` (pour `checkupdates` et `paccache`).

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
| `18` | plan de retour arrière construit à partir de celle-ci |
| `19` | sortie brute de paru en fin d'exécution (4ᵉ nombre = lignes remontées) |

## Limites connues

- Le retour arrière ne porte que sur les paquets. Sur un système en Btrfs, `snap-pac` +
  `snapper` couvrent l'état complet du système de fichiers, ce qu'aucun gestionnaire de
  paquets ne peut faire.
- Ce qui n'est plus dans les caches n'est pas récupérable : la rétention `paccache` fixe la
  profondeur réelle de l'historique exploitable, quelle que soit la longueur du journal.
- Un paquet absent de tous les dépôts configurés est étiqueté `aur` ; il peut en réalité
  avoir été construit localement (`pacman -Qm` liste cet ensemble).
- La taille de téléchargement n'est affichée que si la base sync porte déjà la version
  cible ; sinon le champ est omis plutôt que d'afficher une valeur fausse.

## Licence

MIT. Voir [LICENSE](LICENSE).

## Empaquetage

`packaging/nalarch/` et `packaging/nalarch-git/` contiennent des PKGBUILD prêts pour l'AUR.
La marche à suivre est dans [packaging/README.md](packaging/README.md).
