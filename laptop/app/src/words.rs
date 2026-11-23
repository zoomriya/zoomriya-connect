//! The app's words, in the laptop's language: English or French, as ZOOMRIYA
//! offers everywhere. Short and natural, never word for word.

use serde::Serialize;

/// Everything the tray and the pairing window say.
#[derive(Debug, Serialize)]
pub struct Words {
    pub connected: &'static str,
    pub looking: &'static str,
    pub not_paired: &'static str,
    pub already_running: &'static str,
    pub open: &'static str,
    pub open_desktop: &'static str,
    pub pair: &'static str,
    pub quit: &'static str,
    pub title: &'static str,
    pub lead: &'static str,
    pub code: &'static str,
    pub pair_button: &'static str,
    pub pairing: &'static str,
    pub paired_lead: &'static str,
    pub forget_button: &'static str,
    pub forget_sure: &'static str,
    pub not_home: &'static str,
    pub wrong_code: &'static str,
    pub unreachable: &'static str,
    pub broken: &'static str,
}

const EN: Words = Words {
    connected: "Connected to your box",
    looking: "Looking for your box…",
    not_paired: "Not paired with a box yet",
    already_running: "ZOOMRIYA is already open on this laptop",
    open: "Open ZOOMRIYA",
    open_desktop: "Open Box Desktop",
    pair: "Pair with a box…",
    quit: "Quit ZOOMRIYA",
    title: "Pair with your box",
    lead: "In Box Desktop, open Control Panel › Devices › Add a laptop, then type the code it shows. This laptop must be on your home network.",
    code: "Code",
    pair_button: "Pair",
    pairing: "Pairing…",
    paired_lead: "This laptop is paired with your box. To pair it with another box, forget this one first, and remove the laptop in Box Desktop too.",
    forget_button: "Forget this box",
    forget_sure: "Forget this box? This laptop stops reaching it until you pair again.",
    not_home: "Pair at home: this laptop must be on the same network as the box.",
    wrong_code: "That code does not match. Check it on the box and type it again.",
    unreachable: "No box found on this network. Check that this laptop is on your home network, and that the box is on.",
    broken: "Pairing did not work. Try again.",
};

const FR: Words = Words {
    connected: "Connecté à votre boîtier",
    looking: "Recherche de votre boîtier…",
    not_paired: "Pas encore associé à un boîtier",
    already_running: "ZOOMRIYA est déjà ouvert sur cet ordinateur",
    open: "Ouvrir ZOOMRIYA",
    open_desktop: "Ouvrir Box Desktop",
    pair: "Associer un boîtier…",
    quit: "Quitter ZOOMRIYA",
    title: "Associer votre boîtier",
    lead: "Dans Box Desktop, ouvrez Panneau de configuration › Appareils › Ajouter un ordinateur, puis saisissez le code affiché. Cet ordinateur doit être sur le réseau de la maison.",
    code: "Code",
    pair_button: "Associer",
    pairing: "Association…",
    paired_lead: "Cet ordinateur est associé à votre boîtier. Pour l'associer à un autre, oubliez d'abord celui-ci, et retirez aussi l'ordinateur dans Box Desktop.",
    forget_button: "Oublier ce boîtier",
    forget_sure: "Oublier ce boîtier ? Cet ordinateur n'y accède plus jusqu'à une nouvelle association.",
    not_home: "Associez à la maison : cet ordinateur doit être sur le même réseau que le boîtier.",
    wrong_code: "Ce code ne correspond pas. Vérifiez-le sur le boîtier et saisissez-le à nouveau.",
    unreachable: "Aucun boîtier trouvé sur ce réseau. Vérifiez que cet ordinateur est sur le réseau de la maison et que le boîtier est allumé.",
    broken: "L'association n'a pas marché. Réessayez.",
};

/// The words for the laptop's language, from the usual settings.
pub fn for_this_laptop() -> &'static Words {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .unwrap_or_default();
    if lang.starts_with("fr") { &FR } else { &EN }
}
