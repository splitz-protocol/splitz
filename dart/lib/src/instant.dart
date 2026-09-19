/// Canonical instants (SPEC.md §9.3).
///
/// RFC 3339, UTC, exactly three fractional digits, `Z` suffix —
/// `2026-10-28T19:30:00.000Z`. Canonical instants are fixed width, so their
/// lexicographic order is their chronological order and a log sorts without a
/// calendar.
library;

import 'errors.dart';

final RegExp _grammar = RegExp(
  r'^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(\.\d+)?[Zz]$',
);

int _daysInMonth(int year, int month) {
  if (month == 2) {
    final leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    return leap ? 29 : 28;
  }
  const thirtyOne = {1, 3, 5, 7, 8, 10, 12};
  return thirtyOne.contains(month) ? 31 : 30;
}

/// Normalises [text] to canonical form, refusing anything the grammar of §9.3
/// does not admit.
///
/// Fractional digits beyond the third are truncated, never rounded. A numeric
/// offset, a space separator, a leap second, a day the month does not have, a
/// year outside 0001 to 9999 and a bare date are all refused: a looser rule
/// lets two readers give one entry two different sort keys.
String canonicalInstant(Object? text) {
  if (text is! String) {
    raise(SplitCode.billTypeError, 'An instant is a string, got $text');
  }
  final m = _grammar.firstMatch(text);
  if (m == null) {
    raise(SplitCode.billTypeError, 'Not a canonical instant: "$text"');
  }
  final year = int.parse(m.group(1)!);
  final month = int.parse(m.group(2)!);
  final day = int.parse(m.group(3)!);
  final hour = int.parse(m.group(4)!);
  final minute = int.parse(m.group(5)!);
  final second = int.parse(m.group(6)!);

  if (year < 1 || month < 1 || month > 12) {
    raise(SplitCode.billTypeError, 'Not a date: "$text"');
  }
  if (day < 1 || day > _daysInMonth(year, month)) {
    raise(SplitCode.billTypeError, 'No such day: "$text"');
  }
  // A leap second is refused: it is not a second this grammar has.
  if (hour > 23 || minute > 59 || second > 59) {
    raise(SplitCode.billTypeError, 'Not a time of day: "$text"');
  }

  final frac = m.group(7) == null ? '000' : m.group(7)!.substring(1);
  final millis = '${frac}000'.substring(0, 3);

  return '${_pad(year, 4)}-${_pad(month, 2)}-${_pad(day, 2)}'
      'T${_pad(hour, 2)}:${_pad(minute, 2)}:${_pad(second, 2)}.${millis}Z';
}

String _pad(int value, int width) => value.toString().padLeft(width, '0');
