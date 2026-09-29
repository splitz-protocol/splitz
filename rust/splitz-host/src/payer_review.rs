//! §14.2's rule, run against the text a wallet's review screen shows a payer.
//!
//! A wallet renders its own review screen and extracts what a person can read
//! on it — however it does that — as a list of strings.
//! [`check_payer_review`] derives every fact §14.2 requires from the
//! obligation about to be sent and the bill it came from, and answers each one
//! that text does not show. An empty answer is the only passing one.
//!
//! **How a fact must appear.** The strings are joined with a line break, so
//! no fact is found straddling two of them, and each fact is looked for as a
//! case-sensitive substring. Case-sensitive because a name is shown as the
//! participant wrote it and an address is case-significant; matching loosely
//! would pass a screen that shows a different string. Per fact:
//!
//! - A participant is shown by their display name from the bill, or by their
//!   id when the bill has no participant under it.
//! - An unpayable recipient's reason is shown by the wallet's own words for
//!   it, from `reason_words`, keyed by the reason code §8.5 gives
//!   (`no_address`, `bad_address`, `payout_not_zec`, `unpriceable`). A code
//!   with no entry there is a finding: the kit cannot tell which words carry
//!   it.
//! - A recipient paid by a lower preference (§14.8) is shown by their name
//!   and by `lower_words`, the wallet's own words for that. Empty words are a
//!   finding, for the same reason.
//! - A ZEC amount is [`render_amount`]'s text (§8.1: no trailing zeros, `.` as
//!   the decimal point, no grouping), not touching a digit on either side and
//!   not followed by `.` and a digit, so `0.1` is not found inside `0.12`.
//! - The rate is [`rate_figure`]'s text, under the same digit rule.
//! - An address is shown whole, or by a prefix of at least 10 characters that
//!   ends where the text stops agreeing with the address on a character that
//!   is not an ASCII letter or digit — an ellipsis, a space, the end of a
//!   line. A different address sharing the first 10 characters does not
//!   count.
//!
//! **What this cannot see.** Substrings prove a fact is on the screen, not
//! that it sits beside the person it belongs to: one reason shown once
//! satisfies two recipients with that reason, and a name is found inside a
//! longer word. Nor does it see what is scrolled away, clipped or drawn in a
//! colour nobody can read; the list is whatever the wallet hands over.

use std::collections::{BTreeMap, BTreeSet};

use splitz_core::host::{FoldedBill, PayerObligation};
use splitz_core::{render_amount, ExchangeRate};

use crate::currencies::currency_exponent;

/// Which of §14.2's facts a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRule {
    /// Every recipient the request cannot carry, with the reason (§8.5).
    Unpayable,
    /// Every pay-to address the fold recorded as replaced (§10.3).
    ReplacedAddress,
    /// Every debt with a payment recorded and not yet confirmed (§10.5,
    /// §14.4).
    Awaiting,
    /// Every recipient paid by a preference other than their first (§14.8).
    LowerPreference,
    /// The rate the request was priced at, and who set it.
    Rate,
    /// The ZEC amount and address of every output.
    Output,
}

/// One fact §14.2 requires that the review screen's text does not show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewFinding {
    pub rule: ReviewRule,
    /// The fact, in words: whose, and which part of it.
    pub fact: String,
    /// The text looked for and not found.
    pub expected: String,
}

/// The rate figure a review screen shows: `rate`'s minor units per ZEC in the
/// currency's major units, `.` as the decimal point and every fractional
/// digit its ISO 4217 exponent gives (`51234` EUR is `512.34`). A currency the
/// register gives no exponent is shown as its minor units unchanged.
pub fn rate_figure(rate: &ExchangeRate) -> String {
    let units = rate.minor_units_per_zec;
    let exponent = currency_exponent(&rate.currency).unwrap_or(0) as usize;
    if exponent == 0 {
        return units.to_string();
    }
    let digits = format!("{:0>width$}", units.unsigned_abs(), width = exponent + 1);
    let (whole, fraction) = digits.split_at(digits.len() - exponent);
    let sign = if units < 0 { "-" } else { "" };
    format!("{sign}{whole}.{fraction}")
}

/// §14.2's facts for `obligation` on `folded`, against `visible_text`.
///
/// `reason_words` maps each §8.5 reason code to the words the screen uses for
/// it. `via` is the payer's choice of payouts the obligation was rendered with
/// (`obligation_via`), and `lower_words` the screen's words for a recipient
/// paid by one other than their first; a choice for somebody the obligation
/// does not pay needs nothing shown. Findings come in the order of §14.2's list; within a rule, in the order
/// the obligation or the fold gives the facts. Refused only for an output
/// whose zatoshi §8.1 cannot render, which no rendered obligation carries.
pub fn check_payer_review(
    obligation: &PayerObligation,
    folded: &FoldedBill,
    visible_text: &[String],
    reason_words: &BTreeMap<String, String>,
    via: &BTreeMap<String, i64>,
    lower_words: &str,
) -> splitz_core::Result<Vec<ReviewFinding>> {
    let text = visible_text.join("\n");
    let names: BTreeMap<&str, &str> = folded
        .bill
        .participants
        .iter()
        .map(|p| (p.id.as_str(), p.name.as_str()))
        .collect();
    let name = |id: &str| names.get(id).copied().unwrap_or(id).to_owned();
    let mut out = Vec::new();
    let mut need = |rule: ReviewRule, fact: String, expected: String, shown: bool| {
        if !shown {
            out.push(ReviewFinding {
                rule,
                fact,
                expected,
            });
        }
    };

    for u in obligation.unpayable() {
        let who = name(&u.id);
        let shown = text.contains(&who);
        need(
            ReviewRule::Unpayable,
            "who the request cannot pay".to_owned(),
            who.clone(),
            shown,
        );
        let words = reason_words.get(u.reason).filter(|w| !w.is_empty());
        need(
            ReviewRule::Unpayable,
            format!("why {who} cannot be paid ({})", u.reason),
            words.cloned().unwrap_or_else(|| u.reason.to_owned()),
            words.is_some_and(|w| text.contains(w.as_str())),
        );
    }

    let mut replaced = BTreeSet::new();
    for r in &folded.replaced_addresses {
        if !replaced.insert(r.id.as_str()) {
            continue;
        }
        let who = name(&r.id);
        let shown = text.contains(&who);
        need(
            ReviewRule::ReplacedAddress,
            "whose pay-to address was replaced".to_owned(),
            who,
            shown,
        );
    }

    let mut pending = BTreeSet::new();
    for a in &obligation.awaiting {
        for id in std::iter::once(&a.to).chain(&a.paid_to) {
            if !pending.insert(id.as_str()) {
                continue;
            }
            let who = name(id);
            let shown = text.contains(&who);
            need(
                ReviewRule::Awaiting,
                "who a payment awaiting confirmation went to".to_owned(),
                who,
                shown,
            );
        }
    }

    let paid: BTreeSet<&str> = obligation
        .settlements
        .iter()
        .map(|s| s.to.as_str())
        .collect();
    for (id, &index) in via {
        if index == 0 || !paid.contains(id.as_str()) {
            continue;
        }
        let who = name(id);
        let shown = text.contains(&who);
        need(
            ReviewRule::LowerPreference,
            "who is paid by a lower preference".to_owned(),
            who.clone(),
            shown,
        );
        need(
            ReviewRule::LowerPreference,
            format!("that {who} is paid by a lower preference"),
            if lower_words.is_empty() {
                "lower_preference".to_owned()
            } else {
                lower_words.to_owned()
            },
            !lower_words.is_empty() && text.contains(lower_words),
        );
    }

    let figure = rate_figure(&obligation.rate);
    let shown = shows_number(&text, &figure);
    need(
        ReviewRule::Rate,
        "the rate the request was priced at".to_owned(),
        figure,
        shown,
    );
    if let Some(author) = &folded.rate_author {
        let who = name(author);
        let shown = text.contains(&who);
        need(ReviewRule::Rate, "who set the rate".to_owned(), who, shown);
    }

    let request = &obligation.request;
    for (payment, to) in request.payments.iter().zip(&request.recipients) {
        let who = name(to);
        let amount = render_amount(payment.zatoshi)?;
        let shown = shows_number(&text, &amount);
        need(
            ReviewRule::Output,
            format!("the ZEC sent to {who}"),
            amount,
            shown,
        );
        need(
            ReviewRule::Output,
            format!("the address {who} is paid at"),
            payment.address.clone(),
            shows_address(&text, &payment.address),
        );
    }
    Ok(out)
}

/// Every place `pattern` starts in `text`, overlapping ones included.
fn occurrences<'t>(text: &'t str, pattern: &'t str) -> impl Iterator<Item = usize> + 't {
    let mut from = 0;
    std::iter::from_fn(move || {
        let at = from + text.get(from..)?.find(pattern)?;
        from = at + text[at..].chars().next().map_or(1, char::len_utf8);
        Some(at)
    })
}

/// True when `number` occurs in `text` with no digit touching it, and not
/// followed by `.` and a digit.
fn shows_number(text: &str, number: &str) -> bool {
    let bytes = text.as_bytes();
    occurrences(text, number).any(|at| {
        let end = at + number.len();
        let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
        !(at > 0 && digit(at - 1))
            && !digit(end)
            && !(bytes.get(end) == Some(&b'.') && digit(end + 1))
    })
}

/// The shortest prefix of an address a screen may show in its place.
const ADDRESS_PREFIX_LENGTH: usize = 10;

/// True when `address` occurs in `text` whole, or as a prefix of at least 10
/// characters that stops agreeing with it on a character that is not an
/// ASCII letter or digit.
fn shows_address(text: &str, address: &str) -> bool {
    let Some(head) = address.get(..ADDRESS_PREFIX_LENGTH) else {
        return text.contains(address);
    };
    if address.len() == ADDRESS_PREFIX_LENGTH {
        return text.contains(address);
    }
    let bytes = text.as_bytes();
    let wanted = address.as_bytes();
    occurrences(text, head).any(|at| {
        let mut n = ADDRESS_PREFIX_LENGTH;
        while n < wanted.len() && bytes.get(at + n) == Some(&wanted[n]) {
            n += 1;
        }
        n == wanted.len() || !bytes.get(at + n).is_some_and(u8::is_ascii_alphanumeric)
    })
}
