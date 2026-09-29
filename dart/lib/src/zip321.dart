/// ZIP 321 payment requests (SPEC.md §8).
///
/// One canonical rendering, because two wallets that render the same
/// obligation differently cannot check each other.
library;

import 'dart:convert';

import 'address.dart';
import 'errors.dart';
import 'money.dart';
import 'rate.dart';

/// Whether [address] is one §8.3 admits: non-empty and ASCII alphanumeric.
///
/// Syntax only; [parseAddress] decodes one (§8.6).
bool isZip321Address(String address) =>
    RegExp(r'^[A-Za-z0-9]+$').hasMatch(address);

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

/// Renders [price] as a `fiat` value (§8.4).
String renderFiat(FiatPrice price) {
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
    if (!isZip321Address(p.address)) {
      raise(SplitCode.zip321BadAddress,
          'The ZIP 321 grammar admits only alphanumeric addresses');
    }
    // A memo goes only to an address that decodes (§8.6) and can receive
    // one; ZIP 321 refuses the whole request otherwise.
    if (p.memo != null && !parseAddress(p.address).canReceiveMemo) {
      raise(SplitCode.zip321MemoUndeliverable,
          'A memo cannot be delivered to a transparent or TEX address');
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
      parts.add('fiat$sfx=${renderFiat(p.fiat!)}');
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

/// The parameters §8.2 writes, and nothing else.
const Set<String> _requestParams = {
  'address',
  'amount',
  'fiat',
  'memo',
  'label',
  'message',
};

Never _notCanonical(String why) => raise(SplitCode.zip321NotCanonical, why);

/// Reads back a request in exactly the form [renderUri] writes (§8.7).
///
/// Not a general ZIP 321 reader: every URI this protocol hands a wallet is one
/// [renderUri] produced, so the payments read are rendered again and the
/// result must equal [uri] byte for byte. Anything else — another parameter,
/// another order, another spelling of one amount — is refused with
/// `zip321_not_canonical`, as is anything that does not read at all. A value
/// [renderUri] itself refuses is refused with that code.
List<Zip321Payment> readRequest(String uri) {
  const scheme = 'zcash:';
  if (!uri.startsWith(scheme)) _notCanonical('Not a zcash: URI');
  final rest = uri.substring(scheme.length);
  final query = rest.indexOf('?');
  if (query < 0) _notCanonical('A request carries a query');
  final pathAddress = rest.substring(0, query);

  final byIndex = <int, Map<String, String>>{};
  for (final part in rest.substring(query + 1).split('&')) {
    final eq = part.indexOf('=');
    if (eq <= 0) _notCanonical('Not a parameter: "$part"');
    final key = part.substring(0, eq);
    final value = part.substring(eq + 1);
    final dot = key.indexOf('.');
    final name = dot < 0 ? key : key.substring(0, dot);
    var index = 0;
    if (dot >= 0) {
      final digits = key.substring(dot + 1);
      // `.0` is not written, and an index has no leading zero (§8.2).
      if (!RegExp(r'^[1-9][0-9]{0,3}$').hasMatch(digits)) {
        _notCanonical('Not a parameter index: "$digits"');
      }
      index = int.parse(digits);
    }
    if (!_requestParams.contains(name)) {
      _notCanonical('Not a parameter §8.2 writes: "$name"');
    }
    final params = byIndex.putIfAbsent(index, () => {});
    if (params.containsKey(name)) _notCanonical('"$key" appears twice');
    params[name] = value;
  }

  final count = byIndex.length;
  final payments = <Zip321Payment>[];
  for (var i = 0; i < count; i++) {
    final p = byIndex[i];
    if (p == null) _notCanonical('Payment $i is missing');
    final address = i == 0 && pathAddress.isNotEmpty
        ? (p.containsKey('address')
            ? _notCanonical('The address is written twice')
            : pathAddress)
        : (p['address'] ?? _notCanonical('Payment $i names no address'));
    final amount = p['amount'] ?? _notCanonical('Payment $i has no amount');
    final fiat = p['fiat'];
    final memo = p['memo'];
    final label = p['label'];
    final message = p['message'];
    payments.add(Zip321Payment(
      address: address,
      zatoshi: _readAmount(amount),
      fiat: fiat == null ? null : _readFiat(fiat),
      memo: memo == null ? null : _readMemo(memo),
      label: label == null ? null : _unqchar(label),
      message: message == null ? null : _unqchar(message),
    ));
  }

  final rendered =
      renderUri(payments, includeFiat: payments.any((p) => p.fiat != null));
  if (rendered != uri) _notCanonical('Not the form §8 writes');
  return payments;
}

/// Decimal ZEC to zatoshi. The canonical spelling is enforced by the
/// comparison [readRequest] makes afterwards.
int _readAmount(String text) {
  final match = RegExp(r'^([0-9]{1,8})(?:\.([0-9]{1,8}))?$').firstMatch(text);
  if (match == null) _notCanonical('Not an amount: "$text"');
  final coins = int.parse(match.group(1)!);
  final zats = int.parse((match.group(2) ?? '').padRight(8, '0'));
  return coins * zatoshiPerZec + zats;
}

FiatPrice _readFiat(String text) {
  final match = RegExp(r'^([A-Z]{3}):([0-9]{1,18})$').firstMatch(text);
  if (match == null) _notCanonical('Not a fiat price: "$text"');
  return FiatPrice(match.group(1)!, int.parse(match.group(2)!));
}

List<int> _readMemo(String text) {
  if (!RegExp(r'^[A-Za-z0-9_-]*$').hasMatch(text) || text.length % 4 == 1) {
    _notCanonical('Not unpadded base64url: "$text"');
  }
  try {
    return base64Url.decode(text.padRight((text.length + 3) ~/ 4 * 4, '='));
  } on FormatException {
    // Stray bits after the last whole byte: not the spelling §8.3 writes.
    _notCanonical('Not unpadded base64url: "$text"');
  }
}

/// Undoes [qchar]. Malformed escapes and bytes that are not UTF-8 are refused.
String _unqchar(String text) {
  final out = <int>[];
  for (var i = 0; i < text.length; i++) {
    final unit = text.codeUnitAt(i);
    if (unit == 0x25) {
      if (i + 2 >= text.length) _notCanonical('A cut-off escape');
      final byte = int.tryParse(text.substring(i + 1, i + 3), radix: 16);
      if (byte == null ||
          text.substring(i + 1, i + 3).contains(RegExp(r'[^0-9A-Fa-f]'))) {
        _notCanonical('Not an escape: "${text.substring(i, i + 3)}"');
      }
      out.add(byte);
      i += 2;
    } else if (unit < 0x80) {
      out.add(unit);
    } else {
      _notCanonical('A raw non-ASCII character');
    }
  }
  try {
    return utf8.decode(out);
  } on FormatException {
    _notCanonical('Escapes that are not UTF-8');
  }
}
