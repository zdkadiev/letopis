# Participant instructions (draft, wording not frozen)

These instructions are given to the participant at the start of every session, together
with exactly one task prompt. The wording is a draft until the prompt wording freeze is
recorded in the preparation manifest. Each session is a new conversation: nothing from an
earlier session is available except the files in your working repository.

## Common to every session

- Work in the repository you are given. Read `AGENTS.md` first and follow it; it describes
  the architecture, coding rules and the verification commands.
- Implement the task you are given as ordinary product code, tests and documentation in the
  repository. Do not ask for clarification: decide, and note assumptions in your final answer.
- Run `make check` before you finish and report the exact result. Do not change toolchain
  pins, dependency versions or lock files unless the task requires it.
- Do not use the network, other directories on this machine, or any other copy of the
  project. Your final answer should summarise what you changed and what you verified.

## Baseline condition

You have no persistent memory between sessions. Anything you want to keep must stay in the
repository files you are allowed to change for the task itself.

## Memory condition

You also have the `letopis` command-line tool and its skill. Use them as you see fit to keep
your own knowledge about this project between sessions as Markdown files in a memory
directory of your choice (its structure, and what to save or search, is your decision).
Start with an empty memory directory; do not expect anything to be there from before. The
skill text provided in your session describes how the tool is used.
