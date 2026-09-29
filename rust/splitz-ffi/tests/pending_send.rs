//! §14.3's pay-twice guard for a wallet that keeps the note as a string.

use splitz_ffi::{
    add_expense_entry, create_bill_entry, identity_key_from_seed, join_bill_entry, merge_entries,
    obligation_of, participant_id_for_key, pending_send_after, pending_send_blocks,
    pending_send_note, pending_send_records, set_rate_entry, HostFacts, PayerObligation, SendEnded,
    SplitzError,
};

const TXID: &str = "abababababababababababababababababababababababababababababababab";

struct Device {
    seed: String,
    key: String,
    me: String,
    minute: std::cell::Cell<u32>,
}

impl Device {
    fn new(byte: u8) -> Self {
        let seed = splitz_host::base64url_encode(&[byte; 32]);
        let key = identity_key_from_seed(seed.clone()).unwrap();
        let me = participant_id_for_key(key.clone()).unwrap();
        Self {
            seed,
            key,
            me,
            minute: std::cell::Cell::new(0),
        }
    }

    fn facts(&self) -> HostFacts {
        self.minute.set(self.minute.get() + 1);
        let total = 19 * 60 + 30 + self.minute.get();
        HostFacts {
            me: self.me.clone(),
            now: format!("2026-10-28T{:02}:{:02}:00.000Z", total / 60, total % 60),
            nonce: vec![self.seed.as_bytes()[0]; 16],
        }
    }
}

/// Ben owes Ana 45.00 of a 90.00 dinner, priced; Ben's log and his debt.
fn owing() -> (Device, String, Vec<String>, PayerObligation) {
    let ana = Device::new(1);
    let ben = Device::new(90);
    let create = create_bill_entry(
        ana.facts(),
        "Dinner".into(),
        "EUR".into(),
        "equal".into(),
        ana.key.clone(),
        ana.seed.clone(),
    )
    .unwrap();
    let bill_id = serde_json::from_str::<serde_json::Value>(&create).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let join = |d: &Device, name: &str, pay_to: &str| {
        join_bill_entry(
            d.facts(),
            bill_id.clone(),
            Some(name.into()),
            Some(pay_to.into()),
            Some(d.key.clone()),
            vec![],
            d.seed.clone(),
        )
        .unwrap()
    };
    let entries = vec![
        create,
        join(&ana, "Ana", "u1ana"),
        join(&ben, "Ben", "u1ben"),
        add_expense_entry(
            ana.facts(),
            bill_id.clone(),
            "x1".into(),
            ana.me.clone(),
            9000,
            format!(r#"{{"type":"equal","among":["{}","{}"]}}"#, ana.me, ben.me),
            None,
            ana.seed.clone(),
        )
        .unwrap(),
        set_rate_entry(
            ana.facts(),
            bill_id.clone(),
            "EUR".into(),
            300_000,
            None,
            ana.seed.clone(),
        )
        .unwrap(),
    ];
    let entries = merge_entries(vec![], entries).unwrap().entries;
    let owed = obligation_of(ben.facts(), bill_id.clone(), entries.clone())
        .unwrap()
        .expect("priced");
    assert_eq!(owed.settlements.len(), 1);
    (ben, bill_id, entries, owed)
}

#[test]
fn a_note_blocks_until_the_wallet_answers() {
    let (ben, bill_id, _, owed) = owing();
    assert_eq!(pending_send_blocks(bill_id.clone(), None), None);
    let note = pending_send_note(bill_id.clone(), owed, ben.facts().now).unwrap();
    let held = pending_send_blocks(bill_id.clone(), Some(note.clone())).expect("blocks");
    assert!(!held.damaged);
    assert!(held.uri.starts_with("zcash:u1ana"));

    let after = |how, txid: Option<&str>, recorded| {
        pending_send_after(
            bill_id.clone(),
            note.clone(),
            how,
            txid.map(str::to_owned),
            recorded,
        )
    };
    assert_eq!(after(SendEnded::Refused, Some(TXID), false), None);
    assert_eq!(after(SendEnded::ReachedNetwork, Some(TXID), true), None);
    let kept = after(SendEnded::ReachedNetwork, Some(TXID), false).expect("kept");
    let held = pending_send_blocks(bill_id.clone(), Some(kept)).unwrap();
    assert_eq!(held.txid.as_deref(), Some(TXID));
    assert_eq!(
        after(SendEnded::Unresolved, None, false),
        Some(note.clone())
    );
    let kept = after(SendEnded::Unresolved, Some(TXID), false).expect("kept");
    assert_eq!(
        pending_send_blocks(bill_id.clone(), Some(kept))
            .unwrap()
            .txid
            .as_deref(),
        Some(TXID)
    );
}

#[test]
fn a_note_that_does_not_read_still_blocks_and_is_kept_as_it_was() {
    let (ben, bill_id, _, owed) = owing();
    let other = pending_send_note("another-bill".into(), owed, ben.facts().now).unwrap();
    for raw in ["{not json", r#"{"billId":"x","uri":""}"#, other.as_str()] {
        let held = pending_send_blocks(bill_id.clone(), Some(raw.to_owned())).expect("blocks");
        assert!(held.damaged, "{raw}");
        assert_eq!(
            pending_send_after(
                bill_id.clone(),
                raw.to_owned(),
                SendEnded::Unresolved,
                Some(TXID.into()),
                false
            ),
            Some(raw.to_owned())
        );
    }
}

#[test]
fn records_come_from_the_note_once_and_only_for_a_transaction_id() {
    let (ben, bill_id, entries, owed) = owing();
    let note = pending_send_note(bill_id.clone(), owed, ben.facts().now).unwrap();
    let records = |entries: Vec<String>, note: &str, txid: &str| {
        pending_send_records(
            ben.facts(),
            bill_id.clone(),
            entries,
            note.to_owned(),
            txid.to_owned(),
            ben.seed.clone(),
        )
    };
    let written = records(
        entries.clone(),
        &note,
        &format!(" {} ", TXID.to_uppercase()),
    )
    .unwrap();
    assert_eq!(written.len(), 1);
    let record: serde_json::Value = serde_json::from_str(&written[0]).unwrap();
    assert_eq!(record["payment"]["reference"], TXID);
    assert!(record["payment"]["zatoshi"].as_i64().is_some());
    assert!(record["payment"]["paidAtRate"].is_object());

    let merged = merge_entries(entries.clone(), written).unwrap().entries;
    assert_eq!(records(merged, &note, TXID).unwrap(), Vec::<String>::new());

    let host = |r: Result<Vec<String>, SplitzError>| match r {
        Err(SplitzError::Host { detail, .. }) => detail,
        other => panic!("expected a Host error, got {other:?}"),
    };
    assert!(host(records(entries.clone(), &note, "tx-1")).contains("64 hexadecimal"));
    assert!(host(records(entries, "{not json", TXID)).contains("would not read"));
}

#[test]
fn a_note_is_refused_for_a_request_that_carries_nothing() {
    let (ben, bill_id, _, mut owed) = owing();
    owed.request.uri = None;
    assert!(matches!(
        pending_send_note(bill_id, owed, ben.facts().now),
        Err(SplitzError::Host { .. })
    ));
}
