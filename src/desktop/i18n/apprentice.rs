//! Strings for Labs › Watch and learn and its Skills pane, kept in their own
//! table so the French, Portuguese, and Russian tables stay under the source
//! cap. The card's Polish, Chinese, Japanese, German, Spanish, and Ukrainian
//! rows live with the other lab cards in `labs.rs`; the pane's go here.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[];

/// Brazilian Portuguese, for the card and the pane.
#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Watch and learn", "Observar e aprender"),
    ("Show Apprentice a task once and it writes a skill your coding agents can follow", "Mostre uma tarefa ao Apprentice uma vez e ele escreve uma habilidade que seus agentes de código podem seguir"),
];

/// French, for the card and the pane.
#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Watch and learn", "Observer et apprendre"),
    ("Show Apprentice a task once and it writes a skill your coding agents can follow", "Montrez une tâche à Apprentice une fois et il écrit une compétence que vos agents de code peuvent suivre"),
];

/// Russian, for the card and the pane.
#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Watch and learn", "Смотреть и учиться"),
    ("Show Apprentice a task once and it writes a skill your coding agents can follow", "Покажите Apprentice задачу один раз, и он напишет навык, которому смогут следовать ваши агенты для кода"),
];
