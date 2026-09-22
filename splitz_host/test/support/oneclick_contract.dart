/// The 1Click API as the provider publishes it, for tests to hold the client
/// to.
///
/// `tools/contracts/oneclick.json` is the provider's own schema, pinned by
/// `tools/contracts/oneclick.py`; `tools/contracts/fixtures/` holds its raw
/// answers, captured by `tool/oneclick_live.dart`. Neither is written by hand.
library;

import 'dart:convert';
import 'dart:io';

const String contractDir = '../tools/contracts';

final Map<String, dynamic> _schemas =
    (jsonDecode(File('$contractDir/oneclick.json').readAsStringSync())
            as Map<String, dynamic>)['schemas']
        as Map<String, dynamic>;

Map<String, dynamic> schema(String name) =>
    _schemas[name] as Map<String, dynamic>;

/// A captured answer from the live API.
Object? fixture(String name) =>
    jsonDecode(File('$contractDir/fixtures/$name').readAsStringSync());

String fixtureText(String name) =>
    File('$contractDir/fixtures/$name').readAsStringSync();

/// What the provider would refuse in [body], as a quote request; empty when
/// it would accept it.
///
/// The schema's `required` list, its declared properties, their types and
/// enums, and the bound the live API states for `slippageTolerance` when it
/// refuses one (`fixtures/quote_refused.json`): an integer from 0 to 10000.
List<String> quoteRequestProblems(Map<String, dynamic> body) {
  final s = schema('QuoteRequest');
  final props = s['properties'] as Map<String, dynamic>;
  final problems = <String>[
    for (final r in (s['required'] as List).cast<String>())
      if (!body.containsKey(r)) '$r is required',
  ];
  for (final MapEntry(:key, :value) in body.entries) {
    final p = props[key] as Map<String, dynamic>?;
    if (p == null) {
      problems.add('$key is not a property the API declares');
      continue;
    }
    final ok = switch (p['type']) {
      'string' => value is String,
      'number' => value is num,
      'boolean' => value is bool,
      'array' => value is List,
      _ => true,
    };
    if (!ok) problems.add('$key is not a ${p['type']}');
    final allowed = p['enum'] as List?;
    if (allowed != null && !allowed.contains(value)) {
      problems.add('$key "$value" is not one of $allowed');
    }
  }
  final slippage = body['slippageTolerance'];
  if (slippage is num &&
      (slippage != slippage.roundToDouble() ||
          slippage < 0 ||
          slippage > 10000)) {
    problems.add('slippageTolerance must be an integer from 0 to 10000');
  }
  return problems;
}

/// Whether [path] — `Schema.field.field`, `[]` stepping into an array's items
/// — is something the schema declares.
bool declares(String path) {
  final parts = path.split('.');
  Map<String, dynamic>? node = schema(parts.first);
  for (final part in parts.skip(1)) {
    final isList = part.endsWith('[]');
    final name = isList ? part.substring(0, part.length - 2) : part;
    final props = _resolve(node)?['properties'] as Map<String, dynamic>?;
    node = props?[name] as Map<String, dynamic>?;
    if (node == null) return false;
    if (isList) node = _resolve(node)?['items'] as Map<String, dynamic>?;
  }
  return node != null;
}

Map<String, dynamic>? _resolve(Map<String, dynamic>? node) {
  if (node == null) return null;
  final ref =
      node[r'$ref'] ??
      ((node['allOf'] as List?)?.firstOrNull as Map?)?[r'$ref'];
  if (ref is String) return schema(ref.split('/').last);
  return node;
}
