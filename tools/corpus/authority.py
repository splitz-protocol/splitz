#!/usr/bin/env python3
"""Generates vectors/authority.json from SPEC.md section 10.7.

The curve operation is the host's (section 13), so a case cannot carry a real
signature and stay implementation-neutral. Each case instead carries
`verifies`: the entry ids whose signature the host is to be taken as having
verified against the key that entry names. Everything around that — which key
binds to which participant, what a rival claim costs, that the creator is bound
by the invite rather than by a join — is this protocol's, and is what these
cases pin.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import b64url, derive_bill_id, resolve_identities, stand_in  # noqa: E402


def AT(m):
    return f"2026-10-28T19:{m:02d}:00.000Z"


KEY_A = b64url(b"a" * 32)
KEY_B = b64url(b"b" * 32)
KEY_RIVAL = b64url(b"r" * 32)
NONCE = b64url(b"n" * 16)


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


def join(eid, author, pid, key=None, minute=1):
    p = {"id": pid, "name": pid.title()}
    if key is not None:
        p["identityKey"] = key
    return {"v": 1, "id": eid, "author": author, "kind": "joinBill",
            "at": AT(minute), "participant": p}


CASES = [
    ("a_self_claim_binds_a_key",
     [C, join("j1", "ben", "ben", KEY_B)], ["j1", C["id"]]),

    ("a_self_claim_whose_signature_does_not_verify_binds_nothing",
     [C, join("j1", "ben", "ben", KEY_B)], [C["id"]]),

    ("a_join_naming_somebody_else_proves_nothing_about_them",
     [C, join("j1", "ana", "ben", KEY_B)], ["j1", C["id"]]),

    ("a_join_stating_no_key_binds_nothing",
     [C, join("j1", "ben", "ben")], ["j1", C["id"]]),

    ("the_creator_is_bound_by_the_invite",
     [C], [C["id"]]),

    ("an_unverified_create_entry_binds_no_creator",
     [C], []),

    ("a_join_claiming_the_creators_id_is_not_a_rival_claim",
     [C, join("j1", "ana", "ana", KEY_RIVAL)], ["j1", C["id"]]),

    ("two_keys_claiming_one_id_leaves_it_contested",
     [C, join("j1", "ben", "ben", KEY_B, 1),
      join("j2", "ben", "ben", KEY_RIVAL, 2)], ["j1", "j2", C["id"]]),

    ("a_rival_claim_that_does_not_verify_does_not_contest",
     [C, join("j1", "ben", "ben", KEY_B, 1),
      join("j2", "ben", "ben", KEY_RIVAL, 2)], ["j1", C["id"]]),

    ("backdating_does_not_decide_a_contest",
     [C, join("j1", "ben", "ben", KEY_RIVAL, 0),
      join("j2", "ben", "ben", KEY_B, 9)], ["j1", "j2", C["id"]]),

    ("one_binding_and_one_contest_side_by_side",
     [C, join("j1", "ben", "ben", KEY_B, 1),
      join("j2", "ben", "ben", KEY_RIVAL, 2),
      join("j3", "cai", "cai", KEY_A, 3)], ["j1", "j2", "j3", C["id"]]),

    ("nobody_publishes_a_key",
     [C, join("j1", "ben", "ben"), join("j2", "cai", "cai")], [C["id"]]),

    ("a_participant_rejoining_with_the_same_key_is_not_a_contest",
     [C, join("j1", "ben", "ben", KEY_B, 1),
      join("j2", "ben", "ben", KEY_B, 2)], ["j1", "j2", C["id"]]),

    ("the_creator_binds_even_when_nobody_else_does",
     [C, join("j1", "ana", "ben", KEY_B)], [C["id"]]),

    # A withdrawal does not undo a claim. Were it to, an impostor who minted a
    # rival claim could withdraw the genuine one — §10.8 lets either author
    # withdraw either, since both name the same participant — and bind their
    # own key to that participant.
    ("withdrawing_a_self_claim_does_not_unbind_it",
     [C, join("j1", "ben", "ben", KEY_B),
      void("v1", "ben", "j1", 5)], ["j1", C["id"]]),

    ("withdrawing_a_rival_claim_does_not_clear_the_contest",
     [C, join("j1", "ben", "ben", KEY_B, 1),
      join("j2", "ben", "ben", KEY_RIVAL, 2),
      void("v1", "ben", "j2", 5)], ["j1", "j2", C["id"]]),

    ("an_impostor_withdrawing_the_genuine_claim_binds_nothing",
     [C, join("j1", "ben", "ben", KEY_B, 1),
      join("j2", "ben", "ben", KEY_RIVAL, 2),
      void("v1", "ben", "j1", 5)], ["j1", "j2", C["id"]]),
]


def main():
    out = []
    for name, entries, verifies in CASES:
        verified = set(verifies)
        create_entry = next(e for e in entries if e["kind"] == "createBill")
        bound, contested = resolve_identities(
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
                "contested": sorted(contested),
            },
        })

    # A contested id is bound to nothing: the whole point is that a rival claim
    # costs the ability to be paid rather than handing the identity over.
    for case in out:
        both = set(case["expect"]["bound"]) & set(case["expect"]["contested"])
        assert not both, f"{case['name']}: {both} is both bound and contested"

    doc = {"description": "Which key binds to which participant. "
                          "SPEC.md section 10.7.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "authority.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{len(out)} cases -> {p.name}")


if __name__ == "__main__":
    main()
