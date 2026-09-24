---
name: heal-concepts-setup
description: Read the codebase, glossary, and docs, then propose a concept vocabulary (20–80 concepts, each with a one-line responsibility) and write `.heal/concepts.toml` for the `[features.semantic]` concept tasks. The vocabulary lets HEAL map every function and doc section to a concept and flag mixed files, misplaced functions, scattered concepts, term drift, and duplicated or missing docs. Writes only `.heal/concepts.toml`. Trigger on "set up concepts", "create concepts.toml", "define the concept vocabulary", "map the codebase's concepts", "/heal-concepts-setup".
---

# heal-concepts-setup

Writes `.heal/concepts.toml`: the list of concepts the codebase is
built from. HEAL's `concept`, `term_drift`, and `doc_concept` tasks
ask Jev to classify every function and doc section into one of these
concepts. Jev can only choose among names it is given, so the quality
of this list decides the quality of every split, move, rename, and
doc proposal that follows. This skill is read-only on the codebase;
the only file it writes is `.heal/concepts.toml`.

## When this skill is right

- `[features.semantic]` was just enabled and `heal semantic ask
  --dry-run` shows `concept` with 0 subjects.
- The codebase grew a new area (a new module, service, or domain
  object) that the current vocabulary has no name for.
- Concept findings keep landing on `other`, or two concepts are
  always confused with each other.

## Output language

Write the conversation in the user's language (explicit instruction,
then the chat language, then `[project].response_language`, then
English). Concept **ids** and **descriptions** in the file are
written in English, like source comments: they are sent to the model
and read by every teammate.

## What makes a good concept

- **A responsibility, not a word.** `calibration` — "Derives
  codebase-relative thresholds from the project's own metric
  distribution." Not `utils`, not `helpers`, not a file name.
- **Mutually distinct.** If two descriptions could both describe the
  same function, merge them or sharpen the boundary in the text.
- **The team's own vocabulary.** Prefer the terms in the glossary,
  `README`, docs headings, and the most common identifier nouns. When
  the codebase has a glossary (for example `.claude/docs/glossary.md`
  or `docs/glossary.md`), every canonical term there is a candidate.
- **20–80 concepts.** Fewer and every file looks "mixed"; more and the
  model splits probability across near-synonyms. At most 254 fit in one
  question. HEAL adds `other` automatically.
- **Ids** are lowercase ASCII with `_` or `-` (`change_coupling`).

## Steps

1. **Pre-flight.** Confirm `[features.semantic] enabled = true` in
   `.heal/config.toml`. If it is off, explain that the vocabulary has
   no consumer until the family is enabled and ask whether to continue.
   If `.heal/concepts.toml` exists, read it: keep every existing id
   unless the user agrees to rename it (renaming an id re-asks every
   function classified under it).
2. **Survey.** Run `heal metrics --json` for the module layout and
   LOC by directory. Read the glossary and docs headings if present.
   Skim the largest and most-changed source files (`heal status --json`
   lists hotspots) for the nouns their functions revolve around.
3. **Draft.** Write candidate concepts with one-sentence descriptions
   of what code in that concept is *responsible for*. Group the draft
   by area so the user can scan it.
4. **Review with the user.** Show the draft and ask for changes before
   writing. Point out concepts you were unsure how to separate.
5. **Write** `.heal/concepts.toml`:

   ```toml
   [[concept]]
   id = "calibration"
   description = "Derives codebase-relative thresholds from the project's own metric distribution."

   [[concept]]
   id = "drain_queue"
   description = "Orders findings into the work queue the patch skills consume."
   ```

6. **Price the next step.** Run `heal semantic ask --task concept
   --dry-run` and report the subject count and estimated cost. Do not
   run the paid command yourself; the user decides when to spend.
7. **Remind** the user to commit `.heal/concepts.toml`: it is part of
   the team contract, like `config.toml`.

## Don'ts

- Don't invent concepts for code you have not looked at.
- Don't encode file paths or directory names as concepts; the point of
  the map is to find where files and concepts disagree.
- Don't edit source files, `config.toml`, or anything else under `.heal/`.
