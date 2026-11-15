# Scenes

A scene is a YAML file in `.dost/scenes/<id>.yml`. A scene with the id of a built-in one
(`chain`, `ports`, `vpn`, `config`) replaces it. A command picks its scene with `scene: <id>`.

```yaml
id: firewall
use: table                      # chain | table | map | results | tree
title: Firewall
bind:
  columns: [port, was, now]
  rows: event.ports.state       # path to an object {key: value}
  was: event.ports.was          # optional, enables the was/now columns
```

## Kinds
- `chain`: the tasks of the run in order with a status each. No binding needed.
- `table`: one table per host. `rows` (required) and `was` (optional) are paths; `columns`
  are header names. A cell that differs from `was` is highlighted.
- `map`: hosts in a row joined by links. `links` (default `event.link[*]`) is a path to link
  objects `{a, b, state}`.
- `results`: the tasks that changed something.
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
