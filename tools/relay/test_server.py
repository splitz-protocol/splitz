#!/usr/bin/env python3
"""The relay's state file: one writer at a time, and no save lost to an error.

Ordered by events rather than sleeps, against the module's own `flush`.

Usage: python3 tools/relay/test_server.py
"""
import json
import os
import stat
import sys
import tempfile
import threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import server  # noqa: E402

CH = "a" * 64


def reset(state_file):
    server.STATE_FILE = state_file
    server.CHANNELS.clear()
    server.DIRTY.clear()
    server.json = json


def a_stop_during_a_save_keeps_every_push():
    """The flush on stop waits for a save under way, then writes what came after."""
    with tempfile.TemporaryDirectory() as d:
        state = os.path.join(d, "s.json")
        reset(state)
        server.CHANNELS[CH] = ["old"]
        server.DIRTY.set()

        in_dump = threading.Event()
        release = threading.Event()
        saver_thread = threading.current_thread()  # replaced below

        class Held:
            def __getattr__(self, name):
                return getattr(json, name)

            def dump(self, obj, f):
                if threading.current_thread() is saver_thread:
                    f.write('{"' + CH + '": ["old", ')
                    f.flush()
                    in_dump.set()
                    release.wait()
                    f.write("]}")
                    f.flush()
                    return
                json.dump(obj, f)

        server.json = Held()
        # Daemons, and the held save released on the way out, so a failed
        # assertion ends the run rather than leaving a thread waiting.
        saving = threading.Thread(target=server.flush, daemon=True)
        saver_thread = saving
        saving.start()
        assert in_dump.wait(5), "the save never started"

        # A push lands mid-save, then the stop arrives.
        with server.LOCK:
            server.CHANNELS[CH].append("later")
        server.DIRTY.set()
        stopping = threading.Thread(target=server.flush, daemon=True)
        try:
            stopping.start()
            stopping.join(0.3)
            assert stopping.is_alive(), "the stop wrote while a save was under way"
        finally:
            release.set()
        saving.join(5)
        stopping.join(5)
        with open(state, encoding="utf-8") as f:
            held = json.load(f)
        assert held == {CH: ["old", "later"]}, held
        assert not os.path.exists(state + ".tmp")


def a_failed_save_is_tried_again():
    """A save the disk refuses leaves the store marked, so the next one writes it."""
    with tempfile.TemporaryDirectory() as d:
        state = os.path.join(d, "s.json")
        reset(state)
        server.CHANNELS[CH] = ["one"]
        server.DIRTY.set()
        os.chmod(d, stat.S_IRUSR | stat.S_IXUSR)
        try:
            server.flush()
            raise AssertionError("a refused write was not reported")
        except OSError:
            pass
        finally:
            os.chmod(d, stat.S_IRWXU)
        assert server.DIRTY.is_set(), "the refused save was forgotten"
        server.flush()
        with open(state, encoding="utf-8") as f:
            assert json.load(f) == {CH: ["one"]}


if __name__ == "__main__":
    for test in (a_stop_during_a_save_keeps_every_push, a_failed_save_is_tried_again):
        test()
        print(f"ok  {test.__name__}")
