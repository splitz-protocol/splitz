//! §14.2's review-screen check, against a screen that shows every fact and
//! against the same screen with each fact taken away in turn.

use std::cell::Cell;
use std::collections::BTreeMap;

use serde_json::{json, Value};
use splitz_core::host::{
    add_expense, base64url_no_pad, create_bill, join_bill, obligation_via, record_payment,
    set_rate, BillHost, BillLog, FoldedBill, PayerObligation, Sent, CREATOR_KEY_BYTES,
};
use splitz_core::ExchangeRate;
use splitz_host::{check_payee_review, check_payer_review, rate_figure, ReviewRule};

const BEN_OLD: &str = "u1benold0000000000000000";
const BEN: &str = "u1ben1111111111111111111";
const DAN: &str = "u1dan3333333333333333333";
const EVE: &str = "u1eve4444444444444444444";
const EVE_LATER: &str = "u1eve5555555555555555555";

/// A host speaking as `me` on a clock every host of the test shares.
struct Host<'c> {
    me: &'static str,
    minute: &'c Cell<u32>,
    counter: Cell<u8>,
}

impl BillHost for Host<'_> {
    fn me(&self) -> &str {
        self.me
    }
    fn now(&self) -> String {
        self.minute.set(self.minute.get() + 1);
        let total = 19 * 60 + 30 + self.minute.get();
        format!("2026-10-28T{:02}:{:02}:00Z", total / 60, total % 60)
    }
    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.counter.set(self.counter.get().wrapping_add(1));
        (0..byte_count)
            .map(|i| self.counter.get().wrapping_add(i as u8))
            .collect()
    }
    fn broadcast(&self, _: &str) -> Sent {
        unreachable!("nothing is sent")
    }
}

/// Ana owes Ben 40.00, Cat 30.00, Dan 20.00 and Eve 10.00 (EUR). Ben moved
/// his address, Cat has none, Ana has recorded paying Dan, and Eve set the
/// rate. The request Ana sends carries Ben and Eve. Eve declares a second
/// address after her first, which `via` can choose (§14.8).
fn bill() -> (PayerObligation, FoldedBill) {
    bill_via(&BTreeMap::new())
}

fn bill_via(via: &BTreeMap<String, i64>) -> (PayerObligation, FoldedBill) {
    let minute = Cell::new(0);
    let host = |me: &'static str| Host {
        me,
        minute: &minute,
        counter: Cell::new(me.as_bytes()[0]),
    };
    let (ana, ben, cat, dan, eve) = (
        host("ana"),
        host("ben"),
        host("cat"),
        host("dan"),
        host("eve"),
    );
    let key = base64url_no_pad(&(0..CREATOR_KEY_BYTES as u8).collect::<Vec<u8>>());
    let spent = |who: &Host<'_>, amount: i64, id: &str| -> Value {
        let split = json!({"type": "equal", "among": ["ana", who.me]});
        add_expense(who, id, who.me, amount, split, None).unwrap()
    };
    let entries = vec![
        create_bill(&ana, "Trip", "EUR", "equal", &key, None).unwrap(),
        join_bill(&ana, Some("Ana"), Some("u1ana0000000000000"), None, None).unwrap(),
        join_bill(&ben, Some("Ben"), Some(BEN_OLD), None, None).unwrap(),
        join_bill(&cat, Some("Cat"), None, None, None).unwrap(),
        join_bill(&dan, Some("Dan"), Some(DAN), None, None).unwrap(),
        join_bill(
            &eve,
            Some("Eve"),
            None,
            None,
            Some(vec![
                json!({"type": "zec", "address": EVE}),
                json!({"type": "zec", "address": EVE_LATER}),
            ]),
        )
        .unwrap(),
        join_bill(&ben, Some("Ben"), Some(BEN), None, None).unwrap(),
        spent(&ben, 8000, "x1"),
        spent(&cat, 6000, "x2"),
        spent(&dan, 4000, "x3"),
        spent(&eve, 2000, "x4"),
        record_payment(
            &ana,
            "p1",
            "dan",
            2000,
            "shieldedZec",
            None,
            None,
            None,
            None,
        )
        .unwrap(),
        set_rate(&eve, "EUR", 51234, None).unwrap(),
    ];
    let mut log = BillLog::new(&ana);
    assert!(log.add(entries).unwrap().is_empty());
    let folded = log.fold().unwrap();
    assert!(folded.set_aside.is_empty(), "{:?}", folded.set_aside);
    let obligation = obligation_via(&ana, &folded, via).unwrap().expect("priced");
    (obligation, folded)
}

fn reasons() -> BTreeMap<String, String> {
    BTreeMap::from([("no_address".to_owned(), "has no address".to_owned())])
}

/// One line per fact, so taking a line away takes exactly one fact away.
fn screen() -> Vec<String> {
    [
        "Cat",            // unpayable: who
        "has no address", // unpayable: why
        "Ben",            // replaced address
        "Dan",            // awaiting
        "512.34",         // rate figure
        "0.07807316",     // Ben's output
        "u1ben11111…",    // Ben's address, the first 10 characters
        "0.01951829",     // Eve's output
        EVE,              // Eve's address, whole
    ]
    .map(str::to_owned)
    .to_vec()
}

fn found(shown: &[String], reasons: &BTreeMap<String, String>) -> Vec<(ReviewRule, String)> {
    let (obligation, folded) = bill();
    check_payer_review(
        &obligation,
        &folded,
        shown,
        reasons,
        &BTreeMap::new(),
        "",
        "",
    )
    .unwrap()
    .into_iter()
    .map(|f| (f.rule, f.expected))
    .collect()
}

#[test]
fn the_facts_are_the_ones_this_bill_produces() {
    let (obligation, folded) = bill();
    let unpayable: Vec<_> = obligation
        .unpayable()
        .iter()
        .map(|u| (u.id.as_str(), u.reason))
        .collect();
    assert_eq!(unpayable, [("cat", "no_address")]);
    let replaced: Vec<_> = folded
        .replaced_addresses
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(replaced, ["ben"]);
    let awaiting: Vec<_> = obligation.awaiting.iter().map(|a| a.to.as_str()).collect();
    assert_eq!(awaiting, ["dan"]);
    assert_eq!(folded.rate_author.as_deref(), Some("eve"));
    assert_eq!(obligation.request.recipients, ["ben", "eve"]);
    let payments: Vec<_> = obligation
        .request
        .payments
        .iter()
        .map(|p| (p.address.as_str(), p.zatoshi))
        .collect();
    // §7: a settlement rounds up. 4000e8 / 51234 = 7807315.45…, and
    // 1000e8 / 51234 = 1951828.86….
    assert_eq!(payments, [(BEN, 7_807_316), (EVE, 1_951_829)]);
}

#[test]
fn a_screen_showing_every_fact_passes() {
    assert_eq!(found(&screen(), &reasons()), []);
}

#[test]
fn each_fact_taken_away_is_exactly_its_own_finding() {
    let expected = [
        (ReviewRule::Unpayable, "Cat"),
        (ReviewRule::Unpayable, "has no address"),
        (ReviewRule::ReplacedAddress, "Ben"),
        (ReviewRule::Awaiting, "Dan"),
        (ReviewRule::Rate, "512.34"),
        (ReviewRule::Output, "0.07807316"),
        (ReviewRule::Output, BEN),
        (ReviewRule::Output, "0.01951829"),
        (ReviewRule::Output, EVE),
    ];
    for (i, (rule, text)) in expected.iter().enumerate() {
        let mut shown = screen();
        let removed = shown.remove(i);
        assert_eq!(
            found(&shown, &reasons()),
            [(*rule, (*text).to_owned())],
            "without {removed:?}"
        );
    }
}

#[test]
fn an_amount_inside_a_longer_number_is_not_shown() {
    let mut shown = screen();
    shown[5] = "0.078073169".to_owned();
    assert_eq!(
        found(&shown, &reasons()),
        [(ReviewRule::Output, "0.07807316".to_owned())]
    );
}

#[test]
fn a_different_address_sharing_the_first_ten_characters_is_not_shown() {
    let mut shown = screen();
    shown[6] = "u1ben1111122222…".to_owned();
    assert_eq!(
        found(&shown, &reasons()),
        [(ReviewRule::Output, BEN.to_owned())]
    );
}

#[test]
fn a_reason_the_wallet_gives_no_words_for_is_a_finding() {
    assert_eq!(
        found(&screen(), &BTreeMap::new()),
        [(ReviewRule::Unpayable, "no_address".to_owned())]
    );
}

#[test]
fn the_rate_figure_keeps_the_currencys_fractional_digits() {
    let rate = |currency: &str, units: i64| ExchangeRate {
        currency: currency.to_owned(),
        minor_units_per_zec: units,
        at: "2026-10-28T19:30:00.000Z".to_owned(),
        source: None,
    };
    assert_eq!(rate_figure(&rate("EUR", 51234)), "512.34");
    assert_eq!(rate_figure(&rate("EUR", 5)), "0.05");
    assert_eq!(rate_figure(&rate("EUR", 5000)), "50.00");
    assert_eq!(rate_figure(&rate("JPY", 7000)), "7000");
    assert_eq!(rate_figure(&rate("BHD", 12345)), "12.345");
}

const LOWER: &str = "by a later choice";

/// Eve paid at her second address: the screen of every fact, with her
/// address swapped and the wallet's words for a lower preference added.
fn lower_found(
    via: &BTreeMap<String, i64>,
    checked: &BTreeMap<String, i64>,
    shown: &[String],
    words: &str,
) -> Vec<(ReviewRule, String)> {
    let (obligation, folded) = bill_via(via);
    check_payer_review(&obligation, &folded, shown, &reasons(), checked, words, "")
        .unwrap()
        .into_iter()
        .map(|f| (f.rule, f.expected))
        .collect()
}

fn lower_screen() -> Vec<String> {
    let mut shown = screen();
    shown[8] = EVE_LATER.to_owned();
    shown.push("Eve".to_owned());
    shown.push(LOWER.to_owned());
    shown
}

fn eve_second() -> BTreeMap<String, i64> {
    BTreeMap::from([("eve".to_owned(), 1)])
}

#[test]
fn a_recipient_paid_by_a_lower_preference_is_shown_with_the_wallets_words() {
    let via = eve_second();
    let (obligation, _) = bill_via(&via);
    assert_eq!(obligation.request.payments[1].address, EVE_LATER);
    assert_eq!(lower_found(&via, &via, &lower_screen(), LOWER), []);

    let mut shown = lower_screen();
    shown.pop();
    assert_eq!(
        lower_found(&via, &via, &shown, LOWER),
        [(ReviewRule::LowerPreference, LOWER.to_owned())]
    );
    assert_eq!(
        lower_found(&via, &via, &lower_screen(), ""),
        [(ReviewRule::LowerPreference, "lower_preference".to_owned())]
    );
}

#[test]
fn a_lower_preference_names_the_recipient() {
    // Her name is on the screen as the recipient paid by a lower choice.
    let via = eve_second();
    let shown: Vec<String> = lower_screen().into_iter().filter(|l| l != "Eve").collect();
    assert_eq!(
        lower_found(&via, &via, &shown, LOWER),
        [(ReviewRule::LowerPreference, "Eve".to_owned())]
    );
}

#[test]
fn a_first_choice_or_somebody_not_paid_needs_nothing_shown() {
    let mut shown = screen();
    shown.retain(|l| l != LOWER);
    let chosen = BTreeMap::from([("eve".to_owned(), 0), ("dan".to_owned(), 1)]);
    assert_eq!(lower_found(&BTreeMap::new(), &chosen, &shown, ""), []);
}

const TXID: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809";

fn record(
    method: &str,
    zatoshi: Option<i64>,
    rate: bool,
    reference: Option<&str>,
) -> splitz_core::PaymentRecord {
    splitz_core::PaymentRecord {
        id: "p1".to_owned(),
        from: "ana".to_owned(),
        to: "ben".to_owned(),
        amount: 4000,
        currency: "EUR".to_owned(),
        method: method.to_owned(),
        at: "2026-10-28T19:30:00.000Z".to_owned(),
        zatoshi,
        paid_at_rate: rate.then(|| ExchangeRate {
            currency: "EUR".to_owned(),
            minor_units_per_zec: 51234,
            at: "2026-10-28T19:30:00.000Z".to_owned(),
            source: None,
        }),
        reference: reference.map(str::to_owned),
        note: None,
    }
}

fn payee_screen() -> Vec<String> {
    [
        "Ana sent 0.07807316 ZEC",
        "at 512.34 EUR a ZEC",
        "tx 1a2b3c4d5e6f…c4d5e6f809",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn payee(
    payment: &splitz_core::PaymentRecord,
    shown: &[String],
    absent: &str,
) -> Vec<(ReviewRule, String)> {
    check_payee_review(payment, shown, absent)
        .unwrap()
        .into_iter()
        .map(|f| (f.rule, f.expected))
        .collect()
}

#[test]
fn a_payee_screen_showing_the_records_figures_passes_and_each_missing_one_is_named() {
    let zec = record("shieldedZec", Some(7_807_316), true, Some(TXID));
    assert_eq!(payee(&zec, &payee_screen(), ""), []);
    let expected = [
        (ReviewRule::PayeeZec, "0.07807316"),
        (ReviewRule::PayeeRate, "512.34"),
        (ReviewRule::PayeeReference, TXID),
    ];
    for (i, (rule, text)) in expected.iter().enumerate() {
        let mut shown = payee_screen();
        let removed = shown.remove(i);
        assert_eq!(
            payee(&zec, &shown, ""),
            [(*rule, (*text).to_owned())],
            "without {removed:?}"
        );
    }
}

#[test]
fn a_different_reference_sharing_its_first_ten_characters_is_not_shown() {
    let zec = record("shieldedZec", Some(7_807_316), true, Some(TXID));
    let mut shown = payee_screen();
    shown[2] = "tx 1a2b3c4d5e00…".to_owned();
    assert_eq!(
        payee(&zec, &shown, ""),
        [(ReviewRule::PayeeReference, TXID.to_owned())]
    );
}

#[test]
fn a_zec_record_missing_a_figure_must_say_so_in_the_wallets_words() {
    let bare = record("shieldedZec", Some(7_807_316), false, None);
    let zec_only = vec!["Ana sent 0.07807316 ZEC".to_owned()];
    let mut said = zec_only.clone();
    said.push("rate and reference: not recorded".to_owned());
    assert_eq!(payee(&bare, &said, "not recorded"), []);
    assert_eq!(
        payee(&bare, &zec_only, "not recorded"),
        [
            (ReviewRule::PayeeRate, "not recorded".to_owned()),
            (ReviewRule::PayeeReference, "not recorded".to_owned()),
        ]
    );
    assert_eq!(
        payee(&bare, &said, ""),
        [
            (ReviewRule::PayeeRate, "absent".to_owned()),
            (ReviewRule::PayeeReference, "absent".to_owned()),
        ]
    );
}

#[test]
fn a_cash_record_needs_nothing_shown() {
    let cash = record("cash", None, false, None);
    assert_eq!(payee(&cash, &[], ""), []);
}

/// What a swap record with no ZEC or rate says in their place.
const NONE: &str = "ZEC and rate: not recorded";

#[test]
fn a_reference_prefix_counts_characters_not_bytes() {
    let fifteen = "é".repeat(15);
    let swap = record("swap", None, false, Some(&fifteen));
    let five = vec![format!("ref {}… ok", "é".repeat(5)), NONE.to_owned()];
    assert_eq!(
        payee(&swap, &five, "not recorded"),
        [(ReviewRule::PayeeReference, fifteen.clone())]
    );
    let ten = vec![format!("ref {}… ok", "é".repeat(10)), NONE.to_owned()];
    assert_eq!(payee(&swap, &ten, "not recorded"), []);
    let mixed = record("swap", None, false, Some("abcdefghiéxyzxyz"));
    assert_eq!(
        payee(
            &mixed,
            &["ref abcdefghié…".to_owned(), NONE.to_owned()],
            "not recorded"
        ),
        []
    );
    let emoji_ref = format!("{}x", "😀".repeat(12));
    let emoji = record("swap", None, false, Some(&emoji_ref));
    assert_eq!(
        payee(
            &emoji,
            &[format!("ref {}…", "😀".repeat(5)), NONE.to_owned()],
            "not recorded"
        ),
        [(ReviewRule::PayeeReference, emoji_ref.clone())]
    );
}

#[test]
fn a_payment_the_bill_does_not_explain_names_who_and_says_so() {
    // §6's fabricated refund: Ben writes a refund of Ana's dinner onto
    // himself, so he owes her 100.00 where the bill's debts explain 50.00.
    let minute = Cell::new(0);
    let host = |me: &'static str| Host {
        me,
        minute: &minute,
        counter: Cell::new(me.as_bytes()[0]),
    };
    let (ana, ben) = (host("ana"), host("ben"));
    let key = base64url_no_pad(&(0..CREATOR_KEY_BYTES as u8).collect::<Vec<u8>>());
    let both = json!({"type": "equal", "among": ["ana", "ben"]});
    let entries = vec![
        create_bill(&ana, "Dinner", "EUR", "equal", &key, None).unwrap(),
        join_bill(&ana, Some("Ana"), Some("u1ana0000000000000"), None, None).unwrap(),
        join_bill(&ben, Some("Ben"), Some(BEN), None, None).unwrap(),
        add_expense(&ana, "e1", "ana", 10_000, both.clone(), None).unwrap(),
        add_expense(&ben, "e2", "ben", -10_000, both, None).unwrap(),
        set_rate(&ana, "EUR", 51_234, None).unwrap(),
    ];
    let mut log = BillLog::new(&ana);
    log.add(entries).unwrap();
    let folded = log.fold().unwrap();
    let owed = obligation_via(&ben, &folded, &BTreeMap::new())
        .unwrap()
        .unwrap();
    assert_eq!(owed.settlements[0].unexplained(), 5000);

    let words = "more than the bill explains";
    let unexplained = |shown: &[&str], words: &str| -> Vec<String> {
        let shown: Vec<String> = shown.iter().map(|s| s.to_string()).collect();
        check_payer_review(
            &owed,
            &folded,
            &shown,
            &BTreeMap::new(),
            &BTreeMap::new(),
            "",
            words,
        )
        .unwrap()
        .into_iter()
        .filter(|f| f.rule == ReviewRule::Unexplained)
        .map(|f| f.expected)
        .collect()
    };
    assert!(unexplained(&["Ana", words], words).is_empty());
    assert_eq!(unexplained(&["Ana"], words), vec![words.to_owned()]);
    assert_eq!(unexplained(&[words], words), vec!["Ana".to_owned()]);
    assert_eq!(unexplained(&["Ana"], ""), vec!["unexplained".to_owned()]);
}

#[test]
fn a_short_form_is_whole_or_ten_characters_and_an_ellipsis() {
    use splitz_host::short_form;
    assert_eq!(short_form("ab"), "ab");
    assert_eq!(short_form("123456789012"), "123456789012");
    assert_eq!(short_form("1234567890123"), "1234567890…");
    assert_eq!(
        short_form(&"😀".repeat(13)),
        format!("{}…", "😀".repeat(10))
    );
    assert_eq!(short_form(BEN), format!("{}…", &BEN[..10]));
}
