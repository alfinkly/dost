# dost_stream: stdout callback that prints one JSON object per line for `dost`.
# Not meant to be read by humans. Enabled by dost via ANSIBLE_STDOUT_CALLBACK.
from __future__ import absolute_import, division, print_function

__metaclass__ = type

DOCUMENTATION = """
    name: dost_stream
    type: stdout
    short_description: JSON lines for the dost terminal player
    description:
      - Emits start, task, result, event and recap lines. Tasks with no_log never emit data.
"""

import json
import os
import sys

from ansible.plugins.callback import CallbackBase


def _emit(obj):
    sys.stdout.write(json.dumps(obj, default=str) + "\n")
    sys.stdout.flush()


class CallbackModule(CallbackBase):
    CALLBACK_VERSION = 2.0
    CALLBACK_TYPE = "stdout"
    CALLBACK_NAME = "dost_stream"

    def _no_log(self, result):
        res = result._result
        return bool(result._task.no_log or res.get("_ansible_no_log"))

    def v2_playbook_on_start(self, playbook):
        mode = "check" if os.environ.get("DOST_MODE") == "check" else "apply"
        _emit({"t": "start", "mode": mode})

    def v2_playbook_on_play_start(self, play):
        _emit({"t": "play", "name": play.get_name()})

    def _item(self, result, failed=False, skipped=False):
        res = result._result
        no_log = self._no_log(result)
        label = res.get("_ansible_item_label")
        if label is None:
            label = res.get("item")
        if isinstance(label, (dict, list)):
            label = json.dumps(label, default=str, sort_keys=True)
        _emit(
            {
                "t": "item",
                "host": result._host.get_name(),
                "task": result._task.get_name(),
                "item": None if (no_log or label is None) else str(label),
                "changed": bool(res.get("changed", False)),
                "failed": failed,
                "skipped": skipped,
                "no_log": no_log,
            }
        )

    def v2_playbook_on_task_start(self, task, is_conditional):
        _emit({"t": "task", "name": task.get_name()})

    def _maybe_event(self, result, res):
        # events come only from debug tasks that are not no_log
        if self._no_log(result):
            return
        msg = res.get("msg")
        if isinstance(msg, dict) and isinstance(msg.get("dost"), dict):
            body = dict(msg["dost"])
            version = body.pop("v", 1)
            kind = body.pop("kind", None)
            key = body.pop("key", None)
            if kind is None:
                return
            _emit(
                {
                    "t": "event",
                    "host": result._host.get_name(),
                    "v": version,
                    "kind": str(kind),
                    "key": None if key is None else str(key),
                    "data": body,
                }
            )

    def _result(self, result, failed=False, skipped=False, unreachable=False):
        res = result._result
        no_log = self._no_log(result)
        msg = res.get("msg")
        diff = res.get("diff")
        if isinstance(msg, (dict, list)):
            msg = None
        if isinstance(diff, (dict, list)):
            diff = json.dumps(diff, default=str)
        _emit(
            {
                "t": "result",
                "host": result._host.get_name(),
                "task": result._task.get_name(),
                "module": str(result._task.action),
                "changed": bool(res.get("changed", False)),
                "failed": failed,
                "skipped": skipped,
                "unreachable": unreachable,
                "msg": None if no_log else msg,
                "diff": None if no_log else diff,
                "no_log": no_log,
            }
        )

    def v2_runner_on_ok(self, result):
        res = result._result
        if "results" not in res:
            self._maybe_event(result, res)
        self._result(result)

    def v2_runner_item_on_ok(self, result):
        self._maybe_event(result, result._result)
        self._item(result)

    def v2_runner_item_on_failed(self, result):
        self._item(result, failed=True)

    def v2_runner_item_on_skipped(self, result):
        self._item(result, skipped=True)

    def v2_runner_on_failed(self, result, ignore_errors=False):
        self._result(result, failed=not ignore_errors)

    def v2_runner_on_skipped(self, result):
        self._result(result, skipped=True)

    def v2_runner_on_unreachable(self, result):
        self._result(result, unreachable=True)

    def v2_playbook_on_stats(self, stats):
        for host in sorted(stats.processed.keys()):
            s = stats.summarize(host)
            _emit(
                {
                    "t": "recap",
                    "host": host,
                    "ok": s["ok"],
                    "changed": s["changed"],
                    "failed": s["failures"],
                    "unreachable": s["unreachable"],
                }
            )
