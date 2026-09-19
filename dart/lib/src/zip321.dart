/// ZIP 321 payment requests (SPEC.md §8).
///
/// One canonical rendering, because two wallets that render the same
/// obligation differently cannot check each other.
library;

import 'dart:convert';

import 'errors.dart';
import 'money.dart';
import 'rate.dart';

/// 21000000 ZEC, in zatoshi.
const int maxZatoshi = 2100000000000000;

/// The decoded memo cap, in bytes.
const int maxMemoBytes = 512;

/// A display name is cut to this many UTF-8 bytes, on a character boundary.
const int maxLabelBytes = 96;

/// Parameter indices run to 9999, so a request carries at most this many
/// payments.
const int maxPayments = 10000;

/// The largest fiat count `fiat` admits, in digits. Eighteen keeps every
/// admissible value inside a signed 64-bit integer; nineteen does not.
const int maxFiatDigits = 18;

const String _unreserved =
    'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~';
const String _qcharExtra = r"!$'()*+,;:@";

/// One output of a payment request.
class Zip321Payment {
  const Zip321Payment({
    required this.address,
    required this.zatoshi,
    this.fiat,
    this.memo,
    this.label,
    this.message,
  });

  final String address;
  final int zatoshi;

  /// What this payment's amount was priced as. Advisory: it takes no part in
  /// computing any output value.
  final FiatPrice? fiat;
  final List<int>? memo;
  final String? label;
  final String? message;
}

/// A `<CUR>:<minorUnits>` price for one payment's amount.
///
/// It is the value of that payment, not the price of one ZEC. A per-ZEC rate
/// written here parses and is wrong by the ratio between the rate and the
/// amount.
class FiatPrice {
  const FiatPrice(this.currency, this.minorUnits);
  final String currency;
  final int minorUnits;
}

/// Renders [zatoshi] as decimal ZEC (§8.1).
///
/// Trailing zeros are removed, so one value has exactly one representation.
String renderAmount(int zatoshi) {
  if (zatoshi <= 0) {
    raise(SplitCode.zip321AmountNotPositive,
        'A payment sends more than nothing, got $zatoshi');
  }
  if (zatoshi > maxZatoshi) {
    raise(SplitCode.zip321AmountTooLarge,
        'A payment of $zatoshi zatoshi exceeds the supply');
  }
  final coins = zatoshi ~/ zatoshiPerZec;
  final zats = zatoshi % zatoshiPerZec;
  if (zats == 0) return '$coins';
  var frac = zats.toString().padLeft(8, '0');
  while (frac.endsWith('0')) {
    frac = frac.substring(0, frac.length - 1);
  }
  return '$coins.$frac';
}

/// Percent-escapes [text] as ZIP 321 `qchar` (§8.3).
///
/// Every byte outside the literal set, including every non-ASCII byte, is
/// written as an upper-case escape.
String qchar(String text) {
  final out = StringBuffer();
  for (final byte in utf8.encode(text)) {
    final ch = String.fromCharCode(byte);
    if (_unreserved.contains(ch) || _qcharExtra.contains(ch)) {
      out.write(ch);
    } else {
      out.write('%${byte.toRadixString(16).toUpperCase().padLeft(2, '0')}');
    }
  }
  return out.toString();
}

/// Cuts [name] to [maxLabelBytes] on a character boundary (§8.3).
String boundedLabel(String name) {
  final raw = utf8.encode(name);
  if (raw.length <= maxLabelBytes) return name;
  var cut = raw.sublist(0, maxLabelBytes);
  while (cut.isNotEmpty) {
    try {
      return utf8.decode(cut);
    } on FormatException {
      cut = cut.sublist(0, cut.length - 1);
    }
  }
  return '';
}

/// Unpadded base64url, as everywhere in this protocol.
String _b64(List<int> raw) => base64UrlEncode(raw).replaceAll('=', '');

String _renderFiat(FiatPrice price) {
  if (!isCurrency(price.currency)) {
    raise(SplitCode.zip321BadCurrencyCode,
        'A fiat code is three upper-case letters, got "${price.currency}"');
  }
  if (price.minorUnits <= 0) {
    raise(SplitCode.zip321FiatNotPositive,
        'A fiat price of ${price.minorUnits} is not positive');
  }
  if (price.minorUnits.toString().length > maxFiatDigits) {
    raise(SplitCode.zip321FiatTooManyDigits,
        'A fiat price of ${price.minorUnits} exceeds $maxFiatDigits digits');
  }
  return '${price.currency}:${price.minorUnits}';
}

/// Renders [payments] as one ZIP 321 URI (§8.2).
String renderUri(List<Zip321Payment> payments, {bool includeFiat = false}) {
  if (payments.isEmpty) {
    raise(SplitCode.zip321NoPayments, 'A request carries no payments');
  }
  if (payments.length > maxPayments) {
    raise(SplitCode.zip321TooManyPayments,
        'A request carries ${payments.length} payments, more than $maxPayments');
  }

  // The address is checked before any other parameter of the same payment, so
  // a payment invalid in two ways is refused with the same code everywhere.
  for (final p in payments) {
    if (p.address.isEmpty) {
      raise(SplitCode.zip321NoAddress, 'A payment names no address');
    }
    if (!RegExp(r'^[A-Za-z0-9]+$').hasMatch(p.address)) {
      raise(SplitCode.zip321BadAddress,
          'The ZIP 321 grammar admits only alphanumeric addresses');
    }
  }

  final single = payments.length == 1;
  final parts = <String>[];
  for (var i = 0; i < payments.length; i++) {
    final p = payments[i];
    final sfx = i == 0 ? '' : '.$i';
    if (!(single && i == 0)) {
      parts.add('address$sfx=${p.address}');
    }
    parts.add('amount$sfx=${renderAmount(p.zatoshi)}');
    if (includeFiat && p.fiat != null) {
      parts.add('fiat$sfx=${_renderFiat(p.fiat!)}');
    }
    if (p.memo != null) {
      if (p.memo!.length > maxMemoBytes) {
        raise(SplitCode.zip321MemoTooLarge,
            'A memo of ${p.memo!.length} bytes exceeds $maxMemoBytes');
      }
      parts.add('memo$sfx=${_b64(p.memo!)}');
    }
    if (p.label != null) {
      parts.add('label$sfx=${qchar(boundedLabel(p.label!))}');
    }
    if (p.message != null) {
      parts.add('message$sfx=${qchar(p.message!)}');
    }
  }

  final head = single ? 'zcash:${payments[0].address}?' : 'zcash:?';
  return head + parts.join('&');
}
