//! User-facing strings.
//!
//! English is the source language: every message appears in English at its call
//! site, which keeps the code readable to anyone. `t()` looks that English
//! string up in the translation table for the active locale and returns it
//! unchanged when there is no entry — a missing translation degrades to
//! English rather than to a key name or a panic.
//!
//! Interpolation uses `{0}`, `{1}` placeholders instead of `format!`, because
//! `format!` needs a literal and a translated string is only known at runtime.
//!
//! The table is checked for completeness by a test that scans the source tree
//! for call sites: a string added here without its translation fails the build
//! rather than silently reverting to English on a French system.

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
}

static LANG: OnceLock<Lang> = OnceLock::new();

/// Picks the interface language, once, at startup.
///
/// `--lang en` / `--lang fr` wins; otherwise the usual environment variables
/// are consulted in the order the C library itself uses them. Anything that is
/// not French falls back to English rather than guessing.
pub fn init(args: &[String]) {
    let forced = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str());
    let lang = match forced {
        Some("fr") => Lang::Fr,
        Some(_) => Lang::En,
        None => {
            let env = std::env::var("LC_ALL")
                .or_else(|_| std::env::var("LC_MESSAGES"))
                .or_else(|_| std::env::var("LANG"))
                .unwrap_or_default();
            if env.starts_with("fr") {
                Lang::Fr
            } else {
                Lang::En
            }
        }
    };
    let _ = LANG.set(lang);
}

pub fn lang() -> Lang {
    *LANG.get().unwrap_or(&Lang::En)
}

/// Translates a message. The English text is its own key.
///
/// A key may carry a context prefix, `"context|Text"`, for the cases where the
/// same English word needs two different translations — "Installed" is a tab
/// full of packages in one place and a single package's state in another, and
/// French tells them apart. English keeps only the part after the bar.
pub fn t(s: &'static str) -> &'static str {
    match lang() {
        Lang::En => strip_context(s),
        Lang::Fr => table()
            .get(s)
            .copied()
            .unwrap_or_else(|| strip_context(s)),
    }
}

fn strip_context(s: &'static str) -> &'static str {
    match s.split_once('|') {
        Some((_, text)) => text,
        None => s,
    }
}

/// Translates, then substitutes `{0}`, `{1}`… by position.
///
/// Positional rather than sequential: a translation is free to reorder its
/// arguments, which French regularly needs.
pub fn tf(s: &'static str, args: &[&str]) -> String {
    let mut out = t(s).to_string();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| FR.iter().copied().collect())
}

/// French translations. Keys are the English source strings, verbatim.
#[rustfmt::skip]
const FR: &[(&str, &str)] = &[
    // theme.rs — size units
    ("B", "o"), ("KiB", "Kio"), ("MiB", "Mio"), ("GiB", "Gio"), ("TiB", "Tio"),

    // changelog.rs
    ("AUR package: no packaging log. paru will show the PKGBUILD before building.",
     "Paquet AUR : pas de journal de packaging. paru affichera le PKGBUILD avant de compiler."),
    ("No release notes published for this tag.",
     "Aucune note de version publiée pour cette étiquette."),
    ("Upstream notes cannot be fetched automatically: {0}",
     "Notes amont non récupérables automatiquement : {0}"),
    ("Packaging log unreachable (network?).",
     "Journal de packaging injoignable (réseau ?)."),
    ("This package has no Arch packaging repository.",
     "Ce paquet n'a pas de dépôt de packaging Arch."),

    // plan.rs
    ("Updates", "Mises à jour"),
    ("New packages", "Nouveaux paquets"),
    ("Downgrades", "Retours arrière"),
    ("Removals", "Suppressions"),
    ("Removed as a cascade", "Retirés en cascade"),
    ("paru cache", "cache paru"),
    ("cache", "cache"),
    ("installed", "installé"),

    // data.rs
    ("nalarch — packages protected from removal.",
     "nalarch — paquets protégés contre la suppression."),
    ("One name per line. They stay visible in the Orphans tab but cannot be checked for removal.",
     "Un nom par ligne. Ils restent visibles dans l'onglet Orphelins mais ne peuvent pas être cochés pour suppression."),
    ("libalpm only knows about declared dependencies. A package loaded dynamically (a Qt plugin, a Wayland backend, a GStreamer plugin) therefore looks like an orphan while being vital. Hence this list.",
     "libalpm ne connaît que les dépendances déclarées. Un paquet chargé dynamiquement (plugin Qt, backend Wayland, greffon GStreamer) apparaît donc comme orphelin alors qu'il est vital. D'où cette liste."),
    ("Needed by quickshell (Hyprland shell) with no alpm dependency link:",
     "Requis par quickshell (shell Hyprland) sans lien de dépendance alpm :"),

    // ui.rs
    ("{0} packages · cache {1}", "{0} paquets · cache {1}"),
    ("System is up to date.", "Système à jour."),
    ("No orphans.", "Aucun orphelin."),
    ("Nothing to show.", "Rien à afficher."),
    ("Details", "Détails"),
    ("Detail", "Détail"),
    ("No per-package detail for this action.",
     "Aucun détail par paquet pour cette action."),
    ("Repository", "Dépôt"),
    ("Version", "Version"),
    ("Target", "Cible"),
    ("Note", "Note"),
    ("built from source", "compilation depuis les sources"),
    ("Download", "Téléchargement"),
    ("Size", "Taille"),
    ("Installed", "Installé"),
    ("explicitly", "explicitement"),
    ("as a dependency", "comme dépendance"),
    ("🔒 protected — removal blocked", "🔒 protégé — suppression bloquée"),
    ("Required by: nothing", "Requis par : rien"),
    ("Required by ({0}):", "Requis par ({0}) :"),
    ("… and {0} others", "… et {0} autres"),
    ("Optional for ({0}):", "Optionnel pour ({0}) :"),
    ("Depends on: {0} package(s)", "Dépend de : {0} paquet(s)"),
    ("Location", "Emplacement"),
    ("Files", "Fichiers"),
    ("Total size", "Taille totale"),
    ("Retention", "Rétention"),
    ("{0} versions per package", "{0} versions par paquet"),
    ("Old versions", "Anciennes versions"),
    ("Uninstalled packages", "Paquets désinstallés"),
    ("The cache is what makes a rollback possible: old versions stay",
     "Le cache est ce qui rend un retour arrière possible : les anciennes"),
    ("reinstallable there through pacman -U. Do not empty it entirely —",
     "versions y restent réinstallables via pacman -U. Ne le videz pas"),
    ("it is the only safety net short of a Btrfs snapshot.",
     "entièrement — c'est le seul filet de sécurité sans instantané Btrfs."),
    ("— applies the configured retention ({0} versions)",
     "— applique la rétention configurée ({0} versions)"),
    ("— purges packages that are no longer installed",
     "— purge les paquets qui ne sont plus installés"),
    ("Package cache", "Cache des paquets"),
    ("filter “{0}”", "filtre « {0} »"),
    ("No transaction in /var/log/pacman.log.",
     "Aucune transaction dans /var/log/pacman.log."),
    ("No transaction touches this package.",
     "Aucune transaction ne touche ce paquet."),
    ("Date", "Date"),
    ("Command", "Commande"),
    ("Duration", "Durée"),
    ("State", "État"),
    ("less than a second", "moins d'une seconde"),
    ("interrupted — the log carries no close",
     "interrompue — le journal ne porte pas de clôture"),
    ("Rollback", "Retour arrière"),
    ("Nothing to undo: this transaction only reinstalled.",
     "Rien à défaire : cette transaction n'a fait que réinstaller."),
    ("Impossible: none of the versions involved is still in cache.",
     "Impossible : aucune des versions concernées n'est encore en cache."),
    ("{0} package(s) restorable — a complete rollback is possible.",
     "{0} paquet(s) rétablissables — retour complet possible."),
    ("{0} restorable of {1} — {2} out of cache, left as they are.",
     "{0} rétablissables sur {1} — {2} hors cache, laissés en l'état."),
    ("build the inverse transaction", "construire la transaction inverse"),
    ("pacman warnings", "Avertissements de pacman"),
    ("Operations · {0}", "Opérations · {0}"),
    ("out of cache", "hors cache"),
    ("Detail · lines {0}-{1} of {2}", "Détail · lignes {0}-{1} sur {2}"),
    ("Transaction detail", "Détail de la transaction"),
    ("Enter confirms · Esc cancels", "Entrée valide · Échap annule"),
    ("{0} transactions since {1} — everything that went through pacman, nalarch or not",
     "{0} transactions depuis le {1} — tout ce qui est passé par pacman, nalarch ou non"),
    ("{0} update(s) frozen by IgnorePkg: {1}",
     "{0} mise(s) à jour figée(s) par IgnorePkg : {1}"),
    ("Points of attention", "Points d'attention"),
    ("Points of attention · PgUp/PgDn to scroll",
     "Points d'attention · PgUp/PgDn pour défiler"),
    ("Enter", "Entrée"),
    ("Esc", "Échap"),
    ("launch", "lancer"),
    ("cancel", "annuler"),
    ("↑↓ walk the detail", "↑↓ parcourir le détail"),
    ("command:", "commande :"),
    ("Frozen (IgnorePkg)", "Figés (IgnorePkg)"),
    ("of them built (AUR)", "dont compilés (AUR)"),
    ("To download", "À télécharger"),
    ("Space freed", "Place rendue"),
    ("Disk space", "Espace disque"),
    ("{0} package(s) cannot be sized", "{0} paquet(s) non chiffrables"),
    ("Already cached, nothing to download",
     "Déjà en cache, rien à télécharger"),
    ("Sizes unknown: built from source", "Tailles inconnues : compilation"),
    ("Summary", "Résumé"),
    ("Nothing in particular to report on this transaction.",
     "Rien de particulier à signaler sur cette transaction."),
    ("cached", "en cache"),
    ("Detail · {0} package(s)", "Détail · {0} paquet(s)"),
    ("plan:", "plan :"),
    ("{0} package(s)", "{0} paquet(s)"),
    ("download", "téléchargement"),
    ("freed", "libéré"),
    ("net", "net"),
    ("sizes", "tailles"),
    ("unknown before building", "inconnues avant compilation"),
    ("running", "en cours"),
    ("interrupted (code {0})", "interrompu (code {0})"),
    ("failed (code {0})", "échec (code {0})"),
    ("step:", "étape :"),
    ("Packages", "Paquets"),
    ("Latest", "Dernier"),
    ("Waiting for the first operations…",
     "En attente des premières opérations…"),
    ("{0} configuration file(s) to merge: {1}",
     "{0} fichier(s) de configuration à fusionner : {1}"),
    ("Your version was kept; the new one waits beside it, unapplied.",
     "Ta version a été conservée ; la nouvelle attend à côté, non appliquée."),
    ("Compare and merge:", "Compare et fusionne :"),
    ("(pacman-contrib) — without it, the new configuration never applies.",
     "(pacman-contrib) — sans ça, la nouvelle configuration ne s'applique jamais."),
    ("Reboot required: {0}", "Redémarrage nécessaire : {0}"),
    ("input expected", "saisie attendue"),
    ("Type your password then Enter — nothing shows, that is normal.",
     "Tape ton mot de passe puis Entrée — rien ne s'affiche, c'est normal."),
    ("Type your answer then Enter · j shows the raw output · Ctrl-C interrupts",
     "Tape ta réponse puis Entrée · j pour voir la sortie brute · Ctrl-C interrompt"),
    ("j paru's raw output · ↑↓ scroll · every other key is forwarded to it",
     "j sortie brute de paru · ↑↓ défiler · les autres touches lui sont transmises"),
    ("Finished · j paru's detailed output · ↑↓ PgUp PgDn scroll · Enter to return",
     "Terminé · j sortie détaillée de paru · ↑↓ PgUp PgDn défiler · Entrée revenir"),
    ("{0} · j raw output · ↑↓ PgUp PgDn scroll · Enter back to the table",
     "{0} · j sortie brute · ↑↓ PgUp PgDn défiler · Entrée revenir au tableau"),
    ("Interrupted", "Interrompu"),
    ("Failed (code {0})", "Échec (code {0})"),
    ("output of {0}", "sortie de {0}"),
    ("output of {0} · scrolled back {1} line(s) · End to return",
     "sortie de {0} · remonté de {1} ligne(s) · Fin pour redescendre"),
    ("Worth noting", "À noter"),
    ("fetching the packaging log and the release notes…",
     "récupération du journal de packaging et des notes de version…"),
    ("upstream:", "amont :"),
    ("unknown", "inconnu"),
    ("Loading…", "Chargement…"),
    ("Arch packaging log", "Journal de packaging Arch"),
    ("installed version", "version installée"),
    ("Upstream release notes", "Notes de version amont"),
    ("Nothing published for this version.",
     "Rien de publié pour cette version."),
    ("Changes", "Changements"),
    ("back", "revenir"),
    ("↑↓ PgUp/PgDn scroll", "↑↓ PgUp/PgDn défiler"),
    ("u update", "u mettre à jour"),
    ("u remove", "u supprimer"),
    ("u roll back", "u revenir en arrière"),
    ("u clean", "u nettoyer"),
    ("navigate", "naviguer"),
    ("tab", "onglet"),
    ("space", "espace"),
    ("check", "cocher"),
    ("all/none", "tout/rien"),
    ("protect", "protéger"),
    ("changes", "changements"),
    ("filter", "filtrer"),
    ("filter by package", "filtrer par paquet"),
    ("reload", "recharger"),
    ("quit", "quitter"),

    // main.rs
    ("Demo", "Démonstration"),
    ("Demo — the system is not modified",
     "Démonstration — aucune modification du système"),
    ("Replayed output: no pacman command is invoked.",
     "Sortie rejouée : aucune commande pacman n'est appelée."),
    ("demo render", "rendu de démonstration"),
    ("Partial upgrade", "Mise à jour partielle"),
    ("State reloaded", "État rechargé"),
    ("a TUI package manager for Arch", "gestionnaire de paquets TUI pour Arch"),
    ("start the interface", "lance l'interface"),
    ("force the interface language", "force la langue de l'interface"),
    ("replay a session without touching the system",
     "rejoue une session sans toucher au système"),
    ("render one screen as plain text (no TTY)",
     "rend un écran en texte brut (sans TTY)"),
    ("this message", "ce message"),

    // exec.rs
    ("finished", "terminé"),
    ("starting…", "démarrage…"),
    ("every step succeeded", "toutes les étapes ont abouti"),

    // app.rs
    ("tab|Installed", "Installés"),
    ("Orphans", "Orphelins"),
    ("History", "Historique"),
    ("Cache", "Cache"),
    ("{0} is protected — press p to lift the protection",
     "{0} est protégé — touche « p » pour lever la protection"),
    ("{0} package(s) checked", "{0} paquet(s) cochés"),
    ("Selection cleared", "Sélection vidée"),
    ("{0} to update · {1} to download",
     "{0} à mettre à jour · {1} à télécharger"),
    (" · {0} from the AUR (built from source)",
     " · {0} depuis l'AUR (compilation)"),
    ("{0} to remove · {1} freed", "{0} à supprimer · {1} libérés"),
    ("{0} protected", "{0} protégé"),
    ("{0} unprotected", "{0} déprotégé"),
    ("Failed: {0}", "Échec : {0}"),
    ("No package checked — press a to check them all",
     "Aucun paquet coché — « a » pour tout cocher"),
    ("{0} update(s) excluded through --ignore: {1}.",
     "{0} mise(s) à jour exclue(s) via --ignore : {1}."),
    ("A partial upgrade is not supported on Arch: keep it for one-off cases.",
     "Une mise à jour partielle n'est pas supportée sur Arch : à réserver aux cas ponctuels."),
    ("{0} AUR package(s): built from source, with unpredictable duration and size.",
     "{0} paquet(s) AUR : compilation depuis les sources, durée et taille imprévisibles."),
    ("paru will ask its own questions (reading the PKGBUILD, PGP keys).",
     "paru posera ses propres questions (relecture du PKGBUILD, clés PGP)."),
    ("paru will run without asking again: this approval is what counts.",
     "paru s'exécutera sans reposer de question : cette validation fait foi."),
    ("Update · {0} package(s)", "Mise à jour · {0} paquet(s)"),
    (", {0} of them new", " dont {0} nouveau(x)"),
    (" · {0} left out", " · {0} écarté(s)"),
    ("No package checked", "Aucun paquet coché"),
    ("Removal · {0} package(s)", "Suppression · {0} paquet(s)"),
    ("-Rns also removes dependencies nothing needs any more, along with configuration files.",
     "-Rns retire aussi les dépendances devenues inutiles et les fichiers de configuration."),
    ("Frees roughly {0} of old versions.",
     "Libère environ {0} d'anciennes versions."),
    ("No transaction selected", "Aucune transaction sélectionnée"),
    ("This transaction has nothing to undo (a reinstall only)",
     "Cette transaction n'a rien à défaire (réinstallation seule)"),
    ("Rollback impossible: none of the versions involved is still in cache",
     "Retour impossible : aucune des versions concernées n'est encore en cache"),
    ("Transaction of {0} — {1}.", "Transaction du {0} — {1}."),
    ("The packages come from local caches: nothing is downloaded, no repository is queried.",
     "Les paquets viennent des caches locaux : rien n'est téléchargé, aucun dépôt n'est interrogé."),
    ("{0} package(s) will stay as they are, for lack of a cached version.",
     "{0} paquet(s) resteront en l'état, faute de version en cache."),
    ("Rollback · {0} package(s) of {1}",
     "Retour arrière · {0} paquet(s) sur {1}"),
    ("Cannot launch: {0}", "Lancement impossible : {0}"),
    ("No pending update for this package",
     "Aucune mise à jour en attente pour ce paquet"),
    ("Purge uninstalled packages from the cache",
     "Purge des paquets désinstallés du cache"),
    ("Frees roughly {0} — these packages are no longer installed, but you lose the ability to reinstall them offline.",
     "Libère environ {0} — ces paquets ne sont plus installés, mais tu perds la possibilité de les réinstaller hors ligne."),

    // journal.rs
    ("preparing", "préparation"),
    ("syncing repositories", "synchronisation des dépôts"),
    ("downloading", "téléchargement"),
    ("verifying", "vérification"),
    ("installing", "installation"),
    ("post-transaction hooks", "crochets de post-transaction"),
    ("building", "compilation"),
    ("Downloaded", "Téléchargé"),
    ("Verified", "Vérifié"),
    ("Installed", "Installé"),
    ("Upgraded", "Mis à jour"),
    ("Downgraded", "Rétrogradé"),
    ("Reinstalled", "Réinstallé"),
    ("Removed", "Supprimé"),
    ("Hook", "Crochet"),
    ("Built", "Compilation"),
    ("keyring keys", "clés du trousseau"),
    ("package integrity", "intégrité des paquets"),
    ("loading files", "chargement des fichiers"),
    ("file conflicts", "conflits de fichiers"),
    ("available disk space", "espace disque disponible"),
    ("{0}: upgrade skipped ({1})", "{0} : mise à jour ignorée ({1})"),
    ("permissions differ on {0}", "permissions différentes sur {0}"),
    ("file information unreadable: {0}", "informations de fichier illisibles : {0}"),
    ("{0}: already up to date, skipped", "{0} : déjà à jour, ignoré"),

    // history.rs
    ("upgraded", "mis à jour"),
    ("downgraded", "rétrogradé"),
    ("removed", "supprimé"),
    ("reinstalled", "réinstallé"),
    ("installed (plural)", "installés"),
    ("upgraded (plural)", "mis à jour"),
    ("downgraded (plural)", "rétrogradés"),
    ("removed (plural)", "supprimés"),
    ("reinstalled (plural)", "réinstallés"),
    ("no change", "aucune modification"),
    ("system upgrade", "mise à jour du système"),
    ("system upgrade · {0}", "mise à jour du système · {0}"),
    ("install from file · {0}", "installation depuis fichier · {0}"),
    ("removal · {0}", "suppression · {0}"),
    ("install · {0}", "installation · {0}"),
    ("unknown origin", "origine inconnue"),
    ("just now", "à l'instant"),
    ("{0} min ago", "il y a {0} min"),
    ("{0} h ago", "il y a {0} h"),
    ("{0} d ago", "il y a {0} j"),
    ("{0} months ago", "il y a {0} mois"),
    ("sudo pacman -U ({0} package(s) taken from the local caches)",
     "sudo pacman -U ({0} paquet(s) pris dans les caches locaux)"),

    // risks.rs
    ("{0} package(s) built from the AUR: {1}",
     "{0} paquet(s) compilés depuis l'AUR : {1}"),
    ("A PKGBUILD is a script run under your account, reviewed by nobody. paru will offer to show it before building: that is the moment to read it.",
     "Un PKGBUILD est un script exécuté sous ton compte, sans relecture par personne. paru proposera de l'afficher avant de compiler : c'est le moment de le lire."),
    ("Partial upgrade: {0} package(s) left out",
     "Mise à jour partielle : {0} paquet(s) écartés"),
    ("Left out: {0}. Arch packages are compiled against one another; keeping old ones alongside new ones can break programs with no apparent connection.",
     "Écartés : {0}. Les paquets d'Arch sont compilés les uns contre les autres ; en garder d'anciens à côté de nouveaux peut casser des programmes sans rapport apparent."),
    ("Until you reboot, plugging in a device or mounting an unusual filesystem may fail: the running kernel's modules are no longer on disk.",
     "Tant que tu n'as pas redémarré, brancher un périphérique ou monter un système de fichiers inhabituel peut échouer : les modules du noyau en cours d'exécution ne sont plus sur le disque."),
    ("{0} DKMS module(s) will be rebuilt, which lengthens the operation.",
     "{0} module(s) DKMS seront recompilés, ce qui allonge l'opération."),
    ("Kernel upgraded ({0}): reboot required",
     "Noyau mis à jour ({0}) : redémarrage nécessaire"),
    ("Essential system components: {0}",
     "Composants système essentiels : {0}"),
    ("Do not interrupt the operation once started. A cut here can stop the machine from booting.",
     "Ne pas interrompre l'opération une fois lancée. Une coupure à ce moment peut empêcher la machine de redémarrer."),
    ("Back to an earlier version: {0}",
     "Retour à une version antérieure : {0}"),
    ("The offered version is older than the installed one. Normal after a deliberate rollback, suspicious otherwise.",
     "La version proposée est plus ancienne que celle installée. C'est normal après un retour en arrière volontaire, suspect sinon."),
    ("{0} new package(s) pulled in as dependencies",
     "{0} nouveau(x) paquet(s) tirés comme dépendances"),
    ("You did not ask for them; they arrive because an upgraded package needs them. {0}{1}",
     "Tu ne les as pas demandés ; ils arrivent parce qu'un paquet mis à jour en a besoin. {0}{1}"),
    ("{0} update(s) available but frozen",
     "{0} mise(s) à jour disponibles mais figées"),
    ("{0}. Held back by IgnorePkg in /etc/pacman.conf: they will not be installed while that line is there.",
     "{0}. Retenues par IgnorePkg dans /etc/pacman.conf : elles ne seront pas installées tant que cette ligne est là."),
    ("{0} extra package(s) will be removed as a cascade",
     "{0} paquet(s) supplémentaires seront retirés en cascade"),
    ("You did not check {0}{1}: they leave because nothing will need them once your selection is gone.",
     "Tu n'as pas coché {0}{1} : ils partent parce que plus rien ne les réclamera une fois ta sélection retirée."),
    ("{0} package(s) still needed by packages that stay: {1}",
     "{0} paquet(s) réclamés par des paquets qui restent : {1}"),
    ("Packages outside this removal depend on them. pacman will refuse the operation, or take along whatever depends on them. Check \"Required by\" in the detail panel before approving.",
     "Des paquets extérieurs à cette suppression en dépendent. pacman refusera l'opération, ou emportera aussi ce qui en dépend. Vérifie « Requis par » dans le détail avant de valider."),
    ("Dependencies loaded at run time are invisible",
     "Les dépendances chargées à l'exécution sont invisibles"),
    ("Qt plugins, Wayland backends, GStreamer modules: nothing declares them, so nothing protects them but keep.list. When in doubt, protect rather than remove.",
     "Greffons Qt, backends Wayland, modules GStreamer : rien ne les déclare, donc rien ne les protège hormis la liste keep.list. En cas de doute, protège plutôt que supprimer."),
    ("Configuration files will be deleted",
     "Les fichiers de configuration seront supprimés"),
    ("The -Rns option also removes dependencies nothing needs any more, along with the package's configuration files. Your own files under ~/ are untouched.",
     "L'option -Rns retire aussi les dépendances devenues inutiles et les fichiers de configuration du paquet. Tes fichiers dans ~/ ne sont pas touchés."),
    ("{0}, and {1} others",
     "{0}, et {1} autres"),
    ("{0} package(s) not found: partial rollback",
     "{0} paquet(s) introuvables : retour partiel"),
    ("These versions are no longer in any local cache and will stay as they are: {0}. They were pruned by paccache, which keeps only {1} version(s) per package.",
     "Ces versions ne sont plus dans aucun cache local et resteront en l'état : {0}. Elles ont été élaguées par paccache, qui ne garde que {1} version(s) par paquet."),
    ("This rollback rebuilds an untested state, not the earlier one",
     "Ce retour reconstruit un état inédit, pas l'état d'avant"),
    ("Fewer than half the packages ({0} of {1}) can go back down. The system would end up with a mix of both versions, never shipped nor tested that way. To really undo a transaction of this size, you need a filesystem snapshot.",
     "Moins de la moitié des paquets ({0} sur {1}) peuvent redescendre. Le système se retrouverait avec un mélange des deux versions, jamais livré ni testé ainsi. Pour revenir vraiment en arrière sur une transaction de cette taille, il faut un instantané du système de fichiers."),
    ("The next full upgrade will undo this rollback",
     "La prochaine mise à jour complète annulera ce retour"),
    ("pacman will offer the recent version again on the next -Syu. To freeze it for good, add IgnorePkg = {0} to /etc/pacman.conf.",
     "pacman reproposera la version récente dès le prochain -Syu. Pour figer durablement, ajoute IgnorePkg = {0} dans /etc/pacman.conf."),
    ("Critical component downgraded: {0}",
     "Composant critique rétrogradé : {0}"),
    ("The whole system is tied to these packages. An earlier version can make pacman itself unusable — keep rescue media within reach.",
     "Tout le système est lié à ces paquets. Une version antérieure peut rendre pacman lui-même inutilisable — garde un support de secours sous la main."),
    ("Partial state: the rest of the system does not go back",
     "État partiel : le reste du système ne redescend pas"),
    ("Only the packages from this transaction go back. If one of them is linked against a library upgraded since, pacman will refuse — or the program will start against a version it does not know.",
     "Seuls les paquets de cette transaction reviennent en arrière. Si l'un d'eux est lié à une bibliothèque mise à jour depuis, pacman refusera — ou le programme se lancera contre une version qu'il ne connaît pas."),
    ("{0} package(s) will be uninstalled",
     "{0} paquet(s) seront désinstallés"),
    ("These are the ones the transaction had installed. Their configuration files under /etc are kept, but everything created since (data, enabled systemd units) stays on disk.",
     "Ce sont ceux que la transaction avait installés. Leurs fichiers de configuration dans /etc sont conservés, mais tout ce qui a été créé depuis (données, unités systemd activées) reste sur le disque."),
    ("{0} warning(s) during the original transaction",
     "{0} avertissement(s) lors de la transaction d'origine"),
    ("Rolling back packages is not rolling back the system",
     "Un retour de paquets n'est pas un retour du système"),
    ("The files the packages laid down go back. What a scriptlet or a hook has written since — a database migration, a rewritten configuration, a regenerated cache — stays as it is. For a real state rollback you need a snapshot (snapper/Btrfs).",
     "Les fichiers posés par les paquets redescendent. Ce qu'un scriptlet ou un hook a écrit depuis — migration de base de données, configuration réécrite, cache régénéré — reste tel quel. Pour un vrai retour d'état, il faut un instantané (snapper/Btrfs)."),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every string passed to `t()` or `tf()` must have a French entry.
    ///
    /// The check reads the source tree rather than a list kept by hand: a list
    /// would drift, and the failure mode it protects against — a French
    /// interface with English sentences scattered through it — is invisible
    /// until someone runs it in French.
    #[test]
    fn every_message_is_translated() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut missing: Vec<String> = Vec::new();
        let known: HashMap<&str, &str> = FR.iter().copied().collect();
        for entry in std::fs::read_dir(dir).expect("src/ is readable").flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // i18n.rs holds the table itself: its own literals are not calls.
            if path.file_name().and_then(|n| n.to_str()) == Some("i18n.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap_or_default();
            for literal in call_sites(&source) {
                if !known.contains_key(literal.as_str()) {
                    missing.push(format!(
                        "{}: {literal}",
                        path.file_name().unwrap().to_string_lossy()
                    ));
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "{} untranslated message(s):\n{}",
            missing.len(),
            missing.join("\n")
        );
    }

    /// Extracts the literal of every `t("…")` / `tf("…"` in a source file,
    /// resolving escapes and line continuations so the result matches what the
    /// compiler will hand to `t()`.
    fn call_sites(source: &str) -> Vec<String> {
        let bytes: Vec<char> = source.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            // A call, not the tail of an identifier such as `format!` or `print`.
            let alone = i == 0 || !(bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_');
            let opens = starts(&bytes, i, "t(") || starts(&bytes, i, "tf(");
            if !opens || !alone {
                i += 1;
                continue;
            }
            // The literal may sit on the next line, as rustfmt writes it.
            let mut j = i + if starts(&bytes, i, "tf(") { 3 } else { 2 };
            while bytes.get(j).is_some_and(|c| c.is_whitespace()) {
                j += 1;
            }
            // `t(match self { … })` selects between several messages: every arm
            // is a call site. Missing them was how a whole enum of labels
            // slipped through untranslated.
            if starts(&bytes, j, "match") {
                let (arms, next) = literals_until_close(&bytes, j);
                out.extend(arms);
                i = next;
                continue;
            }
            if bytes.get(j) != Some(&'"') {
                i += 1;
                continue;
            }
            j += 1;
            let mut litteral = String::new();
            while j < bytes.len() && bytes[j] != '"' {
                if bytes[j] == '\\' {
                    j += 1;
                    match bytes.get(j) {
                        // Line continuation: the newline and the indentation
                        // that follows are not part of the string.
                        Some('\n') => {
                            j += 1;
                            while bytes.get(j).is_some_and(|c| c.is_whitespace()) {
                                j += 1;
                            }
                            continue;
                        }
                        Some('n') => litteral.push('\n'),
                        Some('t') => litteral.push('\t'),
                        Some(c) => litteral.push(*c),
                        None => break,
                    }
                    j += 1;
                    continue;
                }
                litteral.push(bytes[j]);
                j += 1;
            }
            out.push(litteral);
            i = j + 1;
        }
        out
    }

    /// Every string literal inside a parenthesised block, up to its close.
    fn literals_until_close(bytes: &[char], from: usize) -> (Vec<String>, usize) {
        let mut depth = 1usize;
        let mut out = Vec::new();
        let mut i = from;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '"' => {
                    let mut lit = String::new();
                    i += 1;
                    while i < bytes.len() && bytes[i] != '"' {
                        if bytes[i] == '\\' {
                            i += 1;
                        }
                        if let Some(c) = bytes.get(i) {
                            lit.push(*c);
                        }
                        i += 1;
                    }
                    out.push(lit);
                }
                _ => {}
            }
            i += 1;
        }
        (out, i)
    }

    fn starts(bytes: &[char], i: usize, motif: &str) -> bool {
        motif
            .chars()
            .enumerate()
            .all(|(k, c)| bytes.get(i + k) == Some(&c))
    }

    /// Two entries with the same key would make one silently override the
    /// other, and the loser is only noticed by reading the interface in the
    /// other language.
    #[test]
    fn no_key_is_defined_twice() {
        let mut seen: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        let mut clashes = Vec::new();
        for (k, v) in FR {
            if let Some(previous) = seen.insert(k, v) {
                if previous != *v {
                    clashes.push(format!("{k}  →  {previous}  /  {v}"));
                }
            }
        }
        assert!(clashes.is_empty(), "duplicate key(s):\n{}", clashes.join("\n"));
    }

    #[test]
    fn a_context_prefix_never_reaches_the_screen() {
        assert_eq!(t("tab|Installed"), "Installed");
    }

    #[test]
    fn placeholders_are_positional() {
        // English order and French order need not agree; the index decides.
        assert_eq!(tf("{0} of {1}", &["3", "7"]), "3 of 7");
    }

    #[test]
    fn an_untranslated_string_falls_back_to_english() {
        assert_eq!(t("no entry for this one"), "no entry for this one");
    }
}
