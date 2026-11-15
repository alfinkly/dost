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

Other catalog fields: `after`, `inventory`, `limit`, `tags`, `extra_vars`, `check: false`
(skip the check, with a visible warning), `watch: [globs]` (files whose change marks a chain
step "may be stale").

State (`.dost/state/`) and run history (`.dost/history/`) stay local; `dost init` adds them to
`.dost/.gitignore`. History stores statuses only, never messages, diffs or passwords.
