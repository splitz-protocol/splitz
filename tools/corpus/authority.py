#!/usr/bin/env python3
"""Generates vectors/authority.json from SPEC.md section 10.7.

The curve operation is the host's (section 13), so a case cannot carry a real
signature and stay implementation-neutral. Each case instead carries
`verifies`: the entry ids whose signature the host is to be taken as having
verified, against any key or, for an item ending `@key`, against that key
alone. Everything around that — which key binds to which participant, that a
key binds only the id it derives, that the creator is bound by the invite
rather than by a join — is this protocol's, and is what these cases pin.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (b64url, derive_bill_id, participant_id,  # noqa: E402
                   resolve_identities, stand_in)


def AT(m):
    return f"2026-10-28T19:{m:02d}:00.000Z"


KEY_A = b64url(b"a" * 32)
KEY_B = b64url(b"b" * 32)
KEY_C = b64url(b"c" * 32)
KEY_RIVAL = b64url(b"r" * 32)
NONCE = b64url(b"n" * 16)

# A participant who publishes a key is named by the id that key derives.
BEN = participant_id(KEY_B)
CAI = participant_id(KEY_C)


def create(key=KEY_A, author="ana"):
    e = {"v": 1, "author": author, "kind": "createBill", "at": AT(0),
         "name": "Dinner", "currency": "EUR", "splitMode": "equal",
         "creatorKey": key, "nonce": NONCE}
    e["id"] = derive_bill_id(e)
    return e


C = create()


def void(eid, author, target, minute):
    return {"v": 1, "id": eid, "author": author, "kind": "voidEntry",
            "at": AT(minute), "targetId": target}


def join(eid, author, pid, key=None, minute=1, name="Ben"):
    p = {"id": pid, "name": name}
    if key is not None:
        p["identityKey"] = key
    return {"v": 1, "id": eid, "author": author, "kind": "joinBill",
            "at": AT(minute), "participant": p}


CASES = [
    ("a_self_claim_binds_a_key",
     [C, join("j1", BEN, BEN, KEY_B)], ["j1", C["id"]]),

    ("a_self_claim_whose_signature_does_not_verify_binds_nothing",
     [C, join("j1", BEN, BEN, KEY_B)], [C["id"]]),

    ("a_self_claim_verifies_against_the_key_it_states",
     [C, join("j1", BEN, BEN, KEY_B)], [f"j1@{KEY_B}", f"{C['id']}@{KEY_A}"]),

    ("a_self_claim_signed_by_another_key_binds_nothing",
     [C, join("j1", BEN, BEN, KEY_B)], [f"j1@{KEY_RIVAL}", C["id"]]),

    ("a_join_naming_somebody_else_proves_nothing_about_them",
     [C, join("j1", "ana", BEN, KEY_B)], ["j1", C["id"]]),

    ("a_join_stating_no_key_binds_nothing",
     [C, join("j1", "ben", "ben")], ["j1", C["id"]]),

    ("a_key_binds_only_the_id_it_derives",
     [C, join("j1", "ben", "ben", KEY_B)], ["j1", C["id"]]),

    ("the_creator_is_bound_by_the_invite",
     [C], [C["id"]]),

    ("the_creator_is_bound_by_the_key_the_create_states",
     [C], [f"{C['id']}@{KEY_A}"]),

    ("an_unverified_create_entry_binds_no_creator",
     [C], []),

    ("a_join_claiming_the_creators_id_binds_nothing",
     [C, join("j1", "ana", "ana", KEY_RIVAL, name="Ana")], ["j1", C["id"]]),

    # A second key cannot derive a bound participant's id, so a rival claim
    # binds nothing and takes nothing from the participant it names.
    ("a_rival_key_cannot_claim_a_bound_id",
     [C, join("j1", BEN, BEN, KEY_B, 1),
      join("j2", BEN, BEN, KEY_RIVAL, 2)], ["j1", "j2", C["id"]]),

    ("backdating_a_rival_claim_changes_nothing",
     [C, join("j1", BEN, BEN, KEY_RIVAL, 0),
      join("j2", BEN, BEN, KEY_B, 9)], ["j1", "j2", C["id"]]),

    ("two_participants_bound_side_by_side",
     [C, join("j1", BEN, BEN, KEY_B, 1),
      join("j2", BEN, BEN, KEY_RIVAL, 2),
      join("j3", CAI, CAI, KEY_C, 3, name="Cai")],
     ["j1", "j2", "j3", C["id"]]),

    ("nobody_publishes_a_key",
     [C, join("j1", "ben", "ben"), join("j2", "cai", "cai", name="Cai")],
     [C["id"]]),

    ("a_participant_rejoining_with_the_same_key_stays_bound",
     [C, join("j1", BEN, BEN, KEY_B, 1),
      join("j2", BEN, BEN, KEY_B, 2)], ["j1", "j2", C["id"]]),

    ("the_creator_binds_even_when_nobody_else_does",
     [C, join("j1", "ana", BEN, KEY_B)], [C["id"]]),

    # A withdrawal does not undo a claim: section 10.8 lets that participant
    # withdraw their own join, and the binding is evidence that was made.
    ("withdrawing_a_self_claim_does_not_unbind_it",
     [C, join("j1", BEN, BEN, KEY_B),
      void("v1", BEN, "j1", 5)], ["j1", C["id"]]),

    ("a_rival_withdrawing_the_genuine_claim_binds_nothing_for_them",
     [C, join("j1", BEN, BEN, KEY_B, 1),
      join("j2", BEN, BEN, KEY_RIVAL, 2),
      void("v1", BEN, "j1", 5)], ["j1", "j2", C["id"]]),
]


def main():
    out = []
    for name, entries, verifies in CASES:
        verified = set(verifies)
        create_entry = next(e for e in entries if e["kind"] == "createBill")
        bound = resolve_identities(
            entries, create_entry,
            verify=stand_in(verified),
        )
        out.append({
            "name": name,
            "log": entries,
            "billId": create_entry["id"],
            # The host is taken to have verified these entries' signatures.
            "verifies": sorted(verified),
            "expect": {
                "bound": {k: bound[k] for k in sorted(bound)},
            },
        })

    # A key binds only the id it derives, or the creator's by the invite.
    for case in out:
        for pid, key in case["expect"]["bound"].items():
            assert pid == "ana" or participant_id(key) == pid, case["name"]

    doc = {"description": "Which key binds to which participant. "
                          "SPEC.md section 10.7.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "authority.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{len(out)} cases -> {p.name}")


if __name__ == "__main__":
    main()
