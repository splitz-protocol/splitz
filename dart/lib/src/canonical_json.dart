/// Canonical JSON (SPEC.md §9.3).
///
/// Object keys in ascending UTF-8 byte order, no insignificant whitespace.
/// This is the encoding used wherever a document's bytes are compared, hashed
/// or signed, which is the one place key order stops being presentational and
/// becomes part of the message.
library;

import 'dart:convert';

import 'errors.dart';
import 'ordering.dart';

/// Encodes [value] canonically.
///
/// A floating point number anywhere in the document is refused with
/// `canonical_json_float` rather than truncated: §2 puts every amount in minor
/// units as an integer, so a document carrying one was not written by a
/// conforming writer.
String canonicalJson(Object? value) {
  final buffer = StringBuffer();
  _write(value, buffer);
  return buffer.toString();
}

void _write(Object? value, StringBuffer out) {
  if (value == null) {
    out.write('null');
  } else if (value is bool) {
    out.write(value ? 'true' : 'false');
  } else if (value is double) {
    raise(SplitCode.canonicalJsonFloat,
        'A canonical document carries integer minor units, got $value');
  } else if (value is int) {
    out.write(value.toString());
  } else if (value is String) {
    out.write(jsonEncode(value));
  } else if (value is List) {
    out.write('[');
    for (var i = 0; i < value.length; i++) {
      if (i > 0) out.write(',');
      _write(value[i], out);
    }
    out.write(']');
  } else if (value is Map) {
    final keys = sortedUtf8(value.keys.cast<String>());
    out.write('{');
    for (var i = 0; i < keys.length; i++) {
      if (i > 0) out.write(',');
      out.write(jsonEncode(keys[i]));
      out.write(':');
      _write(value[keys[i]], out);
    }
    out.write('}');
  } else {
    raise(SplitCode.billTypeError, 'Not encodable: ${value.runtimeType}');
  }
}
