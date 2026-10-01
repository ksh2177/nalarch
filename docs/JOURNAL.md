# Journal de bord — nalarch

> Une entrée par jour travaillé (jamais deux le même jour), format
> `## <emoji> <JJ MOIS AAAA> — <résumé>` avec `### Fait` (hashs de commits) et,
> si pertinent, `### Décisions prises` — le POURQUOI et les mesures, pas
> seulement le quoi. Journal démarré le 01/09/2026 ; l'historique antérieur
> vit dans `git log`.

## 📓 01 SEPTEMBRE 2026 — journal de bord démarré

Journal ouvert à la demande de Stephen : Skynet s'en sert pour suivre les sujets
entre sessions. État du repo au démarrage :

- TUI pacman ; dernier chantier le 20/08/2026 : gestion des orphelins
  (rôle nommé par orphelin, mapping vers les process qui mappent leurs
  fichiers), backfill des tailles du plan, aperçu des recettes AUR ('v').
  1 fichier modifié non commité.

## 🐢 02 SEPTEMBRE 2026 — démarrage lent : un miroir chaotic-aur en panne

### Fait

- Diagnostic du démarrage à 5-10 s et de l'erreur `failed retrieving file
  'chaotic-aur.db' from geo-mirror.chaotic.cx : Connection timed out`. Les deux
  ont la même cause, hors nalarch : `geo-mirror.chaotic.cx` redirige vers
  `paris-fr.silky.network`, qui ne répond plus (tout silky.network est dans cet
  état, de-mirror et nl-mirror inclus). `checkupdates` mangeait les 10 s de
  timeout pacman à chaque lancement.
- Correctif système (pas de commit) : ligne `geo-mirror` commentée dans
  `/etc/pacman.d/chaotic-mirrorlist`, pacman retombe sur `cdn-mirror` (Garuda,
  0,5 s). `checkupdates` : 10,9 s → 1,25 s.
- Doc : section « Dépannage / Troubleshooting » ajoutée aux deux README, avec la
  méthode `time checkupdates` pour nommer le miroir fautif.
- Rechute le soir même : nalarch à ~15 s, `checkupdates` mesuré à 32 s. Cette
  fois `cdn-mirror` répond 503, et les trois miroirs suivants de la liste
  (`br`, `de`, `in`) redirigent en 303 vers des hôtes qui ne répondent pas :
  pacman brûle son délai sur chacun avant d'atteindre `us-mirror`. Sondage de
  tous les `*-mirror.chaotic.cx` : seuls `de-2`, `de-4` et `us` répondent 200.
  Correctif système : décommenter `de-2` et `de-4` en tête de
  `/etc/pacman.d/chaotic-mirrorlist`, commenter `br`, `de`, `in`.

### Décisions prises

- Pas de refactor du démarrage pour l'instant. `App::new_app` attend
  `checkupdates` et `paru -Qua` avant le premier rendu ; la piste, si la
  dépendance au réseau redevient gênante, est d'afficher la table locale tout de
  suite et de charger les mises à jour en tâche de fond avec un indicateur dans
  l'onglet Mises à jour. Proposé à Stephen, non demandé.

## ⚡ 01 OCTOBRE 2026 — ouverture immédiate, et le miroir allemand à 0,3 Mo/s

### Fait

- `fb8b2c1` feat(startup) : nalarch s'ouvre sur `data::load_local` (alpm +
  cache, sans `checkupdates` / `paru -Qua` / `checkrebuild`), le `load()`
  complet tourne dans un thread et `App::poll_refresh` remplace l'état à son
  arrivée. Spinner sur l'onglet Mises à jour, `u` et `b` refusés tant que la
  vérification tourne. Même chemin pour `reload()`. Premier écran en ~0,8 s
  (debug), contre 36 s avant. C'est la piste notée le 02/09, demandée cette fois.
- `12548f0` : commit du travail du 02/09 resté en attente (dépannage, journal,
  pkgver).
- Téléchargements à ~200 Kio/s par paquet sur une fibre 8 Gb : cause système.
  Mirrorlist générée le 28/08 par `reflector --country France,Germany --sort
  rate`, jamais rafraîchie (timer désactivé) ; son premier serveur,
  `de.arch.niranjan.co`, était tombé à 0,3 Mo/s, et pacman n'abandonne un
  miroir que sur erreur, pas sur lenteur. Le même miroir faisait les 36 s de
  `checkupdates`. Mesures depuis le poste : miroirs FR 50-86 Mo/s.
- Correctif système (Stephen, sudo) : `reflector.conf` = France, https, age 12,
  latest 20, sort rate ; `reflector.timer` activé. Nouvelle tête de liste
  `mirrors.gandi.net` à 122 Mo/s. Ancienne liste sauvée en
  `/etc/pacman.d/mirrorlist.2026-08-28`.

- Écran d'exécution muet pendant tout le téléchargement (signalé par Stephen :
  « on sait rien de ce qui se passe »). Cause : pacman 7 n'imprime plus
  l'extension `.pkg.tar.zst` sur ses barres, et `download_line` exigeait
  `.pkg.tar` avant même de lire le débit — aucun téléchargement reconnu, bloc
  Téléchargement jamais affiché. Diagnostic sur une sortie RÉELLE capturée
  (`fakeroot pacman -Sw` sur une copie de base, cache jetable, sous `script`).
  Corrigé : nom reconnu par sa release numérique, ligne `Total (n/m)` lue,
  fichiers en cours suivis et affichés un par ligne. Script de démo aligné
  sur le vrai format (il masquait le bug en gardant l'ancien).
- Autre écart constaté : paru « nothing to do » alors que nalarch montrait
  2 electron. `mirrors.gandi.net` (classé 1er, 122 Mo/s) avait ~3 h de retard,
  et `--age 12` l'acceptait ; `checkupdates` avait vu une base plus fraîche.
  Proposé : `--age 2` dans reflector.conf (hogwarts.fr passe en tête).
- Côté nalarch, ce cas est désormais nommé au lieu de finir sur « toutes les
  étapes ont abouti » : `Journal::nothing_to_do` + `ui::skipped_by_paru`
  (paquets dépôt du plan non traités, seulement pour un `-Syu`). Vérifié en
  rejouant la démo patchée localement (patch non commité).

### Décisions prises

- Pas de France+Allemagne : le classement par débit de reflector se fait depuis
  archlinux.org, pas depuis le poste, et a écarté tous les FR le 28/08. France
  seule suffit largement en débit et reste proche.
- Ne jamais activer `reflector.timer` avec la conf par défaut (`--latest 5
  --sort age`, sans pays) : pire que la liste figée.
- `cargo fmt --check` échouait déjà avant la session ; pas de reformatage de
  masse mêlé à la fonctionnalité.
