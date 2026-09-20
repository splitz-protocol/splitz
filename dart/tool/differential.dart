/// Answers the differential lane's operation list.
///
/// Reads one JSON operation per line on stdin and writes one JSON answer per
/// line on stdout. Every implementation answers the same list, and
/// `tools/differential/compare.py` diffs the answers against each other rather
/// than against anybody's expectation.
library;

import 'dart:convert';
import 'dart:io';

import 'package:splitz_core/splitz_core.dart';

/// Runs [body] and returns its value, or the refusal code that stopped it.
Map<String, Object?> attempt(Object? Function() body) {
  try {
    return {'ok': body()};
  } on SplitError catch (e) {
    return {'refused': e.code};
  }
}

Map<String, Object?> answer(Map<String, dynamic> op) {
  switch (op['op']) {
    case 'allocate':
      return attempt(() => allocate(
            op['total'] as int,
            (op['weights'] as List).cast<int>(),
          ));

    case 'split':
      return attempt(() => splitExpense(
            op['total'] as int,
            (op['split'] as Map).cast<String, dynamic>(),
          ));

    case 'rate':
      return attempt(() => fiatToZatoshi(
            op['minorUnits'] as int,
            ExchangeRate(
              currency: op['currency'] as String,
              minorUnitsPerZec: op['minorUnitsPerZec'] as int,
              at: '2026-10-28T19:30:00.000Z',
            ),
            rounding: switch (op['rounding'] as String) {
              'down' => RateRounding.down,
              'nearest' => RateRounding.nearest,
              _ => RateRounding.up,
            },
          ));

    case 'amount':
      return attempt(() => renderAmount(op['zatoshi'] as int));

    case 'qchar':
      return attempt(() => qchar(op['text'] as String));

    case 'instant':
      return attempt(() => canonicalInstant(op['text'] as String));

    case 'invite':
      return attempt(() {
        final i = parseInvite(op['uri'] as String);
        return {
          'billId': i.billId,
          'key': i.key,
          'name': i.name,
          'expiry': i.expiry
        };
      });

    case 'canonical':
      return attempt(() => canonicalJson(op['value']));

    case 'request':
      return attempt(() => renderUri(
            [
              for (final p
                  in (op['payments'] as List).cast<Map<String, dynamic>>())
                Zip321Payment(
                  address: p['address'] as String,
                  zatoshi: p['zatoshi'] as int,
                  fiat: p['fiat'] == null
                      ? null
                      : FiatPrice((p['fiat'] as List)[0] as String,
                          (p['fiat'] as List)[1] as int),
                  memo: p['memo'] == null
                      ? null
                      : utf8.encode(p['memo'] as String),
                  label: p['label'] as String?,
                  message: p['message'] as String?,
                ),
            ],
            includeFiat: op['includeFiat'] as bool,
          ));

    case 'fold':
      return attempt(() {
        final r = foldLog((op['log'] as List).cast<Map<String, dynamic>>());
        // The bill goes through the decoder: a fold that returns a document
        // its own decoder refuses is the defect this op exists to catch, and
        // it must show as a divergence rather than a crash.
        decodeBill(r.bill);
        return {
          'bill': r.bill,
          'setAside': [
            for (final a in r.setAside) {'id': a.id, 'code': a.code},
          ],
          'withdrawn': r.withdrawn,
        };
      });

    case 'merge':
      return attempt(() {
        final r = mergeLogs([
          for (final part in (op['parts'] as List))
            (part as List).cast<Map<String, dynamic>>(),
        ]);
        return {
          // The entries themselves: §10.2 rule 2 decides which copy under one
          // id survives, and an id list is the same either way.
          'merged': r.merged,
          'refused': [
            for (final a in r.refused) {'id': a.id, 'code': a.code},
          ],
        };
      });

    case 'property':
      return {
        'ok': [
          for (final run in (op['runs'] as List))
            answer((run as Map).cast<String, dynamic>()),
        ],
      };

    case 'billid':
      return attempt(
          () => deriveBillId((op['entry'] as Map).cast<String, dynamic>()));

    default:
      return {'refused': 'unknown_operation'};
  }
}

void main() {
  for (String? line = stdin.readLineSync();
      line != null;
      line = stdin.readLineSync()) {
    if (line.trim().isEmpty) continue;
    final op = (jsonDecode(line) as Map).cast<String, dynamic>();
    final result = answer(op);
    stdout.writeln(jsonEncode({'id': op['id'], ...result}));
  }
}
