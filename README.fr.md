# nalarch

Gestionnaire de paquets **TUI** pour Arch Linux, dans l'esprit de [nala](https://github.com/volitank/nala)
côté Debian : voir ce qui va changer, se déplacer dedans, décider, puis appliquer.

*[English version](README.md) — c'est la version de référence ; celle-ci peut avoir un temps de retard.*

![nalarch à l'usage : l'écran de plan annonce ce que la transaction va faire et ce qui mérite un regard, l'écran d'exécution raconte chaque téléchargement, vérification, mise à jour et crochet au fil de l'opération, puis les onglets installés, orphelins, historique et cache sont parcourus.](docs/demo.gif)

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
| **Cache** | volume, anciennes versions, paquets désinstallés | `paccache -rk<N>` / `U` → `-ruk0` |

Rien ne se lance sans passer par un écran de validation. Il annonce ce que la transaction va
**réellement** faire — y compris les dépendances qu'elle tire au passage, que `checkupdates`
ne montre jamais, et les retraits que `-Rns` emporte en cascade — à côté des points qui
méritent un regard : compilation AUR, mise à jour partielle, noyau, paquet encore réclamé
par quelque chose qui reste.

Le panneau de détails affiche systématiquement **qui dépend du paquet sélectionné**
(`Requis par` / `Optionnel pour`) — l'information à regarder avant toute suppression.

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
| `/` | filtrer (nom et description ; Historique : par paquet), `Échap` annule |
| `c` | voir ce que change la mise à jour sélectionnée |
| `u` | ouvrir le plan de l'action de l'onglet (Historique : le retour arrière) |
| `U` | (onglet Cache) purger les paquets désinstallés |
| `r` | recharger l'état |
| `q` | quitter |

Sur l'écran de plan : `Entrée` lance, `Échap` annule, `↑` `↓` parcourent le détail.

Pendant l'exécution, les frappes vont à paru, `Ctrl-C` compris — on répond à ses questions et
on saisit le mot de passe sudo sans quitter l'interface. Les touches de déplacement font
exception : elles font défiler le panneau, avant comme après l'exécution, et `j` bascule
entre la transcription et la sortie brute de paru.

## Langue

Anglais par défaut, français quand l'environnement le demande : `LC_ALL`, `LC_MESSAGES` et
`LANG` sont consultés dans cet ordre, et `--lang en` / `--lang fr` force le choix.

L'anglais est la langue source ; `src/i18n.rs` porte la table française, et un test balaie
les sources à la recherche des appels sans entrée — une interface française ne peut donc pas
se remplir de phrases anglaises en silence. Rien à voir avec la locale imposée à paru : voir
[Comment ça marche](docs/design.fr.md).

## Installation

Depuis l'AUR, une fois publié :

```bash
paru -S nalarch          # ou nalarch-git
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
