/// Ascending order, everywhere (SPEC.md §2.3).
///
/// The order is the lexicographic order of the UTF-8 encoding, byte by byte.
/// Dart's own `String.compareTo` compares UTF-16 code units and disagrees for
/// every character outside the Basic Multilingual Plane, so it is never used
/// on an identifier: leftover minor units, settlement ties and log order are
/// all resolved by ascending id.
library;

import 'dart:convert';

import 'errors.dart';

/// Compares [a] and [b] as UTF-8 byte sequences.
int compareUtf8(String a, String b) {
  final x = utf8.encode(a);
  final y = utf8.encode(b);
  final shorter = x.length < y.length ? x.length : y.length;
  for (var i = 0; i < shorter; i++) {
    if (x[i] != y[i]) return x[i] - y[i];
  }
  return x.length - y.length;
}

/// [values] in ascending UTF-8 byte order.
List<String> sortedUtf8(Iterable<String> values) =>
    values.toList()..sort(compareUtf8);

/// The distinct members of [values], in ascending UTF-8 byte order.
List<String> uniqueSortedUtf8(Iterable<String> values) =>
    sortedUtf8(values.toSet());

/// Whether [text] holds a UTF-16 code unit that is not part of a surrogate
/// pair (§2.3).
///
/// Such a code point has no UTF-8 encoding. An encoder substitutes U+FFFD for
/// every one of them, so two strings that are not equal compare equal and
/// ascending id stops being a total order.
bool hasLoneSurrogate(String text) {
  for (var i = 0; i < text.length; i++) {
    final unit = text.codeUnitAt(i);
    if (unit < 0xD800 || unit > 0xDFFF) continue;
    final isHigh = unit < 0xDC00;
    if (!isHigh) return true; // a low surrogate not consumed by a high one
    if (i + 1 >= text.length) return true;
    final next = text.codeUnitAt(i + 1);
    if (next < 0xDC00 || next > 0xDFFF) return true;
    i++; // a well-formed pair
  }
  return false;
}

/// Refuses any string in [value], at any depth, that is not a sequence of
/// Unicode scalar values (§2.3). Object keys are strings too.
void checkScalarValues(Object? value) {
  if (value is String) {
    if (hasLoneSurrogate(value)) {
      raise(SplitCode.billNotScalarValues,
          'A string carries a lone surrogate and has no UTF-8 encoding');
    }
  } else if (value is Map) {
    for (final e in value.entries) {
      checkScalarValues(e.key);
      checkScalarValues(e.value);
    }
  } else if (value is List) {
    for (final v in value) {
      checkScalarValues(v);
    }
  }
}
