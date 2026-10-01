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
