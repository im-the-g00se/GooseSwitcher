# System dictionaries and recognition settings

## Dictionary source

GooseSwitcher reads the dictionaries installed by Fedora instead of copying
word-list data into this repository:

| Language | Fedora package | Hunspell files |
| --- | --- | --- |
| Russian | `hunspell-ru` | `/usr/share/hunspell/ru_RU.aff`, `/usr/share/hunspell/ru_RU.dic` |
| US English | `hunspell-en-US` | `/usr/share/hunspell/en_US.aff`, `/usr/share/hunspell/en_US.dic` |

Install them with:

```shell
sudo dnf install hunspell-ru hunspell-en-US
```

These packages provide a reproducible source through Fedora's signed
repositories and source RPMs. Fedora identifies the Russian dictionary as the
Ispell/Hunspell word list by Alexander I. Lebedev under a modified BSD license.
The English dictionary is generated from SCOWL and carries its upstream
permissive/LGPL notices. The files stay system-owned and are not redistributed
as part of GooseSwitcher. Their licenses and the MPL-2.0 `spellbook` parser are
compatible with the GPL-3.0 license in this repository's `LICENSE` file.

Fedora's current Russian dictionary declares KOI8-R while the English one uses
UTF-8. The loader supports both encodings and passes decoded Hunspell affix
rules and stems to `spellbook`; it does not reduce a Hunspell dictionary to a
list of stems.

## Loading and failure behavior

`RecognitionRuntime::load` reads and indexes both dictionary pairs exactly once
at engine startup. It also reads all user rules and the two recognition settings
from `SqliteStore` at that time. `RecognitionRuntime::decide` subsequently uses
only the in-memory dictionaries, rule hash map, and configuration snapshot, so
per-word checks do not query SQLite or reread dictionary files.

An absent, unreadable, unsupported, or malformed dictionary produces a startup
error that names the affected path or locale. An explicitly empty `.dic` file
(empty content or a zero entry count) creates an empty index; words for that
language are then unknown and cannot trigger a dictionary-backed replacement.

Restart the engine after changing settings or user rules, or after a dictionary
package update, so it creates a fresh snapshot.

## Updating dictionaries

Normal Fedora updates replace the system files:

```shell
sudo dnf upgrade hunspell-ru hunspell-en-US
```

The package changelogs, exact installed versions, licenses, and source RPM names
can be inspected reproducibly with:

```shell
rpm -q --changelog hunspell-ru hunspell-en-US
dnf repoquery --info hunspell-ru hunspell-en-US
```

No generated word list is committed to GooseSwitcher when these packages are
updated.

## Persisted settings

The existing SQLite `settings` table stores serialized key/value pairs. The
runtime recognizes:

| Key | Values | Default | Meaning |
| --- | --- | --- | --- |
| `corrections_enabled` | `true`, `false` | `true` | Enables or disables every automatic replacement, including `always_replace` user rules. |
| `minimum_word_length` | a non-negative integer | `3` | Minimum number of letters required for dictionary-backed correction. Joiners such as hyphens and apostrophes are not counted. |

Invalid serialized values produce a startup error instead of silently changing
behavior. No GTK dependency or settings window is introduced at this stage;
the existing `SqliteStore::set_setting` API remains the write boundary for a
future settings application.
