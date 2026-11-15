# Events: how playbooks draw scenes

A playbook reports what happened with an ordinary `debug` task. No plugin or module is
needed in your playbook; `dost` enables its own callback when it runs Ansible.

```yaml
- name: Report the port state
  ansible.builtin.debug:
    msg:
      dost:
        v: 1
        kind: ports
        state: { "22": "on", "80": "off" }
        was:   { "22": "on", "80": "on" }
```

Quote `"on"` and `"off"`: unquoted they are booleans in YAML 1.1.

## Contract (version 1)
- The message is a mapping with a `dost` key. `v` must be `1`; other versions are ignored.
- `kind` names the event. Unknown kinds are kept but no built-in scene draws them.
- Optional `key` lets one host send several events of the same kind (they are stored by
  host, kind and key; the same key overwrites, different keys accumulate).
- Every other field is the payload (`data`).
- The host comes from Ansible, do not write it into the event.
- Tasks with `no_log: true` never emit events (`dost doctor` warns about it).
- In a `loop`, each item emits its own event.

## Kinds used by the built-in scenes
| kind | payload | scene |
|---|---|---|
| `ports` | `state: {port: on/off}`, optional `was: {port: on/off}` | `ports` (table per host, was/now, changes highlighted) |
| `transfer` | `from`, `to`, `state: pending/running/done/failed/missing/refused`, optional `label`, `done`, `total` (bytes), `detail`; `key` is the row id | `transfer` (one row per key, bar and MB when `total` is known) |
| `step` | `state: running/ok/failed`, optional `detail`; `key` is a step id of a `chain` scene with `bind.steps` | `chain` with steps (one link per step, spinner on the running one) |
| `node` | `state: up/down/running/pending`; `key` is the id of a node of a `tree` scene | `tree` (status of any node; no need to send it for nodes in `focus`) |
| `link` | `a`, `b`, `state: up/down` (use `key` to send several) | `vpn` (map of hosts; a down link shows a break marker, hosts named only in links show as not joined) |

The `chain` scene (task progress) and the `config` scene (tasks that changed something, with
the first line of the diff when there is one) need no events; they read Ansible's task results.

## Check runs
During `--check` the same events arrive. `dost` draws them dimmed with the label
"ПРОВЕРКА, предварительно": they describe what would happen, not what is.

## Progress from long scripts (side channel)

A single Ansible task shows nothing until it ends, so a script that runs for minutes can report
live through a file. `dost` sets `DOST_PROGRESS_FILE` for the run and follows it: append one JSON
object per line, `{"host": "laptop", "dost": {"v": 1, "kind": "transfer", "key": "files", "state": "running", "done": 1048576, "total": 8388608}}`
(`host` defaults to `local`), and it arrives as an ordinary event while the script is still running.
Pass the variable to the task with `environment: { DOST_PROGRESS_FILE: "{{ lookup('env', 'DOST_PROGRESS_FILE') }}" }`
when the task runs on another host or through `become`. Outside `dost` the variable is unset and a well-behaved
script does nothing.
