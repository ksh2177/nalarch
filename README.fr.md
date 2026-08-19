# nalarch

Gestionnaire de paquets **TUI** pour Arch Linux, dans l'esprit de [nala](https://github.com/volitank/nala)
côté Debian : voir ce qui va changer, se déplacer dedans, décider, puis appliquer.

*[English version](README.md) — c'est la version de référence ; celle-ci peut avoir un temps de retard.*

![nalarch à l'usage : l'écran de plan annonce ce que la transaction va faire et ce qui mérite un regard, l'écran d'exécution raconte chaque téléchargement, vérification, mise à jour et crochet au fil de l'opération, puis le changelog d'une mise à jour est ouvert — le verdict, le journal de packaging d'Arch, les notes de version amont — avant que les onglets installés, orphelins, historique, recherche et cache soient parcourus.](docs/demo.gif)

## Principe

**libalpm en lecture, paru en écriture.**

Aucune logique de résolution de dépendances ni de compilation AUR n'est réimplémentée.
nalarch lit la base de données via `libalpm` — la même bibliothèque que pacman et paru —
affiche l'état, puis délègue chaque action à `paru`, qui reste seul maître de la
transaction. Il ne peut pas diverger du comportement de paru, puisque c'est paru qui agit.

paru ne tourne pas *à côté* de nalarch : il tourne **dedans**, dans un pseudo-terminal. On
ne quitte jamais l'interface, et la sortie de paru est analysée puis racontée, pas relayée.

```
             ┌─ libalpm ────────────► état, plan, tailles, dépendances inverses
   nalarch ──┤
             └─ PTY ──► paru ──► vt100 ──► panneau encadré, étape, invites
```

Les mises à jour sont détectées avec `checkupdates` plutôt qu'avec `pacman -Sy` : il
synchronise dans une base temporaire, donc aucun risque de laisser le système en
*partial upgrade*.

**[Comment ça marche](docs/design.fr.md)** parcourt les écrans, l'analyse et le raisonnement
en détail — pourquoi la locale est figée, comment le changelog est trouvé, de quoi la
transcription est faite.

## Fonctionnalités

| Onglet | Contenu | Action (`u`) |
|---|---|---|
| **Mises à jour** | dépôts + AUR, ancienne → nouvelle version, taille de téléchargement | `paru -Syu` |
| **Installés** | paquets explicites dont rien ne dépend (≈ `pacman -Qett`) | `paru -Rns` |
| **Orphelins** | tirés comme dépendance, plus réclamés par rien (= `pacman -Qdt`) | `paru -Rns` |
| **Historique** | toutes les transactions passées, et ce qu'un retour arrière rétablirait | `pacman -U` depuis le cache |
| **Recherche** | dépôts et AUR dans une seule liste, avec votes et mainteneur | `paru -S` |
| **Cache** | volume, anciennes versions, paquets désinstallés | `paccache -rk<N>` / `U` → `-ruk0` |

Rien ne se lance sans passer par un écran de validation. Il annonce ce que la transaction va
**réellement** faire — y compris les dépendances qu'elle tire au passage, que `checkupdates`
ne montre jamais, et les retraits que `-Rns` emporte en cascade — à côté des points qui
méritent un regard : compilation AUR, mise à jour partielle, noyau, paquet encore réclamé
par quelque chose qui reste.

Le panneau de détails affiche systématiquement **qui dépend du paquet sélectionné**
(`Requis par` / `Optionnel pour`) — l'information à regarder avant toute suppression.

**Installés** laisse volontairement de côté les dépendances : 261 paquets sur 1883 sur une
machine ordinaire. C'est ce qui en fait la liste de *tes* applications plutôt qu'un déversoir
du système — mais c'est aussi pourquoi y chercher une dépendance ne donne rien. La ligne
d'état le dit, et nomme le paquet quand il est malgré tout installé. Pour demander « est-ce
que c'est là ? » à propos de n'importe quoi, l'onglet Recherche couvre tous les paquets,
installés ou non.

## Historique et retour arrière

L'onglet **Historique** lit `/var/log/pacman.log` plutôt qu'un journal qui lui serait propre.
Sur Arch, toute opération sur les paquets passe par libalpm et atterrit dans ce fichier —
`pacman`, `paru`, une dépendance tirée par un script — l'historique est donc complet et
rétroactif, y compris ce qui a été fait avant que nalarch n'existe. Le filtre `/` porte sur
les noms de paquets : il répond à « quand est-ce que *ce* paquet a changé, et vers quoi ? ».

`u` construit la **transaction inverse** à partir des caches locaux : ce qui a été installé
est retiré, ce qui a été mis à jour redescend, ce qui a été supprimé revient. Rien n'est
téléchargé. Le verdict — combien de paquets sont encore rétablissables — est affiché avant
la liste des opérations, parce que sur une mise à jour de cinq cents paquets c'est lui qui
décide.

Trois choses qu'un retour de paquets ne fait pas, et que l'écran de plan énonce :

- **Il ne revient pas sur l'état du système.** Les fichiers posés par les paquets
  redescendent ; ce qu'un scriptlet ou un hook a écrit depuis — migration de base, config
  réécrite — reste tel quel. Pour un vrai retour d'état, il faut un instantané (snapper/Btrfs).
- **Il ne tient pas.** La prochaine mise à jour complète remonte les versions restaurées,
  sauf `IgnorePkg`.
- **Il peut fabriquer un état inédit.** Quand moins de la moitié d'une grosse transaction est
  récupérable, le système se retrouve avec un mélange des deux versions, jamais livré ni
  testé ainsi. C'est signalé comme risque sérieux, en tête de liste.

## Chercher, et installer

`/` saisit une requête, `Entrée` la lance. Les dépôts sont interrogés via libalpm, sans
sous-processus à analyser ; l'AUR via son point d'API, dans un fil séparé pour qu'une
résolution lente ne fige jamais l'interface.

Les résultats sont classés par ce qu'on voulait probablement : le nom exact d'abord, puis un
début de nom, puis une sous-chaîne. À pertinence égale, la source relue passe devant, puis la
popularité que l'AUR mesure lui-même. Classer tous les dépôts avant tout l'AUR paraît prudent,
mais chercher `yazi` enterre alors le paquet AUR de ce nom sous un `libyazi` sans rapport.

Un résultat AUR porte ce sur quoi l'AUR lui-même le juge : votes, popularité, signalement de
péremption par un utilisateur, et mainteneur — **aucun mainteneur signifie orphelin**, ce qui
est la chose la plus utile à savoir avant de compiler un PKGBUILD sous son propre compte.

`espace` coche un résultat, `u` ouvre son plan. Ce plan sépare ce que tu as **demandé** de ce
qui arrive avec : demander un paquet en amène couramment une dizaine, et c'est en général
présenté comme un mur de noms juste avant une demande de confirmation.

## La liste de protection

`~/.config/nalarch/keep.list`

libalpm ne connaît que les dépendances **déclarées**. Un paquet chargé dynamiquement —
plugin Qt, backend Wayland, greffon GStreamer — apparaît donc comme orphelin alors qu'il est
vital. Sur une machine Hyprland, `qt6-wayland` est le cas d'école : `pacman -Qdtq` le liste,
et un `pacman -Rns $(pacman -Qdtq)` en aveugle casse le shell graphique.

Les paquets de cette liste s'affichent en jaune avec la marque `[·]` et **ne peuvent pas être
cochés pour suppression**. La touche `p` ajoute ou retire la protection du paquet
sélectionné. Le fichier est créé au premier lancement avec `qt6-wayland` et
`qt6-avif-image-plugin` déjà protégés.

## Raccourcis

| Touche | Effet |
|---|---|
| `↑` `↓` / `j` `k` | naviguer (`PgUp`/`PgDn` par 10, `g`/`G` début/fin) |
| `←` `→` / `h` `l` / `Tab` | changer d'onglet |
| `espace` | cocher / décocher |
| `a` / `n` | tout cocher / tout décocher |
| `p` | protéger / déprotéger |
| `/` | filtrer (nom et description ; Historique : par paquet ; Recherche : la requête), `Échap` annule |
| `c` | voir ce que change la mise à jour sélectionnée |
| `u` | ouvrir le plan de l'action de l'onglet (Historique : le retour arrière) |
| `U` | (onglet Cache) purger les paquets désinstallés |
| `r` | recharger l'état |
| `q` | quitter |

Sur l'écran de plan : `Entrée` lance, `Échap` annule, `↑` `↓` parcourent le détail.

Pendant l'exécution, les frappes vont à paru, `Ctrl-C` compris — on répond à ses questions et
on saisit le mot de passe sudo sans quitter l'interface. Les touches de déplacement font
exception : elles font défiler la vue affichée, avant comme après l'exécution, et `j` bascule
entre la transcription et la sortie brute de paru.

La transcription se remonte sur toutes les opérations, pas seulement celles qui tiennent à
l'écran : une mise à jour de soixante-quinze paquets est justement le moment où l'on veut
revoir ce qui s'est passé plus tôt. Tant qu'on suit, les nouvelles opérations poussent la vue ;
une fois remonté elle reste où on l'a mise, et `Fin` reprend le suivi.

## Icônes

Actives par défaut, tirées du jeu Material Design que portent les Nerd Fonts.

Le seul endroit où elles ne peuvent pas fonctionner est un TTY nu — aucune police patchée n'y
est chargée et chaque glyphe sortirait en carré vide, ce qui est aussi un moment où nalarch
sert. Ce cas est détecté plutôt que laissé à l'utilisateur : `TERM=linux` et ses semblables
désignent une console, et une console ne sait pas les dessiner quoi qu'on configure. Un
multiplexeur n'est pas une console : `tmux` et `screen` les gardent.

Hors console, la police ne peut pas être interrogée : `--no-icons` ou `icons = false` dans
`~/.config/nalarch/config` restent donc là pour qui n'en a pas de patchée. Elles supposent une
variante **mono-chasse (Mono)** — les variantes double largeur dessinent ces glyphes sur deux
cellules alors que la mise en page en compte une, ce qui décale toutes les colonnes suivantes.

Rien de ce qu'elles montrent ne porte de sens à soi seul : chaque glyphe accompagne le mot
qu'il décore, sur les onglets et dans la colonne du dépôt. Les éteindre fait perdre de la
décoration, pas de l'information.

## Langue

Anglais par défaut, français quand l'environnement le demande : `LC_ALL`, `LC_MESSAGES` et
`LANG` sont consultés dans cet ordre, et `--lang en` / `--lang fr` force le choix.

L'anglais est la langue source ; `src/i18n.rs` porte la table française, et un test balaie
les sources à la recherche des appels sans entrée — une interface française ne peut donc pas
se remplir de phrases anglaises en silence. Rien à voir avec la locale imposée à paru : voir
[Comment ça marche](docs/design.fr.md).

## Installation

Depuis l'AUR, une fois publié (les inscriptions y sont temporairement suspendues) :

```bash
paru -S nalarch          # ou nalarch-git
```

En attendant, directement depuis ce dépôt — le PKGBUILD épingle le digest du tarball du tag :

```bash
git clone https://github.com/ksh2177/nalarch && cd nalarch/packaging/nalarch
makepkg -si
```

Depuis les sources :

```bash
cargo build --release
install -Dm755 target/release/nalarch ~/.local/bin/nalarch
```

Dépendances système : `pacman`, `paru`, `pacman-contrib` (pour `checkupdates` et `paccache`),
`sudo`. Les PKGBUILD sont dans [`packaging/`](packaging/README.md).

Deux modes aident quand il n'y a rien à mettre à jour : `nalarch --demo` rejoue une session
ressemblant à paru sans toucher au système, et `nalarch --dump` rend un écran en texte brut
sans TTY. Les deux sont décrits dans [Comment ça marche](docs/design.fr.md).

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

## Comment c'est fait

nalarch est une collaboration entre un humain et une IA, et je préfère le dire
clairement plutôt que vous le laissiez découvrir dans un trailer de commit. J'ai
conçu l'outil, pris les décisions d'architecture et d'ergonomie, je relis le code,
je l'utilise tous les jours sur ma propre machine et je le maintiens. L'essentiel du
code a été écrit en binôme avec Claude (Anthropic), sous cette direction.

Les outils de cette catégorie méritent la méfiance — nalarch se place devant votre
gestionnaire de paquets. C'est aussi pourquoi sa conception est volontairement
conservatrice : libalpm n'est utilisé qu'en lecture, chaque écriture passe par paru
dans un pseudo-terminal (aucune résolution de dépendances ni logique de build n'est
réimplémentée ici), et les pièges de mise à jour partielle comme un `pacman -Sy` nu
sont évités par construction, avec des commentaires dans le source qui expliquent
pourquoi.

Jugez-le comme n'importe quel autre outil : sur son code, son suivi d'issues et son
historique à partir de maintenant. Les rapports de bugs sont bienvenus.
