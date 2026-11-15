# Quickstart

## Requirements
- macOS or Linux
- ansible-core 2.15 or newer (`ansible --version`)
- Rust (stable) to build

## Build and check
```bash
./install.sh        # puts `dost` into ~/.local/bin (PREFIX=... to change)
dost doctor
```
`doctor` reports the Ansible version, whether the bundled callback loads, and (inside a
project) every problem in your catalog with file and field.

## See the scenes
```bash
dost demo
```
Keys: `↑↓` choose, `Enter` open or confirm, `Tab` switch scene / log / raw output,
`q` or `Esc` cancel or go back.

## Your project
1. `cd` to a directory that has `playbooks/`.
2. `dost init` writes one stub per playbook into `.dost/catalog/`.
3. Edit a stub:
   ```yaml
   id: perimeter
   title: Perimeter
   playbook: playbooks/perimeter.yml
   about: Closes every port except SSH.
   danger: medium            # low | medium | high
   needs_become: true        # asks for the sudo password once
   scene: ports              # see docs/scenes.md
   done: |
     Perimeter closed on {{ hosts | join(", ") }}.
   ```
4. Optional: chains in `.dost/catalog/chains/<id>.yml` (`steps: [base, wg, perimeter]`) and
   known errors in `.dost/errors.yml`:
   ```yaml
   - match: "Too many authentication failures"
     cause: The provider limits new SSH connections.
     fix: Enable ControlMaster in your ssh config and run again.
   ```
5. Make a playbook draw something: emit events, see [events.md](events.md).

Other catalog fields: `focus` (nodes of a `tree` scene the command works on), `after`, `inventory`, `limit`, `tags`, `extra_vars`, `check: false`
(skip the check, with a visible warning), `watch: [globs]` (files whose change marks a chain
step "may be stale").

State (`.dost/state/`) and run history (`.dost/history/`) stay local; `dost init` adds them to
`.dost/.gitignore`. History stores statuses only, never messages, diffs or passwords.

## When a run fails

Keys on the failure screens: `r` retries the same step (a check or an apply), `p` retries and asks for
the sudo password again. If sudo says the password is wrong or missing, `dost` forgets that password
at once, so `r` already asks for a new one. A failed apply can be retried without typing the id again.
Passwords with spaces at either end are refused (Ansible would strip them).

## Ask Claude (headless help)

On a failure screen press `h`. `dost` shows exactly what would be sent (the failure lines, the last output
lines, the playbook path and the catalog command ids; values that look like passwords, tokens or keys
are hidden) and asks once per session; `y` sends it.

It runs `claude -p` in the project directory, **read-only**: the tools are limited to Read, Grep and Glob,
nothing can be run or edited, there is a small budget cap and the session is not saved. The answer is
structured: a summary, the likely cause and up to three proposals:

- **patch**: a unified diff against files of this project. `a` applies it (it is validated first: only
  files inside the project, no deletes, no `.git`; it must apply cleanly or nothing changes). Then `r`
  retries the run, `u` undoes the patch.
- **steps**: numbered steps. `Enter` on a step that names a catalog command opens that command in
  `dost`; `c` copies a step's command (an `ansible-playbook` line or a read-only check) to the clipboard.
  `dost` never runs arbitrary shell from an answer: servers change only through Ansible.

Optional `.dost/config.yml`:
```yaml
assistant:
  command: claude          # path of the Claude Code CLI
  model: sonnet            # optional
  tools: [Read, Grep, Glob]
  max_budget_usd: 0.5
  timeout_secs: 180
```
It needs the Claude Code CLI signed in (run `claude` once in a terminal). If it is not, `dost` says so.
