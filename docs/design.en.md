# dost: design (as built in 0.1)

An English summary of [design.md](design.md) (the original, in Russian), updated to what
version 0.1 actually does.

## Problem
Working with Ansible fails on memory, not on Ansible: many playbooks, an order between them,
forgotten purposes, recurring errors whose fix is forgotten, and no plain statement of what is
true after a run. `dost` is a terminal player that keeps this knowledge next to the playbooks.

## Goals
- Menu instead of flags; check first, apply after confirmation.
- Live scenes that never lie: a preview (check run) is visibly marked.
- A catalog in the user's project, not in the tool; `dost init` for a quick start.
- One light Rust binary, macOS and Linux, on top of the regular `ansible-playbook`.

## Non-goals
Not a replacement for Ansible, not a server or runner, not a monitoring system, not an IaC tool.

## Anti-goals
A picture that shows something untrue; the sudo password anywhere but memory and an anonymous
pipe; ties to one infrastructure; having to rewrite playbooks just to run them.

## Architecture
```
 project (.dost/)                      dost
   catalog/*.yml  ─────────────►   loader
   scenes/*.yml   ─────────────►   runner ── spawns ansible-playbook + bundled callback
   errors.yml     ─────────────►   stream (JSON lines) ──► RunState ──► scenes (ratatui)
   state/ history/ (local)         summary, hints, history
```
- **Catalog**: one YAML per playbook (title, about, danger, needs_become, scene, after,
  done, inventory, limit, tags, extra_vars, check, watch), chains, known errors, scenes, theme.
- **Callback**: `assets/dost_stream.py`, embedded in the binary, enabled with
  `ANSIBLE_CALLBACK_PLUGINS` and `ANSIBLE_STDOUT_CALLBACK`; the run mode arrives as `DOST_MODE`.
  It prints one JSON object per line (start, task, result, event, recap). Results of `no_log`
  tasks carry no data.
- **Events (v1)**: `debug` tasks with a `dost` mapping; see [events.md](events.md).
- **Scenes**: kinds `chain`, `table`, `map`, `results`; built from widgets (chain, table, map,
  graph, progress); bindings use dotted paths with `[*]`, evaluated per host; see
  [scenes.md](scenes.md).
- **Password**: typed into a masked field, kept in memory for one command (check and apply),
  passed to Ansible as `--become-password-file /dev/fd/N` over an anonymous pipe, wiped on drop.
- **State**: chain progress is local memory, not truth about servers; a step whose playbook (or
  `watch` files) changed since the run shows "may be stale". History stores statuses only.

## Flow
menu → preflight (command line shown) → [password once] → check (`--check --diff`) →
passed: confirm apply (typed id for `danger: medium|high`) → apply → result: the `done`
message, hints on failure, next chain step. A failed check offers `a` = apply anyway, which
always needs the typed id. `check: false` skips the check with a visible warning.

Without a terminal: `dost run <id>` prints plain lines and runs the check; `--apply` applies;
`danger: high` also needs `--confirm <id>`; the sudo password is read from the first line of stdin.

## Limits of 0.1
Windows is not supported; ssh and vault passwords are not prompted (use keys and
`vault_password_file`); one `become` password per run; the UI is English; the `animation`
field is accepted but not rendered; the live terminal loop and the terminal-restore guard are
not covered by automated tests.
