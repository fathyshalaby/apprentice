# Watch and learn (macOS, Labs)

Watch and learn lets a person show Apprentice a task once and get a skill
file a coding agent can follow. They record a narrated demonstration (a
lesson), a local model they chose drafts a `SKILL.md` from it, they answer up
to five "why?" questions, approve the skill, and install it into their coding
agents. Plan: [the design](../superpowers/specs/2026-11-22-watch-and-learn-design.md)
and [the implementation plan](../superpowers/plans/2026-11-22-watch-and-learn.md).

This map covers what exists now. The lab and its settings ship first; the
lesson, drafting, store, install, and pane behaviors land in the later tasks
and are listed under Not built yet. Behavior IDs are stable.

```ts
Turn it on                               // apprentice.gate
├── Labs › Watch and learn (labs.watch_and_learn, off by default, macOS only,
│   group Annotate, added 20261122) -> labs::watch_and_learn()
├── Needs Annotate: the DisableAnnotate lock holds it off and the card reads
│   managed (policy::Lock::Annotate); the saved switch is never rewritten
├── Off, locked, or not macOS -> gate reads false where work starts (a lesson
│   start, a draft, an install), not only in the pane
└── Not on Hex's website: labs::web leaves it out like the other
    Annotate-dependent labs, so site/src/labs.json is unchanged

Choose a model                           // apprentice.model
├── hex settings set apprentice.model apple-intelligence | ollama:<name> | none
│   -> ApprenticeSettings::choice() -> ModelChoice
│   ├── "apple-intelligence" -> ModelChoice::AppleIntelligence (text only)
│   ├── "ollama:<name>"      -> ModelChoice::Ollama { model }
│   └── anything else (e.g. "cloud:x") -> no choice; the CLI refuses it
├── Unchosen is the default and is omitted from settings.json
└── Apprentice never picks or switches a model by itself

Point at Ollama                          // apprentice.ollama
├── hex settings set apprentice.ollama_url <url>; default http://127.0.0.1:11434
├── Accepted: http on localhost, 127.0.0.0/8, or [::1], no sign-in, path, or query
│   (app_settings::is_loopback_http_url)
└── Anything else is refused with invalid-value; the request's own resolver
    (net::loopback_agent) refuses a name that resolves off this computer
```

The recommended models, the lesson, evidence, drafting, the skills store, agent
install, and the pane are described in the plan.

## Entry points

- Settings › Labs › Annotate group: the Watch and learn card
  (`rune_ui/labs.rs`, row key `lab-watch-and-learn`; macOS with Annotate).
- `hex labs on|off watch-and-learn`.
- `hex settings set labs.watch_and_learn on`, `apprentice.model`,
  `apprentice.ollama_url`.

## Checks (locators, not observed runs)

- `labs::tests::watch_and_learn_is_its_own_switch_and_macos_only`
- `app_settings::apprentice::tests::apprentice_settings_default_to_no_model_and_local_ollama`,
  `model_strings_parse`, `only_loopback_http_urls_are_allowed`,
  `a_saved_file_without_the_section_loads_the_defaults`
- `control::settings::tests::every_persisted_setting_has_a_cli_key`,
  `settings_keys_name_their_feature`
- `desktop::i18n::tests::every_lab_card_string_has_every_language`
- `labs::web::tests` (the site's lab list is unchanged)

## Not built yet

Lesson recording, evidence and drafting, the skills store, agent install, the
Skills pane, and its events are later tasks in the plan; none of them exists
in this change, and the lab's switch gates nothing yet beyond the Labs card.
