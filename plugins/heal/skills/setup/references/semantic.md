# Semantic family: enable, key, concepts

Loaded by `/heal:setup` for the semantic step. `[features.semantic]`
asks TypeSafe's Jev classifier typed questions about the code, tests,
and docs, and caches the answers under `.heal/semantic/verdicts/`.
It is the only heal feature that sends project content over the
network, and it costs money per run, so every part below waits for
the user's explicit yes.

## Part 1 — Enable

Ask once with `AskUserQuestion`, stating plainly that `heal semantic
ask` sends the selected code and prose to the TypeSafe API and is
billed per request, and that every other heal command stays offline.
On a yes, set `enabled = true` under `[features.semantic]` in
`.heal/config.toml` (keep the shipped pinned `model` and `max_usd`
unless the user names others; moving aliases such as `jev-latest` are
rejected by the loader).

## Part 2 — API key

The key never goes into the chat, the config, or anything under
`.heal/`. When doctor reports no `key_source`, ask the user to run
one of these themselves:

```sh
heal auth jev set            # reads the key from stdin into the per-user credentials file
export TYPESAFE_API_KEY=…    # or provide it through the environment
```

Then `heal auth jev status --json` confirms it against the API (one
network call; exit 2 means the key or model was rejected).

## Part 3 — Concept vocabulary (`.heal/concepts.toml`)

Writes `.heal/concepts.toml`: the list of concepts the codebase is
built from. HEAL's `concept`, `term_drift`, and `doc_concept` tasks
ask Jev to classify every function and doc section into one of these
concepts. Jev can only choose among names it is given, so the quality
of this list decides the quality of every split, move, rename, and
doc proposal that follows. This skill is read-only on the codebase;
the only file it writes is `.heal/concepts.toml`.

### What makes a good concept

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

### Steps

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

### Don'ts

- Don't invent concepts for code you have not looked at.
- Don't encode file paths or directory names as concepts; the point of
  the map is to find where files and concepts disagree.
- Don't edit source files, `config.toml`, or anything else under `.heal/`.

## Part 4 — First run

Price everything the enabled tasks would ask with
`heal semantic ask --dry-run --json` and report the subject counts and
estimated cost per task. Do not run the paid `heal semantic ask`
yourself unless the user explicitly asks for it in this session; the
verdicts it writes under `.heal/semantic/verdicts/` are tracked, so
remind the user to commit them along with `.heal/concepts.toml`.
