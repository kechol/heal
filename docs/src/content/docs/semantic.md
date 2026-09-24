---
title: Semantic (Jev)
description: Opt-in judgments from TypeSafe's Jev classifier — what gets sent, how to set up the API key, and how the answers are cached.
---

HEAL's metrics are computed locally from your source and git history.
They tell you **where** code is hard to change, but not **what it
means**: whether a function's name matches what it does, whether a
test actually checks anything, whether a doc still describes the code.

The optional `[features.semantic]` family asks those questions to
[Jev](https://docs.typesafe.ai/), a classifier from TypeSafe. Jev does
not write text or code. It answers each question with a probability,
so heal can turn the answers into findings the same way it does for
every other metric.

Everything in this family is **off by default**. If you never enable
it, heal behaves exactly as before.

## What gets sent, and when

- Only `heal semantic ask` sends anything. It sends the pieces of
  code, tests, or documentation that heal has already selected (for
  example, functions in files that change often), to the TypeSafe API.
- `heal auth jev status` checks that your API key works. It sends the
  key, but no project content.
- Every other command — `heal status`, `heal metrics`, `heal diff`,
  and the post-commit hook — never connects to the network. They read
  the answers `heal semantic ask` saved earlier.
- Files matching `[features.semantic].exclude` are never sent.

TypeSafe states that it does not train models on customer data. Zero
data retention is available on enterprise plans only. Read the
[TypeSafe legal pages](https://docs.typesafe.ai/legal.md) before
enabling this on a private codebase.

## Enable it

Turning the family on is a team decision, so it lives in the shared
config:

```toml
[features.semantic]
enabled = true
# model = "jev-1.13.0"   # pinned version; moving aliases are rejected
# max_usd = 1.0          # stop a single run before it spends more
# concurrency = 8        # requests in flight at once
# exclude = ["secrets/", "*.pem"]
```

Once enabled, `heal status` also uses the saved answers to order the
TODO list, and the bundled patch skills ask Jev to double-check their
own changes.

## Set up the API key

Each person uses their own key. Get one from the
[TypeSafe console](https://console.typesafe.ai/), then either export it:

```sh
export TYPESAFE_API_KEY=...
```

or store it in your user config (outside the project, readable only by
you):

```sh
printf '%s\n' "$KEY" | heal auth jev set
heal auth jev status    # shows where the key comes from and checks it
heal auth jev clear     # removes the stored key
```

The key is never written under `.heal/`. `TYPESAFE_API_KEY` wins over
the stored file when both are present. `TYPESAFEAI_API_KEY` is also
accepted, so a key you already use for other Jev tools works as is.

## Ask

```sh
heal semantic ask --dry-run   # what would be asked, and the estimated price
heal semantic ask             # ask, and save the answers
heal semantic ask --prune     # also remove answers nothing refers to anymore
```

Answers are saved in `.heal/semantic/verdicts/`, one file per task.
Commit that directory: teammates then see the same results without an
API key, and heal only asks about code that changed since the last run.
If two branches both add answers, you can reduce merge conflicts with:

```text
# .gitattributes
.heal/semantic/verdicts/*.jsonl merge=union
```

Answers from the on-demand checks below and from `focus` describe one
person's work in progress, so they are kept in
`.heal/cache/semantic/verdicts/` instead. That directory is ignored by
git and safe to delete.

## What HEAL asks

Each kind of question is a _task_. You can turn one off with
`[features.semantic.tasks.<id>] enabled = false`, or change the
probability it needs with `cutoff = 0.7`.

| Task                 | What it asks                                                                                                                  | Where you see it                                                                                 |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `commit_intent`      | Whether each recent commit was a bug fix, a feature, a refactor, and so on                                                    | Files where bug fixes concentrate move up the `heal status` list                                 |
| `consequence`        | What a defect in each flagged file would cost, from dev tooling to data integrity                                             | Critical code moves up the `heal status` list                                                    |
| `triage`             | Whether a drain-queue finding is a mechanical fix, a false positive, or needs a design decision, and how big the fix is       | Guides the patch skills; smaller fixes come first                                                |
| `friction`           | Whether complex code is hard to change, to test, or to read, and whether the complexity is intrinsic                          | Code that really hurts moves up                                                                  |
| `focus`              | How much a task you describe will touch each file (`--focus`)                                                                 | `heal status --focus` puts the files to prepare first on top                                     |
| `concept`            | Which concept of your vocabulary each function implements (inline unit tests are skipped)                                     | Files that mix concepts, functions that belong elsewhere, and concepts scattered over many files |
| `term_drift`         | Whether two words in function names mean the same thing (`user` / `account`)                                                  | One word per thing — rename suggestions                                                          |
| `name_mismatch`      | Whether a function's name or doc comment matches what its body does                                                           | Names that promise something the code does not do                                                |
| `split_points`       | Where a long, complex function divides into steps with one purpose each                                                       | Split lines on complexity findings, used by `/heal-code-patch`                                   |
| `fix_pattern`        | Which standard refactoring fits a complexity or duplication finding                                                           | Guides `/heal-code-patch`                                                                        |
| `test_value`         | Whether each test would catch the behaviour its name claims breaking, or only checks its own mocks, the framework, or nothing | Tests to delete or rewrite (needs `[features.test]`)                                             |
| `mock_scope`         | What each mock replaces: an outside service, the code under test, or an internal part                                         | Mocks that tie tests to implementation details                                                   |
| `test_triage`        | Whether uncovered code is plain logic, coordination, or I/O; why skipped tests are skipped                                    | Tells `/heal-test-review` where a unit test pays off                                             |
| `test_duplicate`     | Whether two similar tests check the same thing, or differ only in inputs                                                      | Tests to delete or merge into one table-driven test                                              |
| `doc_structure`      | What kind of document each section is (tutorial, how-to, reference, explanation, …) and where a new document starts           | Pages to split, merge, or keep to one mode (needs `[features.docs]`)                             |
| `doc_placement`      | Which section of your docs a reader would look in for each page                                                               | Pages filed in the wrong place, and where to link orphan pages                                   |
| `doc_drift_semantic` | Whether a paired doc section still describes what the code does                                                               | Sections that state something the code no longer does                                            |
| `doc_concept`        | Which concept of your vocabulary each doc section explains                                                                    | Concepts the code relies on that no doc explains                                                 |
| `doc_overlap`        | Whether two pages explain the same concept twice, or contradict each other                                                    | Explanations to merge, and contradictions to fix                                                 |

### Order of the TODO list

With answers saved, `heal status` still groups findings by tier and
severity exactly as before. Inside each group, it then prefers files
where a defect costs more, code that is hard to work with, files where
bug fixes keep landing, and cheaper fixes — before falling back to the
usual hotspot score. Without saved answers the order is unchanged. The
bundled patch skills, and your scripts, read the same order from
`heal status --json`: every finding in a queue carries `drain_rank`
(1 is next, counted per family) and `drain_tier`.

To prepare for a specific piece of work, describe it in a file and run:

```sh
heal semantic ask --task focus --focus plan.md
heal status --focus plan.md
```

### The concept vocabulary

The `concept` task needs a list of the ideas your code is built from,
in `.heal/concepts.toml`. Jev chooses among names you give it; it never
invents one. Generate a first draft with the bundled skill, review it,
and commit the file:

```sh
claude /heal-concepts-setup
```

```toml
[[concept]]
id = "calibration"
description = "Derives thresholds from the project's own metric distribution."
```

The patch and review skills also run on-demand checks
(`verify_patch`, `verify_tests`, `verify_proposal`, `name_choice`) with
`--task`. `verify_patch` and `verify_tests` judge the commits given with
`--diff` (for example `--diff HEAD~1..HEAD`). These checks double-check
the agent's own work and are never part of a plain `heal semantic ask`.

## Cost

Jev charges for input only ($42 per billion input tokens at the time of
writing). `--dry-run` prints an estimate before anything is sent, and
`max_usd` stops a run before it crosses the limit. A run over a
medium-sized repository typically costs a few cents.

## Results are candidates, not verdicts

A classifier is sometimes wrong, and a score close to a threshold can
change slightly between model versions. HEAL pins the model version and
reuses saved answers so results stay stable, but treat each semantic
finding as something for a person to look at, not an instruction to
follow blindly.

## Turn it off

Set `enabled = false`. The next `heal status` drops every semantic
finding and returns to the normal order. The saved answers stay in
`.heal/semantic/` until you delete them.
