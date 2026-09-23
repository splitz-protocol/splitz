#!/usr/bin/env python3
"""Operation lists for the host layer, answered by both implementations.

The protocol's corpus is generated from one reference, so it cannot hold a case
that reference is self-consistently wrong about. The host layer has no corpus
at all: nothing in `vectors/` covers a curve operation, because a vector cannot
carry a private key. This generates the inputs instead, and the two host
implementations answer them independently.

Usage: python3 tools/differential/host_generate.py <seed> <count>
"""
import base64
import json
import random
import sys




# Bills a signature is made or checked on (§10.6).
BILLS = ["g0a5mrH6D5nx5bJ7KrgwVA", "AAAAAAAAAAAAAAAAAAAAAA", "b-2"]

def _b64(raw):
    import base64
    return base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=")

def b64(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).decode().rstrip("=")


def a_seed(rng: random.Random) -> str:
    return b64(bytes(rng.randrange(256) for _ in range(32)))


def an_entry(rng: random.Random) -> dict:
    """An entry shaped the way §9.1 writes one, with the fields §10.6 covers."""
    entry = {
        "v": 1,
        "id": b64(bytes(rng.randrange(256) for _ in range(16))),
        "author": rng.choice(["ana", "ben", "cal", "dee"]),
        "kind": rng.choice(["joinBill", "addExpense", "recordPayment"]),
        "at": f"2026-10-28T19:{rng.randrange(60):02}:{rng.randrange(60):02}Z",
    }
    if rng.random() < 0.5:
        entry["participant"] = {
            "id": entry["author"],
            "name": rng.choice(["Ana", "Ben", "Cal", ""]),
        }
    if rng.random() < 0.3:
        entry["amount"] = rng.randrange(-10_000, 10_000)
    if rng.random() < 0.3:
        # Non-ASCII, because §9.3's canonical form is where two encoders
        # disagree first.
        entry["note"] = rng.choice(["café", "naïve", "日本語", "a\u0000b", "🧾"])
    return entry


def a_bill_key(rng: random.Random) -> str:
    """A key for the cipher. Most are usable; some are the wrong length."""
    n = rng.choice([32, 32, 32, 32, 31, 33, 3])
    return b64(bytes(rng.randrange(256) for _ in range(n)))


def a_raw_blob(rng: random.Random) -> str:
    """Something a relay might hand back that is not a blob this build made."""
    kind = rng.randrange(6)
    if kind == 0:
        return ""
    if kind == 1:
        return b64(bytes([1]))                       # a version byte and nothing else
    if kind == 2:
        return b64(bytes([1]) + bytes(rng.randrange(256) for _ in range(39)))
    if kind == 3:
        return b64(bytes([rng.choice([0, 2, 7, 255])])
                   + bytes(rng.randrange(256) for _ in range(60)))
    if kind == 4:
        return "not base64url!!"
    return b64(bytes(rng.randrange(256) for _ in range(rng.randrange(0, 45))))


CODES = ["self_payment", "unauthorized_entry", "unknown_entry", "currency_mismatch",
         "bill_bad_currency", "unknown_participant", "participant_still_named"]


def a_bill(rng: random.Random) -> dict:
    """A bill document, in the shape §9 writes one."""
    people = ["ana", "ben", "cal"][:rng.randrange(2, 4)]
    payments = []
    for i in range(rng.randrange(0, 3)):
        payer, payee = rng.sample(people, 2)
        payments.append({
            "id": f"p{i}",
            "from": payer,
            "to": payee,
            "amount": rng.randrange(1, 5000),
            "currency": "EUR",
            "method": rng.choice(["shieldedZec", "cash", "swap"]),
            "at": f"2026-10-28T20:{i:02}:00Z",
        })
    confirmed = [p["id"] for p in payments if rng.random() < 0.5]
    return {
        "v": 1,
        "id": "b1",
        "name": "Dinner",
        "currency": "EUR",
        "splitMode": "equal",
        "participants": [{"id": who, "name": who.title()} for who in people],
        "expenses": [],
        "payments": payments,
        "confirmedPayments": confirmed,
    }


def a_history_entry(rng: random.Random) -> dict:
    """An entry shaped the way one kind of history line reads."""
    who = rng.choice(["ana", "ben", "cal"])
    at = f"2026-10-28T19:{rng.randrange(60):02}:{rng.randrange(60):02}Z"
    base = {"v": 1, "id": b64(bytes(rng.randrange(256) for _ in range(16))),
            "author": who, "at": at}
    kind = rng.choice(["createBill", "joinBill", "joinBill", "addExpense",
                       "amendEntry", "voidEntry", "recordPayment",
                       "confirmPayment", "setRate", "somethingElse"])
    base["kind"] = kind
    if kind == "createBill":
        base["bill"] = {"name": rng.choice(["Dinner", "Trip", ""])}
    elif kind == "joinBill":
        participant = {"id": who, "name": who.title()}
        if rng.random() < 0.7:
            participant["payTo"] = f"u1{who}"
        base["participant"] = participant
    elif kind == "addExpense":
        base["expense"] = {"paidBy": who, "amount": rng.randrange(-500, 9000),
                           "description": rng.choice(["Wine", "", "café"])}
    elif kind == "voidEntry":
        base["void"] = {"target": "e-gone"}
    elif kind == "recordPayment":
        base["payment"] = {"id": rng.choice(["p0", "p1", "p9"]), "to": who,
                           "amount": rng.randrange(1, 5000),
                           "method": rng.choice(["shieldedZec", "swap", "cash"]),
                           "reference": rng.choice(["ref-1", None])}
    elif kind == "confirmPayment":
        base["confirmation"] = {"paymentId": rng.choice(["p0", "p1"]),
                                "method": "shieldedZec", "reference": "tx-1"}
    elif kind == "setRate":
        base["rate"] = {"minorUnitsPerZec": rng.randrange(1, 10**6),
                        "source": rng.choice(["a feed", ""])}
    return base


PEOPLE = ["ana", "ben", "cal", "dee"]


def weights(rng: random.Random, span: tuple[int, int]) -> dict:
    return {who: rng.randrange(*span)
            for who in rng.sample(PEOPLE, rng.randrange(0, 4))}


def a_draft(rng: random.Random) -> dict:
    """A split form in whatever state somebody left it."""
    return {
        "kind": rng.choice(["equal", "exact", "percentage", "shares",
                            "itemized"]),
        "among": rng.sample(PEOPLE, rng.randrange(0, 4)),
        "amounts": weights(rng, (-2000, 9000)),
        "basisPoints": weights(rng, (0, 7000)),
        "shareCounts": weights(rng, (0, 5)),
        "items": [
            {"description": rng.choice(["tacos", "beer", "", "café"]),
             "minorUnits": rng.randrange(-500, 6000),
             "sharedBy": rng.sample(PEOPLE, rng.randrange(0, 3))}
            for _ in range(rng.randrange(0, 3))
        ],
        "extra": rng.choice([0, 0, 1000, -100]),
        "toggle": rng.sample(PEOPLE, rng.randrange(0, 3)),
        "total": rng.choice([9000, 1000, 0, -9000, 10**18]),
    }


STATUS_WORDS = ["PENDING_DEPOSIT", "KNOWN_DEPOSIT_TX", "INCOMPLETE_DEPOSIT",
                "SUCCESS", "FAILED", "REFUNDED", "PROCESSING", "EXPIRED",
                "something_new", "", "3"]

TEXTS = ["plain", "a b", "a+b", "a/b", "a&b=c", "café", "a~_-.b", "💸",
         "!'()*", "", "%2F", "a\tb"]


def a_quote_body(rng: random.Random) -> dict:
    quote = {}
    if rng.random() < 0.85:
        quote["depositAddress"] = rng.choice(["u1provider", ""])
    if rng.random() < 0.85:
        quote["amountOut"] = rng.choice(["12340000", ""])
    if rng.random() < 0.6:
        quote["minAmountOut"] = rng.choice(["12216600", "", 7])
    if rng.random() < 0.5:
        quote["depositMemo"] = rng.choice(["memo-1", ""])
    if rng.random() < 0.5:
        quote["deadline"] = rng.choice(["2026-10-28T19:35:00Z", "not a time",
                                        "2026-10-28T19:35:00.500Z"])
    body = {"quote": quote} if rng.random() < 0.9 else {}
    if rng.random() < 0.6:
        body["correlationId"] = rng.choice(["near-intent-7f3a", ""])
    return body


def a_watch_json(rng: random.Random) -> dict:
    out = {}
    for key in ["billId", "reference", "to", "depositAddress", "depositMemo",
                "assetSymbol", "assetChain"]:
        if rng.random() < 0.8:
            out[key] = rng.choice(["b1", "r1", "ben", "u1provider", "memo-1",
                                   "USDC", "base", "", "a/b"])
    if rng.random() < 0.1:
        out["billId"] = 7
    return out


def a_key_ish(rng: random.Random) -> str:
    """Something a wallet might be handed as a key. Most are not valid."""
    kind = rng.randrange(7)
    if kind == 0:
        return b64(bytes(rng.randrange(256) for _ in range(32)))
    if kind == 1:
        return base64.urlsafe_b64encode(
            bytes(rng.randrange(256) for _ in range(32))).decode()  # padded
    if kind == 2:
        return ""
    if kind == 3:
        return "A" * rng.choice([42, 43, 44, 45])
    if kind == 4:
        return b64(bytes(rng.randrange(256) for _ in range(rng.randrange(1, 40))))
    if kind == 5:
        return "not base64url!!"
    return "+/" + "A" * 41  # standard base64's alphabet, not url's


def a_settle_case(rng: random.Random) -> dict:
    """A whole bill, and one participant settling what it says they owe.

    Both runners build the entries themselves from this, so the operation
    compares entry construction, the fold, the obligation and §14's
    withholding as well as the records a send is allowed to write. The
    instants are supplied rather than derived, because a clock is the one
    thing the two implementations cannot be asked to agree about on their own.

    The shapes are chosen rather than left to chance. A bill drawn at random
    leaves the settling participant in credit almost every time, and an
    operation that settles nothing compares nothing: `many` is the shape that
    reaches one transaction paying several people, which is where a record's
    own id matters.
    """
    n = rng.randrange(2, 5)
    who = PEOPLE[:n]
    # §9.2's lanes, so the operation also diffs which payees §8.5 carries and
    # which it withholds, and for which of the two reasons.
    def a_payout(p: str):
        r = rng.random()
        if r < 0.45:
            return None
        if r < 0.65:
            return [{"type": "zec", "address": f"u1{p}"}]
        if r < 0.8:
            return [{"type": "swap", "asset": "USDC", "chain": "base",
                     "address": f"0x{p}"}]
        if r < 0.95:
            return [{"type": "cash"}]
        return [{"type": "giftCard", "address": f"g-{p}"}]

    people = [
        {
            "id": p,
            "payTo": f"u1{p}" if rng.random() < 0.8 else None,
            "payouts": a_payout(p),
        }
        for p in who
    ]
    shape = rng.choice(["one", "many", "many", "any"])
    if shape == "one":
        # Everybody else owes the one person who paid.
        payers = [who[0]] * rng.randrange(1, 4)
        me = rng.choice(who[1:])
    elif shape == "many":
        # Each of the others pays one expense and the last person pays none,
        # so that one settles a debt to several people at once.
        payers = who[:-1]
        me = who[-1]
    else:
        payers = [rng.choice(who) for _ in range(rng.randrange(1, 4))]
        me = rng.choice(who)
    # In `many` the payers must all end up in credit, or the one who paid
    # least owes as well and the plan nets down to a single payment — the case
    # this shape exists to avoid. Comparable amounts keep every payer a
    # creditor; elsewhere the amount is free, including the ones §5 refuses.
    base = rng.randrange(6000, 120000)
    expenses = [
        {
            "id": f"x{i}",
            "paidBy": payer,
            "amount": (base + rng.randrange(-base // 40, base // 40 + 1)
                       if shape == "many" else rng.randrange(-200, 120000)),
            "among": (who if shape != "any"
                      else sorted(rng.sample(who, rng.randrange(1, n + 1)))),
        }
        for i, payer in enumerate(payers)
    ]
    rate = None if rng.random() < 0.15 else rng.randrange(1, 10**7)
    # create, one join each, the expenses, the rate if there is one, and one
    # more for the send itself.
    steps = 1 + n + len(expenses) + (1 if rate is not None else 0) + 1
    return {
        "op": "settle_records",
        "people": people,
        "expenses": expenses,
        "rate": rate,
        "me": me,
        "shape": shape,
        "creatorKey": b64(bytes(rng.randrange(256) for _ in range(32))),
        "txid": ("" if rng.random() < 0.1
                 else "tx-" + b64(bytes(rng.randrange(256) for _ in range(8)))),
        "instants": [f"2026-10-28T19:{i // 60:02}:{i % 60:02}Z"
                     for i in range(steps)],
    }


def main() -> int:
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 400
    rng = random.Random(seed)
    seeds = [a_seed(rng) for _ in range(8)]
    out = sys.stdout
    for _ in range(count):
        op = rng.choice([
            "public_key", "sign_entry", "verify", "identity_seed",
            "well_formed_key", "b64_round_trip", "seal_open", "open_raw",
            "store_read", "store_merge", "activity", "split_draft",
            "swap_encode", "swap_status", "swap_quote", "swap_watch",
            "settle_records",
        ])
        if op == "settle_records":
            json.dump(a_settle_case(rng), out)
            out.write("\n")
            continue
        if op == "public_key":
            json.dump({"op": op, "seed": rng.choice(seeds)}, out)
        elif op == "sign_entry":
            json.dump({"op": op, "seed": rng.choice(seeds),
                       "entry": an_entry(rng),
                       "bill": rng.choice(BILLS)}, out)
        elif op == "verify":
            # Signed on one bill and sometimes checked on another: §10.6 puts
            # the bill in the message, so both must refuse that.
            bill = rng.choice(BILLS)
            json.dump({"op": op, "seed": rng.choice(seeds),
                       "entry": an_entry(rng),
                       "key": a_key_ish(rng),
                       "signWith": rng.choice(seeds),
                       "bill": bill,
                       "verifyOn": rng.choice(BILLS) if rng.random() < 0.3 else bill,
                       "tamper": rng.random() < 0.3}, out)
        elif op == "identity_seed":
            # Unpadded base64url of the bytes a wallet derives from its
            # spending secret; empty is an account with none.
            json.dump({"op": op, "secret": rng.choice(
                ["", _b64(b"\x01\x02\x03"), _b64("mnemonic words".encode()),
                 _b64("日本語".encode()), _b64(bytes(range(256)))])}, out)
        elif op == "well_formed_key":
            json.dump({"op": op, "key": a_key_ish(rng)}, out)
        elif op == "seal_open":
            json.dump({"op": op,
                       "key": a_bill_key(rng),
                       "openWith": a_bill_key(rng) if rng.random() < 0.3 else None,
                       "entry": an_entry(rng),
                       "tamper": rng.choice([0, 0, 1, 2])}, out)
        elif op == "swap_encode":
            json.dump({"op": op, "text": rng.choice(TEXTS)}, out)
        elif op == "swap_status":
            body = {"status": rng.choice(STATUS_WORDS)}
            # The provider's shape, and the shapes a reader must survive: a
            # missing or non-object `swapDetails`, an empty or malformed hash
            # list, and top-level lookalikes that must NOT be read.
            details = rng.choice([None, "not-an-object", {}])
            if isinstance(details, dict):
                hashes = rng.choice([None, [], ["0xbare"], [{"hash": ""}],
                                     [{"hash": "0xdead", "explorerUrl": "u"},
                                      {"hash": "0xsecond"}],
                                     [{"explorerUrl": "u"}]])
                if hashes is not None:
                    details["destinationChainTxHashes"] = hashes
                if rng.random() < 0.5:
                    details["refundReason"] = rng.choice(["", "a note", 7])
            if details is not None:
                body["swapDetails"] = details
            for key in ["destinationTxHash", "message"]:
                if rng.random() < 0.3:
                    body[key] = rng.choice(["0xtop", "", "top"])
            json.dump({"op": op, "body": body,
                       "memo": rng.choice(["memo-1", "", None, "a/b"])}, out)
        elif op == "swap_quote":
            json.dump({"op": op,
                       "amount": rng.choice([0, -1, 1, 1000000]),
                       "recipient": rng.choice(["0xcara", ""]),
                       "refundTo": rng.choice(["u1ana", ""]),
                       "deadline": "2026-10-28T19:40:00.000Z",
                       "body": a_quote_body(rng),
                       # How the scripted provider answers: echoing the
                       # request it was sent, echoing it for another
                       # recipient, or not at all.
                       "echo": rng.choice(["asked", "asked", "asked",
                                           "other", None])}, out)
        elif op == "swap_watch":
            json.dump({"op": op, "json": a_watch_json(rng)}, out)
        elif op == "split_draft":
            json.dump({"op": op, **a_draft(rng)}, out)
        elif op == "activity":
            entries = [a_history_entry(rng) for _ in range(rng.randrange(0, 7))]
            json.dump({"op": op,
                       "bill": a_bill(rng),
                       "entries": entries,
                       "setAside": [{"id": e["id"], "code": rng.choice(CODES)}
                                    for e in entries if rng.random() < 0.3],
                       "withdrawn": [e["id"] for e in entries
                                     if rng.random() < 0.2],
                       "me": rng.choice(["ana", "ben", "cal", "zzz"])}, out)
        elif op == "store_read":
            json.dump({"op": op, "stored": rng.choice([
                "", "not json", '{"not":"a list"}', "[1,2,3]", "null",
                json.dumps([an_entry(rng) for _ in range(rng.randrange(0, 4))]),
                json.dumps([an_entry(rng), 7, None]),
            ])}, out)
        elif op == "store_merge":
            json.dump({"op": op,
                       "held": [an_entry(rng) for _ in range(rng.randrange(0, 3))],
                       "incoming": [an_entry(rng) for _ in range(rng.randrange(0, 3))]}, out)
        elif op == "open_raw":
            json.dump({"op": op, "key": a_bill_key(rng),
                       "blob": a_raw_blob(rng)}, out)
        else:
            json.dump({"op": op, "text": a_key_ish(rng)}, out)
        out.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
