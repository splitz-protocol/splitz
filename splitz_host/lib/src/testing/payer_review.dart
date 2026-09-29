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

  /// The rate the request was priced at, and who set it.
  rate,

  /// The ZEC amount and address of every output.
  output,
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
/// it. Findings come in the order of §14.2's list; within a rule, in the order
/// the obligation or the fold gives the facts.
List<ReviewFinding> checkPayerReview({
  required PayerObligation obligation,
  required FoldedBill folded,
  required List<String> visibleText,
  required Map<String, String> reasonWords,
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

/// True when [address] occurs in [text] whole, or as a prefix of at least
/// 10 characters that stops agreeing with it on a
/// character that is not an ASCII letter or digit.
bool _showsAddress(String text, String address) {
  if (address.length <= _addressPrefixLength) return text.contains(address);
  final head = address.substring(0, _addressPrefixLength);
  for (var at = text.indexOf(head); at >= 0; at = text.indexOf(head, at + 1)) {
    var n = _addressPrefixLength;
    while (n < address.length &&
        at + n < text.length &&
        text.codeUnitAt(at + n) == address.codeUnitAt(n)) {
      n++;
    }
    if (n == address.length) return true;
    if (at + n >= text.length || !_isAddressChar(text.codeUnitAt(at + n))) {
      return true;
    }
  }
  return false;
}
