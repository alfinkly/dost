# Scenes

A scene is a YAML file in `.dost/scenes/<id>.yml`. A scene with the id of a built-in one
(`chain`, `ports`, `vpn`, `config`) replaces it. A command picks its scene with `scene: <id>`.

```yaml
id: firewall
use: table                      # chain | table | map | results | tree | report | matrix | transfer
title: Firewall
bind:
  columns: [port, was, now]
  rows: event.ports.state       # path to an object {key: value}
  was: event.ports.was          # optional, enables the was/now columns
```

## Kinds
- `chain`: the tasks of the run in order with a status each. No binding needed.
  With `bind.steps` it becomes a **fixed chain** of named links drawn top-down with connectors:
  ```yaml
  id: gpu
  use: chain
  bind:
    steps:
      - { id: host,  label: opkit-hero }
      - { id: ports, label: Ports }
      - { id: gpu,   label: GPU host }
  ```
  The playbook drives it with `step` events (key = step id, `state: running|ok|failed`, optional
  `detail` text shown dim next to the link). A running link shows a spinner, links not reached yet are
  `○`, and a link still running when the run ends is shown as failed.
- `table`: one table per host. `rows` (required) and `was` (optional) are paths; `columns`
  are header names. A cell that differs from `was` is highlighted.
- `map`: hosts in a row joined by links. `links` (default `event.link[*]`) is a path to link
  objects `{a, b, state}`.
- `results`: the tasks that changed something.
- `report`: **ordinary Ansible, prettier.** No events needed: it reads the task and loop-item
  results of the run and sorts them into titled sections with rules, plus a header of the plays
  (stages) with a spinner on the one running. Rows say what happened: created / already there /
  failed, per loop item (users, peers, packages).
  ```yaml
  id: nodeup
  use: report
  bind:
    stages:
      - { play: "^Admin users", label: Users }
    sections:
      - id: users
        title: Users
        play: "^Admin users"          # optional: the section title spins while this play runs
        rules:
          - task: "^Admin accounts$"  # regex on the task name
            label: "{item}"            # {item} is the loop item label, {host}, {task} also work
            changed: created           # words per outcome: changed / ok / failed / skipped
            ok: already there
            failed: not created
  ```
  A rule that matches loop items makes one row per item, otherwise one row per task result. With
  `focus: [host, ...]` in the catalog only those hosts' rows are shown (the others are hidden, which
  matters for plays that run on every node); when several hosts remain, rows get a `host:` prefix.
- `matrix`: **who got what.** The things a loop walks over (people, hosts) are the rows, the steps
  of the run are the columns, one cell per row and step. No events needed: it reads loop-item results.
  A cell is `+` changed, `✓` already there, `✗` failed, `–` skipped, `·` nothing reported; when the task
  ran on several hosts it adds `n/m` (hosts in that state out of all hosts the task touched). The last
  column sums the row up. `missing` on a column names every row that has no result in it although the
  column's task has run, and is printed under the table as `! row: text`. `stages` is the same header of
  plays as in `report`; `row_title` is the header of the first column.
  ```yaml
  id: access
  use: matrix
  bind:
    row_title: Person
    stages:
      - { play: "^Accounts", label: Accounts }
    columns:
      - { label: Account, task: "^People accounts$" }
      - { label: Key, task: "^People SSH keys", missing: "no SSH key in GitLab" }
  ```
- `transfer`: things moving from one place to another, one row per `transfer` event, in the order
  they first arrived: `label  from  ──▶  to  detail`. A running row without a total shows a packet
  crossing the arrow; with `done` and `total` (bytes) it shows a filled bar and `120.0 MB / 480.0 MB`;
  a row that just finished is still animated for a moment and then settles into a solid ✓ arrow; a
  `failed`, `missing` or `refused` row ends in a broken arrow. Optional `bind.titles: [FROM, TO]` names the
  two columns, and `bind.strip: ["kv/opkit/"]` removes the listed substrings from `from` and `to`
  (long names are otherwise cut with an ellipsis to fit the width).
  ```yaml
  id: copy
  use: transfer
  bind: { titles: [PROD, PREPROD], strip: ["kv/opkit/"] }
  ```
- `tree`: a fixed hierarchy drawn top-down, the same picture for every playbook that uses it.
  The structure is yours, in `bind.nodes` (`id`, `label`, optional `parent`; ids are unique, parents must exist, no cycles).
  A command declares which nodes it works on with `focus: [id, ...]` in the catalog: those nodes
  spin while the run lasts and become ✓ or ✗ when it ends. A `node` event
  (`key` = node id, `state: up|down|running|pending`) sets any node's status and wins over `focus`.
  Nodes nothing is known about are shown as `·`.

  ```yaml
  id: secrets
  use: tree
  bind:
    nodes:
      - { id: bao,   label: OpenBao }
      - { id: hero,  label: opkit-hero, parent: bao }
      - { id: agent, label: "bao-agent@stage", parent: hero }
  ```

## Binding paths
Evaluated per host against `{ host, event: { <kind>: <payload> } }`. A keyed event is nested
under its key: `event.link.<key>`.

- dotted keys: `event.ports.state`
- `[*]` visits every value of an object or element of an array: `event.link[*]`
- nothing else (no indexes, filters or expressions)
- a missing key gives an empty result, not an error; a malformed path is an error at load
  (`dost doctor` shows it)

## Themes
`.dost/theme.yml`:
```yaml
colors: { ok: green, fail: red, running: yellow, pending: darkgray, accent: cyan }
```
Names or `#rrggbb`; unknown values keep the default.

## Small terminals
A widget that does not fit its area draws a plain list instead; more than 50 nodes are always
a list. The app itself asks for at least 80×24.
