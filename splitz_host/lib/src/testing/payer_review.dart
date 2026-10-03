/// §14.2's rule, run against the text a wallet's review screen shows a payer.
///
/// A wallet renders its own review screen and extracts what a person can read
/// on it — however it does that — as a list of strings. [checkPayerReview]
/// derives every fact §14.2 requires from the obligation about to be sent and
/// the bill it came from, and answers each one that text does not show. An
/// empty answer is the only passing one.
///
/// **How a fact must appear.** The strings are joined with a line break, so
/// no fact is found straddling two of them, and each fact is looked for as a
/// case-sensitive substring. Case-sensitive because a name is shown as the
/// participant wrote it and an address is case-significant; matching loosely
/// would pass a screen that shows a different string. Per fact:
///
/// - A participant is shown by their display name from the bill, or by their
///   id when the bill has no participant under it.
/// - An unpayable recipient's reason is shown by the wallet's own words for
///   it, from `reasonWords`, keyed by the reason code §8.5 gives
///   (`no_address`, `bad_address`, `payout_not_zec`, `unpriceable`). A code
///   with no entry there is a finding: the kit cannot tell which words carry
///   it.
/// - A recipient paid by a lower preference (§14.8) is shown by their name
///   and by `lowerWords`, the wallet's own words for that. Empty words are a
///   finding, for the same reason.
/// - A ZEC amount is [renderAmount]'s text (§8.1: no trailing zeros, `.` as
///   the decimal point, no grouping), not touching a digit on either side and
///   not followed by `.` and a digit, so `0.1` is not found inside `0.12`.
/// - The rate is [rateFigure]'s text, under the same digit rule.
/// - An address is shown whole, or by a prefix of at least 10 characters that
///   ends where the text stops agreeing with the address on a character that
///   is not an ASCII letter or digit — an ellipsis, a space, the end of a
///   line. A different address sharing the first 10 characters does not
///   count.
///
/// **What this cannot see.** Substrings prove a fact is on the screen, not
/// that it sits beside the person it belongs to: one reason shown once
/// satisfies two recipients with that reason, and a name is found inside a
/// longer word. Nor does it see what is scrolled away, clipped or drawn in a
/// colour nobody can read; the list is whatever the wallet hands over.
library;

import 'package:splitz_core/host.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;

import '../currencies.dart';

/// Which of §14.2's facts a finding is about.
enum ReviewRule {
  /// Every recipient the request cannot carry, with the reason (§8.5).
  unpayable,

  /// Every pay-to address the fold recorded as replaced (§10.3).
  replacedAddress,

  /// Every debt with a payment recorded and not yet confirmed (§10.5, §14.4).
  awaiting,

  /// Every recipient paid by a preference other than their first (§14.8).
  lowerPreference,

  /// Every recipient paid more than the debts the bill records explain (§6).
  unexplained,

  /// The rate the request was priced at, and who set it.
  rate,

  /// The ZEC amount and address of every output.
  output,

  /// The ZEC a payment record says was sent, shown to its payee.
  payeeZec,

  /// The rate a payment record was priced at, shown to its payee.
  payeeRate,

  /// A payment record's reference, shown to its payee.
  payeeReference,
}

/// One fact §14.2 requires that the review screen's text does not show.
class ReviewFinding {
  const ReviewFinding(this.rule, this.fact, this.expected);

  final ReviewRule rule;

  /// The fact, in words: whose, and which part of it.
  final String fact;

  /// The text looked for and not found.
  final String expected;

  @override
  String toString() => '${rule.name}: $fact — "$expected" not shown';
}

/// The rate figure a review screen shows: [rate]'s minor units per ZEC in the
/// currency's major units, `.` as the decimal point and every fractional
/// digit its ISO 4217 exponent gives (`51234` EUR is `512.34`). A currency the
/// register gives no exponent is shown as its minor units unchanged.
String rateFigure(splitz.ExchangeRate rate) {
  final units = rate.minorUnitsPerZec;
  final exponent = currencyExponent(rate.currency) ?? 0;
  if (exponent == 0) return '$units';
  final digits = units.abs().toString().padLeft(exponent + 1, '0');
  final whole = digits.substring(0, digits.length - exponent);
  final sign = units < 0 ? '-' : '';
  return '$sign$whole.${digits.substring(digits.length - exponent)}';
}

/// §14.2's facts for [obligation] on [folded], against [visibleText].
///
/// [reasonWords] maps each §8.5 reason code to the words the screen uses for
/// it. [via] is the payer's choice of payouts the obligation was rendered with
/// (`obligationVia`), and [lowerWords] the screen's words for a recipient paid
/// by one other than their first; a choice for somebody the obligation does
/// not pay needs nothing shown. [unexplainedWords] are the screen's words for
/// a payment carrying more than the debts the bill records explain (§6).
/// Findings come in the order of §14.2's list; within a rule, in the order
/// the obligation or the fold gives the facts.
List<ReviewFinding> checkPayerReview({
  required PayerObligation obligation,
  required FoldedBill folded,
  required List<String> visibleText,
  required Map<String, String> reasonWords,
  Map<String, int> via = const {},
  String lowerWords = '',
  String unexplainedWords = '',
}) {
  final text = visibleText.join('\n');
  final names = {for (final p in folded.bill.participants) p.id: p.name};
  String name(String id) => names[id] ?? id;
  final out = <ReviewFinding>[];
  void need(ReviewRule rule, String fact, String expected, bool shown) {
    if (!shown) out.add(ReviewFinding(rule, fact, expected));
  }

  for (final u in obligation.unpayable) {
    final who = name(u.id);
    need(
      ReviewRule.unpayable,
      'who the request cannot pay',
      who,
      text.contains(who),
    );
    final words = reasonWords[u.reason];
    need(
      ReviewRule.unpayable,
      'why $who cannot be paid (${u.reason})',
      words ?? u.reason,
      words != null && words.isNotEmpty && text.contains(words),
    );
  }

  final replaced = <String>{};
  for (final r in folded.replacedAddresses) {
    if (!replaced.add(r.id)) continue;
    final who = name(r.id);
    need(
      ReviewRule.replacedAddress,
      'whose pay-to address was replaced',
      who,
      text.contains(who),
    );
  }

  final pending = <String>{};
  for (final a in obligation.awaiting) {
    for (final id in [a.to, ...a.paidTo]) {
      if (!pending.add(id)) continue;
      final who = name(id);
      need(
        ReviewRule.awaiting,
        'who a payment awaiting confirmation went to',
        who,
        text.contains(who),
      );
    }
  }

  final paid = {for (final s in obligation.settlements) s.to};
  for (final id in via.keys.toList()..sort()) {
    if (via[id] == 0 || !paid.contains(id)) continue;
    final who = name(id);
    need(
      ReviewRule.lowerPreference,
      'who is paid by a lower preference',
      who,
      text.contains(who),
    );
    need(
      ReviewRule.lowerPreference,
      'that $who is paid by a lower preference',
      lowerWords.isEmpty ? 'lower_preference' : lowerWords,
      lowerWords.isNotEmpty && text.contains(lowerWords),
    );
  }

  for (final s in obligation.settlements) {
    if (s.unexplained <= 0) continue;
    final who = name(s.to);
    need(
      ReviewRule.unexplained,
      'who is paid more than their debts explain',
      who,
      text.contains(who),
    );
    need(
      ReviewRule.unexplained,
      'that part of what $who is paid is unexplained',
      unexplainedWords.isEmpty ? 'unexplained' : unexplainedWords,
      unexplainedWords.isNotEmpty && text.contains(unexplainedWords),
    );
  }

  final figure = rateFigure(obligation.rate);
  need(
    ReviewRule.rate,
    'the rate the request was priced at',
    figure,
    _showsNumber(text, figure),
  );
  final author = folded.rateAuthor;
  if (author != null) {
    final who = name(author);
    need(ReviewRule.rate, 'who set the rate', who, text.contains(who));
  }

  final payments = obligation.request.payments;
  for (var i = 0; i < payments.length; i++) {
    final p = payments[i];
    final who = name(obligation.request.recipients[i]);
    final amount = splitz.renderAmount(p.zatoshi);
    need(
      ReviewRule.output,
      'the ZEC sent to $who',
      amount,
      _showsNumber(text, amount),
    );
    need(
      ReviewRule.output,
      'the address $who is paid at',
      p.address,
      _showsAddress(text, p.address),
    );
  }
  return out;
}

/// §14.2's payee facts for [payment], against the text of the screen its
/// payee confirms it on.
///
/// Each of the record's ZEC, rate and reference that it carries must be shown:
/// the ZEC as `renderAmount` writes it, the rate as [rateFigure] writes it,
/// the reference whole or by a prefix of at least 10 characters, under the
/// same matching as [checkPayerReview]. A `shieldedZec` or `swap` record
/// missing one must show [absentWords], the wallet's words for a figure the
/// record does not carry; empty words are then a finding. A `cash` record
/// carries none of them and needs nothing shown. Findings come in that order.
List<ReviewFinding> checkPayeeReview({
  required splitz.PaymentRecord payment,
  required List<String> visibleText,
  String absentWords = '',
}) {
  final text = visibleText.join('\n');
  final overZec = payment.method != 'cash';
  final out = <ReviewFinding>[];
  void need(ReviewRule rule, String fact, (String, bool)? shown) {
    final String expected;
    final bool seen;
    if (shown != null) {
      (expected, seen) = shown;
    } else if (!overZec) {
      return;
    } else if (absentWords.isEmpty) {
      (expected, seen) = ('absent', false);
    } else {
      (expected, seen) = (absentWords, text.contains(absentWords));
    }
    if (!seen) out.add(ReviewFinding(rule, fact, expected));
  }

  final zatoshi = payment.zatoshi;
  final amount = zatoshi == null ? null : splitz.renderAmount(zatoshi);
  need(
    ReviewRule.payeeZec,
    'the ZEC the record says was sent',
    amount == null ? null : (amount, _showsNumber(text, amount)),
  );
  final rate = payment.paidAtRate;
  final figure = rate == null ? null : rateFigure(rate);
  need(
    ReviewRule.payeeRate,
    'the rate it was priced at',
    figure == null ? null : (figure, _showsNumber(text, figure)),
  );
  final reference = payment.reference;
  need(
    ReviewRule.payeeReference,
    'its reference',
    reference == null || reference.isEmpty
        ? null
        : (reference, _showsAddress(text, reference)),
  );
  return out;
}

bool _isDigit(int c) => c >= 0x30 && c <= 0x39;

bool _isAddressChar(int c) =>
    _isDigit(c) || (c >= 0x41 && c <= 0x5a) || (c >= 0x61 && c <= 0x7a);

/// True when [number] occurs in [text] with no digit touching it, and not
/// followed by `.` and a digit.
bool _showsNumber(String text, String number) {
  for (
    var at = text.indexOf(number);
    at >= 0;
    at = text.indexOf(number, at + 1)
  ) {
    final end = at + number.length;
    if (at > 0 && _isDigit(text.codeUnitAt(at - 1))) continue;
    if (end < text.length && _isDigit(text.codeUnitAt(end))) continue;
    if (end + 1 < text.length &&
        text.codeUnitAt(end) == 0x2e &&
        _isDigit(text.codeUnitAt(end + 1))) {
      continue;
    }
    return true;
  }
  return false;
}

/// The shortest prefix of an address a screen may show in its place.
const int _addressPrefixLength = 10;

/// [value] — an address or a reference — as a narrow screen may show it and
/// these checks still count it as shown (§14.2): whole when it is at most two
/// characters longer than the shortest prefix allowed, otherwise that prefix
/// and an ellipsis. Characters are Unicode scalar values (§2.3).
String shortForm(String value) {
  final runes = value.runes.toList();
  if (runes.length <= _addressPrefixLength + 2) return value;
  return '${String.fromCharCodes(runes.take(_addressPrefixLength))}…';
}

/// True when [address] occurs in [text] whole, or as a prefix of at least
/// 10 characters that stops agreeing with it on a
/// character that is not an ASCII letter or digit.
///
/// A character is a Unicode scalar value (§2.3), counted the same way in
/// every implementation: UTF-16 units and UTF-8 bytes give two answers for
/// one screen.
bool _showsAddress(String text, String address) {
  final t = text.runes.toList();
  final a = address.runes.toList();
  if (a.length <= _addressPrefixLength) return text.contains(address);
  for (var at = 0; at + _addressPrefixLength <= t.length; at++) {
    var n = 0;
    while (n < a.length && at + n < t.length && t[at + n] == a[n]) {
      n++;
    }
    if (n < _addressPrefixLength) continue;
    if (n == a.length) return true;
    if (at + n >= t.length || !_isAddressChar(t[at + n])) return true;
  }
  return false;
}
