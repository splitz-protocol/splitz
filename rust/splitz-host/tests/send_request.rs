//! The order a wallet's send keeps, and how a swap provider's answer is read.

use std::cell::Cell;

use splitz_core::host::ProposedOutput;
use splitz_core::{render_uri, Zip321Payment};
use splitz_host::{
    fee_raised_problem, proposal_problem, send_payment_request, send_reviewed_payment_request,
    swap_answer, HostError, WalletSendOutcome, WalletSendPhase,
};

fn request() -> String {
    render_uri(
        &[
            Zip321Payment {
                address: "u1ana".into(),
                zatoshi: 7004,
                ..Default::default()
            },
            Zip321Payment {
                address: "u1ben".into(),
                zatoshi: 9246,
                ..Default::default()
            },
        ],
        false,
    )
    .unwrap()
}

fn out(address: &str, zatoshi: i64) -> ProposedOutput {
    ProposedOutput {
        address: address.into(),
        zatoshi,
    }
}

fn sent() -> WalletSendOutcome {
    WalletSendOutcome {
        phase: WalletSendPhase::Succeeded,
        txid: Some("cd34".into()),
        status_message: None,
        error: None,
    }
}

#[test]
fn what_the_wallet_read_is_held_against_the_request() {
    let uri = request();
    assert_eq!(
        proposal_problem(&uri, &[out("u1ben", 9246), out("u1ana", 7004)]),
        None
    );
    assert!(proposal_problem(&uri, &[out("u1ana", 7004)])
        .unwrap()
        .contains("Nothing was sent"));
    assert_eq!(
        proposal_problem("zcash:u1ana?amount=1&foo=bar", &[out("u1ana", 100_000_000)]),
        splitz_core::describe_code("zip321_not_canonical").map(str::to_owned)
    );
}

#[test]
fn a_request_read_differently_builds_nothing() {
    let proposed = Cell::new(0);
    let outcome = send_payment_request(
        &request(),
        || Ok(vec![out("u1ana", 7004)]),
        || {
            proposed.set(proposed.get() + 1);
            Ok(1)
        },
        |_: i32| sent(),
    );
    assert_eq!(outcome.phase, WalletSendPhase::Failed);
    assert!(outcome.error.unwrap().contains("Nothing was sent"));
    assert_eq!(proposed.get(), 0);
}

#[test]
fn a_reader_or_builder_that_failed_is_a_failure_not_a_send_that_may_land() {
    let read_failed = send_payment_request(
        &request(),
        || Err("unreadable".into()),
        || -> Result<i32, String> { panic!("never proposed") },
        |_| panic!("never broadcast"),
    );
    assert_eq!(read_failed.phase, WalletSendPhase::Failed);
    assert!(read_failed.error.unwrap().contains("unreadable"));

    let broadcasts = Cell::new(0);
    let refused = send_payment_request(
        &request(),
        || Ok(vec![out("u1ana", 7004), out("u1ben", 9246)]),
        || -> Result<i32, String> { Err("insufficient funds".into()) },
        |_| {
            broadcasts.set(broadcasts.get() + 1);
            sent()
        },
    );
    assert_eq!(refused.phase, WalletSendPhase::Failed);
    assert!(refused.error.unwrap().contains("insufficient funds"));
    assert_eq!(broadcasts.get(), 0);
}

#[test]
fn the_broadcasts_own_outcome_is_returned_as_it_is() {
    let outcome = send_payment_request(
        &request(),
        || Ok(vec![out("u1ana", 7004), out("u1ben", 9246)]),
        || Ok(1),
        |_: i32| WalletSendOutcome {
            phase: WalletSendPhase::PendingBroadcast,
            txid: Some("ab12".into()),
            status_message: Some("stored, not broadcast".into()),
            error: None,
        },
    );
    assert_eq!(outcome.phase, WalletSendPhase::PendingBroadcast);
    assert_eq!(outcome.txid.as_deref(), Some("ab12"));
}

fn refusal(status: u16, body: &str) -> (String, bool) {
    match swap_answer(status, body.as_bytes()) {
        Err(HostError::Swap { message, transient }) => (message, transient),
        other => panic!("expected a swap refusal, got {other:?}"),
    }
}

#[test]
fn a_swap_answer_is_read_by_its_status_first() {
    assert_eq!(
        swap_answer(200, br#"{"quote":1}"#).unwrap(),
        r#"{"quote":1}"#
    );
    assert_eq!(swap_answer(399, b"ok").unwrap(), "ok");
    assert_eq!(
        refusal(400, r#"{"error":"no route"}"#),
        ("The swap provider answered 400".into(), false)
    );
    assert_eq!(
        refusal(
            400,
            r#"{"message":"slippageTolerance should not be empty","statusCode":400}"#
        )
        .0,
        "The swap provider answered 400: slippageTolerance should not be empty"
    );
    assert!(refusal(503, "upstream down").1);
    assert!(swap_answer(200, &[0xff, 0xfe, 0x41]).unwrap().contains('A'));
}

#[test]
fn a_raised_fee_is_refused_in_words_and_an_equal_or_lower_one_is_not() {
    assert_eq!(
        fee_raised_problem(10000, 15000).as_deref(),
        Some(
            "The fee went up from 0.0001 ZEC to 0.00015 ZEC since you reviewed this \
             payment. Nothing was sent; review it again."
        )
    );
    assert!(fee_raised_problem(10000, 10001).is_some());
    assert_eq!(fee_raised_problem(10000, 10000), None);
    assert_eq!(fee_raised_problem(10000, 5000), None);
}

#[test]
fn a_fee_figure_no_amount_renders_is_said_in_zatoshi() {
    assert!(fee_raised_problem(0, 1)
        .unwrap()
        .starts_with("The fee went up from 0 zatoshi to 0.00000001 ZEC"));
    assert!(fee_raised_problem(-5, splitz_core::MAX_ZATOSHI + 1)
        .unwrap()
        .contains("-5 zatoshi to 2100000000000001 zatoshi"));
}

fn reviewed_send(fee: i64, read: Vec<ProposedOutput>) -> (WalletSendOutcome, Vec<String>) {
    let calls = std::cell::RefCell::new(Vec::new());
    let outcome = send_reviewed_payment_request(
        &request(),
        10000,
        || Ok(read),
        || {
            calls.borrow_mut().push("propose".to_owned());
            Ok((7, fee))
        },
        |p: i32| calls.borrow_mut().push(format!("discard {p}")),
        |p: i32| {
            calls.borrow_mut().push(format!("broadcast {p}"));
            sent()
        },
    );
    (outcome, calls.into_inner())
}

#[test]
fn a_raised_fee_discards_the_proposal_and_broadcasts_nothing() {
    let both = vec![out("u1ben", 9246), out("u1ana", 7004)];
    let (outcome, calls) = reviewed_send(10001, both);
    assert_eq!(outcome.phase, WalletSendPhase::Failed);
    assert!(outcome.error.unwrap().contains("Nothing was sent"));
    assert_eq!(calls, ["propose", "discard 7"]);
}

#[test]
fn the_reviewed_fee_or_less_is_broadcast() {
    for fee in [10000, 9999, 0] {
        let both = vec![out("u1ben", 9246), out("u1ana", 7004)];
        let (outcome, calls) = reviewed_send(fee, both);
        assert_eq!(outcome.phase, WalletSendPhase::Succeeded, "{fee}");
        assert_eq!(calls, ["propose", "broadcast 7"]);
    }
}

#[test]
fn a_request_read_differently_builds_nothing_to_discard() {
    let (outcome, calls) = reviewed_send(10000, vec![out("u1ana", 7004)]);
    assert_eq!(outcome.phase, WalletSendPhase::Failed);
    assert!(calls.is_empty());
}
