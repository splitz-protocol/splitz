#!/usr/bin/env python3
"""The relay's state file: one writer at a time, and no save lost to an error.

Ordered by events rather than sleeps, against the module's own `flush`.

Usage: python3 tools/relay/test_server.py
"""
import json
import os
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import server  # noqa: E402

CH = "a" * 64


def stamp(channel, blobs, bits):
    """The first nonce from zero whose stamp has `bits` leading zero bits."""
    n = 0
    while server.work_done(channel, blobs, str(n)) < bits:
        n += 1
    return str(n)


def short_stamp(channel, blobs, bits):
    """A nonce whose stamp has exactly `bits - 1` leading zero bits."""
    n = 0
    while server.work_done(channel, blobs, str(n)) != bits - 1:
        n += 1
    return str(n)


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


def a_stop_by_any_signal_keeps_the_last_push():
    """INT, TERM (`kill`) and HUP (a closed terminal) all end through the
    flush, so a push made just before the stop is in the state file."""
    here = os.path.dirname(os.path.abspath(__file__))
    for stop in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        with tempfile.TemporaryDirectory() as d:
            state = os.path.join(d, "s.json")
            relay = subprocess.Popen(
                [sys.executable, os.path.join(here, "server.py"), "--port", "0",
                 "--state-file", state],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            try:
                origin = relay.stdout.readline().split()[-1]
                push = urllib.request.Request(
                    f"{origin}/c/{CH}",
                    data=json.dumps({"blobs": [stop.name],
                                     "work": stamp(CH, [stop.name], 16)}).encode(),
                    method="POST",
                )
                with urllib.request.urlopen(push) as answer:
                    assert answer.status == 200
                relay.send_signal(stop)
                try:
                    code = relay.wait(30)
                except subprocess.TimeoutExpired:
                    relay.kill()
                    raise AssertionError(
                        f"{stop.name}: the relay did not stop in 30 s; "
                        f"stderr: {relay.stderr.read()[-500:]}"
                    )
                assert code == 0, f"{stop.name}: exit {code}"
            finally:
                if relay.poll() is None:
                    relay.kill()
                relay.stdout.close()
                relay.stderr.close()
            with open(state, encoding="utf-8") as f:
                assert json.load(f) == {CH: [stop.name]}, stop.name


def a_push_carries_enough_work():
    """§15.5. Unstamped, short, replayed for other blobs, or not a nonce is
    refused; a stamp at the bits asked, sized to the push, is stored; the
    legacy flag takes an unstamped push."""
    assert stamp(CH, ["x"], 16) == "127719"
    assert stamp(CH, ["x", "y"], 16) == "103390"
    # One list of blobs re-split into another is another stamp.
    assert server.work_done(CH, ["a", "b"], stamp(CH, ["a\nb"], 16)) < 16
    assert server.work_bits(0, 16) == 16 and server.work_bits(1024, 16) == 16
    assert server.work_bits(1025, 16) == 17 and server.work_bits(2049, 16) == 18
    assert server.work_base(0, 512) == 16 and server.work_base(256, 512) == 20
    assert server.work_base(511, 512) == 23
    assert server.NONCE.fullmatch("568171") and not server.NONCE.fullmatch("568171\n")
    assert server.CHANNEL.fullmatch(CH) and not server.CHANNEL.fullmatch(CH + "\n")
    assert server.BLOB.fullmatch("AZaz09-_") and not server.BLOB.fullmatch("AZ=")
    here = os.path.dirname(os.path.abspath(__file__))

    def post(origin, body):
        request = urllib.request.Request(
            f"{origin}/c/{CH}", data=json.dumps(body).encode(), method="POST")
        try:
            with urllib.request.urlopen(request) as answer:
                return answer.status, json.load(answer)
        except urllib.error.HTTPError as e:
            return e.code, json.load(e)

    for legacy in (False, True):
        relay = subprocess.Popen(
            [sys.executable, os.path.join(here, "server.py"), "--port", "0"]
            + (["--legacy-unstamped"] if legacy else []),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            origin = relay.stdout.readline().split()[-1]
            if legacy:
                assert post(origin, {"blobs": ["old"]})[0] == 200
                continue
            assert post(origin, {"blobs": ["u"]}) == (
                428, {"error": "a push carries work", "work": 16})
            assert post(origin, {"blobs": ["s"], "work": short_stamp(CH, ["s"], 16)})[0] == 428
            assert post(origin, {"blobs": ["r"], "work": stamp(CH, ["x"], 16)})[0] == 428
            assert post(origin, {"blobs": ["x"], "work": "abc"})[0] == 400
            assert post(origin, {"blobs": ["x"], "work": 127719})[0] == 400
            assert post(origin, {"blobs": ["x"], "work": None})[0] == 400
            # A nonce or a channel is read whole: a trailing newline is not one.
            assert post(origin, {"blobs": ["x"], "work": stamp(CH, ["x"], 16) + "\n"})[0] == 400
            # A blob is base64url with no padding; anything else is no client's.
            for junk in ("a=", "a b", "é", "\U0001F600", ""):
                assert post(origin, {"blobs": [junk], "work": stamp(CH, [junk], 16)})[0] == 400, junk
            assert post(origin, {"blobs": ["x"], "work": stamp(CH, ["x"], 16)}) == (
                200, {"ok": True})
            big = ["b" * 2049]
            assert post(origin, {"blobs": big, "work": short_stamp(CH, big, 18)})[0] == 428
            assert post(origin, {"blobs": big, "work": stamp(CH, big, 18)})[0] == 200
        finally:
            relay.kill()
            relay.wait()
            relay.stdout.close()
            relay.stderr.close()



def a_page_holds_blobs_up_to_its_bound():
    """§15.5: blobs while each costs its characters plus 3, at most
    PAGE_BYTES, and at least one."""
    edge = server.PAGE_BYTES
    try:
        server.PAGE_BYTES = 16
        blobs = ["aaaa", "bbbbbbbbbb", "cc", "dddddd"]  # cost 7, 13, 5, 9
        assert server.page(blobs, 0) == {"blobs": ["aaaa"], "next": 1}
        assert server.page(blobs, 1) == {"blobs": ["bbbbbbbbbb"], "next": 2}
        assert server.page(blobs, 2) == {"blobs": ["cc", "dddddd"]}
        assert server.page(blobs, 4) == {"blobs": []}
        assert server.page(blobs, 99) == {"blobs": []}
        # Exactly the bound fits; one byte over starts a new page.
        assert server.page(["a" * 5, "b" * 5], 0) == {"blobs": ["a" * 5, "b" * 5]}
        assert server.page(["a" * 5, "b" * 6], 0) == {"blobs": ["a" * 5], "next": 1}
        # A blob alone over the bound is still a page of one.
        assert server.page(["z" * 40, "y"], 0) == {"blobs": ["z" * 40], "next": 1}
    finally:
        server.PAGE_BYTES = edge
    # At the real bound, 127 blobs at the blob ceiling fit and a 128th does not.
    full = ["x" * server.MAX_BLOB_CHARS] * 128
    assert server.page(full, 0)["next"] == 127


if __name__ == "__main__":
    for test in (
        a_page_holds_blobs_up_to_its_bound,
        a_push_carries_enough_work,
        a_stop_during_a_save_keeps_every_push,
        a_failed_save_is_tried_again,
        a_stop_by_any_signal_keeps_the_last_push,
    ):
        test()
        print(f"ok  {test.__name__}")
