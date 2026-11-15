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
| `link` | `a`, `b`, `state: up/down` (use `key` to send several) | `vpn` (map of hosts; a down link shows a break marker, hosts named only in links show as not joined) |

The `chain` scene (task progress) and the `config` scene (tasks that changed something, with
the first line of the diff when there is one) need no events; they read Ansible's task results.

## Check runs
During `--check` the same events arrive. `dost` draws them dimmed with the label
"ПРОВЕРКА, предварительно": they describe what would happen, not what is.
