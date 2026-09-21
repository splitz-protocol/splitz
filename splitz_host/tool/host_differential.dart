/// Answers `tools/differential/host_generate.py`'s operations.
///
/// One JSON object per line in, one per line out. The Rust host answers the
/// same list, and the runner diffs them: nothing in `vectors/` can cover a
/// curve operation, because a vector cannot carry a private key.
library;

import 'dart:convert';
import 'dart:typed_data';
import 'dart:io';

import 'package:crypto/crypto.dart' as hashing;
import 'package:splitz_core/host.dart' as seam;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';

final signer = SplitsSigner();

/// The swap refusals, by which one rather than by its wording.
String swapTag(SwapException e) {
  final m = e.message;
  final which = m.startsWith('A swap sends more than nothing')
      ? 'nothing'
      : m.startsWith('A swap states both')
      ? 'no_refund'
      : m.startsWith('The provider omitted')
      ? 'omitted'
      : m.startsWith('Malformed')
      ? 'malformed'
      : m.startsWith('A quote response carries')
      ? 'no_quote'
      : m.contains('could not be reached')
      ? 'unreachable'
      : m.contains('is not JSON')
      ? 'not_json'
      : 'unclassified';
  return '$which/${e.isTransient}';
}

TradableAsset usdcOnBase() => const TradableAsset(
  assetId: 'nep141:base-usdc',
  symbol: 'USDC',
  chain: 'base',
  decimals: 6,
);

Map<String, Object?> quoteJson(SwapQuote q) => {
  'depositAddress': q.depositAddress,
  'depositMemo': q.depositMemo,
  'amountInZatoshi': q.amountInZatoshi,
  'amountOut': q.amountOut,
  'deadline': q.deadline,
  'reference': q.reference,
  'paymentReference': q.paymentReference,
};

/// A provider that answers with one scripted body and records its URLs.
({OneClickSwaps swaps, List<Uri> urls}) scripted(String body, String deadline) {
  final urls = <Uri>[];
  return (
    swaps: OneClickSwaps(
      origin: Uri.parse('https://swap.example'),
      zecAssetId: 'nep141:zec',
      deadline: () => deadline,
      post: (url, _) async {
        urls.add(url);
        return body;
      },
      get: (url) async {
        urls.add(url);
        return body;
      },
    ),
    urls: urls,
  );
}

/// Rebuilds a split form from one operation's description of it.
SplitDraft draftFrom(Map<String, dynamic> op) {
  final kind = switch (op['kind'] as String) {
    'exact' => SplitKind.exact,
    'percentage' => SplitKind.percentage,
    'shares' => SplitKind.shares,
    'itemized' => SplitKind.itemized,
    _ => SplitKind.equal,
  };
  Map<String, int> weights(String key) => {
    for (final e in (op[key] as Map).entries) e.key as String: e.value as int,
  };
  Set<String> strings(Object? raw) => {
    for (final v in (raw as List? ?? const [])) v as String,
  };
  final draft = SplitDraft(
    kind: kind,
    among: strings(op['among']),
    amounts: weights('amounts'),
    basisPoints: weights('basisPoints'),
    shareCounts: weights('shareCounts'),
    items: [
      for (final item in op['items'] as List)
        DraftItem(
          description: (item as Map)['description'] as String,
          minorUnits: item['minorUnits'] as int,
          sharedBy: strings(item['sharedBy']),
        ),
    ],
    extraMinorUnits: op['extra'] as int,
  );
  for (final id in strings(op['toggle'])) {
    draft.toggle(id);
  }
  return draft;
}

/// One history line, as JSON, so the two implementations are compared line for
/// line rather than by a summary either could get wrong the same way.
Map<String, Object?> eventJson(BillEvent e) => {
  'entryId': e.entryId,
  'kind': rustName(e.kind),
  'author': e.author,
  'at': e.at,
  'subject': e.subject,
  'amount': e.amountMinorUnits,
  'description': e.description,
  'method': e.method,
  'reference': e.reference,
  'withdrawn': e.withdrawn,
  'refusedCode': e.refusedCode,
  'confirmed': e.confirmed,
  'applied': e.applied,
};

/// `BillEventKind.addressChanged` -> `AddressChanged`, the name Rust's
/// `Debug` prints, so the two answers are one string.
String rustName(Enum value) {
  final name = value.name;
  return name[0].toUpperCase() + name.substring(1);
}

final sealing = SplitsSealing();

/// The two implementations word their refusals differently; what has to match
/// is *which* refusal. Both drivers map to this vocabulary.
String tag(Object e) {
  if (e is! SealingException) return 'other';
  const prefixes = {
    'Empty blob': 'empty',
    'Blob is format v': 'version',
    'Blob is too short': 'short',
    'Malformed base64url': 'malformed_b64',
    'Key is ': 'key_length',
    'Could not open': 'auth',
    'Opened blob is not UTF-8': 'not_utf8',
    'Opened blob is not JSON': 'not_json',
    'Opened blob is not an entry': 'not_entry',
    'An entry is not sealable': 'not_sealable',
  };
  for (final entry in prefixes.entries) {
    if (e.message.startsWith(entry.key)) return entry.value;
  }
  return 'unclassified';
}

/// Flips a bit of a blob: 1 in the ciphertext, 2 in the version byte.
String tamperBlob(String blob, int how) {
  final List<int> bytes;
  try {
    bytes = SplitsSigner.decode(blob).toList();
  } on FormatException {
    return blob;
  }
  if (how == 1 && bytes.length > 3) {
    bytes[bytes.length - 3] ^= 0x01;
  } else if (how == 2 && bytes.isNotEmpty) {
    bytes[0] = (bytes[0] + 1) & 0xff;
  }
  return SplitsSigner.encode(bytes);
}

/// Changes one character of a signature, so it is well formed and wrong.
String tamper(String sig) => (sig[0] == 'A' ? 'B' : 'A') + sig.substring(1);

/// A wallet for the settle operation: a fixed instant, a fixed transaction id,
/// and no signing. The instant is supplied per entry so both implementations
/// write the same `at` without either deriving one.
class DiffHost implements seam.BillHost {
  DiffHost({
    required this.me,
    required this.payToAddress,
    required String at,
    required String txid,
  }) : _at = DateTime.parse(at).toUtc(),
       _txid = txid;

  @override
  final String me;
  @override
  final String? payToAddress;
  final DateTime _at;
  final String _txid;

  @override
  seam.Clock get now =>
      () => _at;

  @override
  seam.Randomness get randomBytes =>
      (int n) => Uint8List.fromList(List<int>.generate(n, (i) => i));

  @override
  seam.SignEntry? get sign => null;
  @override
  seam.VerifyEntry? get verify => null;

  @override
  seam.Broadcast get broadcast =>
      (uri) async => _txid.isEmpty
      ? const seam.Sent.failed(detail: 'no transaction id')
      : seam.Sent.sent(_txid);
}

/// The records `settle` writes, or why it wrote none.
Future<Map<String, Object?>> settleRecords(Map<String, dynamic> op) async {
  final people = [
    for (final p in op['people'] as List) (p as Map).cast<String, dynamic>(),
  ];
  final expenses = [
    for (final e in op['expenses'] as List) (e as Map).cast<String, dynamic>(),
  ];
  final instants = [for (final i in op['instants'] as List) i as String];
  final me = op['me'] as String;
  final txid = op['txid'] as String;
  final rate = op['rate'] as int?;

  DiffHost at(int step, String who, String? payTo) =>
      DiffHost(me: who, payToAddress: payTo, at: instants[step], txid: txid);

  final first = people.first;
  var step = 0;
  final entries = <Map<String, dynamic>>[
    seam.createBill(
      host: at(step++, first['id'] as String, first['payTo'] as String?),
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: op['creatorKey'] as String,
    ),
  ];
  for (final p in people) {
    final payouts = p['payouts'] as List?;
    entries.add(
      seam.joinBill(
        host: at(step++, p['id'] as String, p['payTo'] as String?),
        name: p['id'] as String,
        payTo: p['payTo'] as String?,
        payouts: payouts == null
            ? null
            : [for (final o in payouts) (o as Map).cast<String, dynamic>()],
      ),
    );
  }
  for (final e in expenses) {
    entries.add(
      seam.addExpense(
        host: at(step++, e['paidBy'] as String, null),
        expenseId: e['id'] as String,
        paidBy: e['paidBy'] as String,
        amount: e['amount'] as int,
        split: <String, dynamic>{'type': 'equal', 'among': e['among']},
      ),
    );
  }
  if (rate != null) {
    entries.add(
      seam.setRate(
        host: at(step++, first['id'] as String, first['payTo'] as String?),
        currency: 'EUR',
        minorUnitsPerZec: rate,
      ),
    );
  }

  final mine = people.firstWhere((p) => p['id'] == me);
  final host = at(step, me, mine['payTo'] as String?);
  final log = seam.BillLog(host);
  final refused = log.add(entries);
  final seam.FoldedBill folded;
  try {
    folded = log.fold();
  } on protocol.SplitError catch (e) {
    return {'folded': false, 'error': e.code};
  }
  final owed = seam.obligationFor(host, folded);
  if (owed == null) {
    return {
      'folded': true,
      'refused': [for (final r in refused) r.code],
      'setAside': [for (final a in folded.setAside) a.code],
      'priced': false,
    };
  }
  final settled = await seam.settle(host, log, owed);
  return {
    'folded': true,
    'refused': [for (final r in refused) r.code],
    'setAside': [for (final a in folded.setAside) a.code],
    'priced': true,
    'settlements': [
      for (final s in owed.settlements) {'to': s.to, 'amount': s.amount},
    ],
    'unpayable': [
      for (final u in owed.unpayable) {'id': u.id, 'reason': u.reason},
    ],
    'awaiting': [
      for (final a in owed.awaiting)
        {'to': a.to, 'owed': a.owed, 'paid': a.paid},
    ],
    'uri': owed.uri,
    'result': settled.result.name,
    'detail': settled.detail,
    'txid': settled.txid,
    // The whole payload, `at` included: the clock is fixed, so an instant
    // that differs is a real divergence rather than a race.
    'records': [for (final r in settled.records) r['payment']],
  };
}

Future<Object?> answer(Map<String, dynamic> op) async {
  switch (op['op'] as String) {
    case 'settle_records':
      return settleRecords(op);
    case 'public_key':
      return signer.publicKeyFromSeed(
        SplitsSigner.decode(op['seed'] as String),
      );
    case 'sign_entry':
      final entry = (op['entry'] as Map).cast<String, dynamic>();
      final String message;
      try {
        message = protocol.signingMessage(entry);
      } on protocol.SplitError catch (e) {
        return {'error': e.code};
      }
      final sign = signer.signerFor(SplitsSigner.decode(op['seed'] as String));
      return {'message': message, 'sig': await sign(utf8.encode(message))};
    case 'verify':
      final entry = (op['entry'] as Map).cast<String, dynamic>();
      final signWith = SplitsSigner.decode(op['signWith'] as String);
      String message;
      try {
        message = protocol.signingMessage(entry);
      } on protocol.SplitError {
        return {'signable': false};
      }
      var sig = await signer.signerFor(signWith)(utf8.encode(message));
      if (op['tamper'] == true) sig = tamper(sig);
      final signed = <String, dynamic>{...entry, 'sig': sig};
      final trueKey = await signer.publicKeyFromSeed(signWith);
      return {
        'signable': true,
        'againstGivenKey': await signer.verifyEntry(
          signed,
          op['key'] as String,
        ),
        'againstTrueKey': await signer.verifyEntry(signed, trueKey),
      };
    case 'identity_seed':
      final viewingKey = op['viewingKey'] as String;
      // Random on both sides when there is no viewing key, so there is
      // nothing to compare.
      if (viewingKey.isEmpty) return null;
      return SplitsSigner.encode(
        hashing.sha256
            .convert(utf8.encode('${SplitsKeys.identityDomain}:$viewingKey'))
            .bytes,
      );
    case 'seal_open':
      final key = op['key'] as String;
      final String blob;
      try {
        blob = await sealing.seal(
          (op['entry'] as Map).cast<String, dynamic>(),
          key,
        );
      } on SealingException catch (e) {
        return {'sealed': false, 'why': tag(e)};
      }
      final presented = tamperBlob(blob, op['tamper'] as int);
      final openWith = (op['openWith'] as String?) ?? key;
      try {
        final entry = await sealing.open(presented, openWith);
        return {'sealed': true, 'blob': blob, 'opened': true, 'entry': entry};
      } on SealingException catch (e) {
        return {'sealed': true, 'blob': blob, 'opened': false, 'why': tag(e)};
      }
    case 'open_raw':
      try {
        final entry = await sealing.open(
          op['blob'] as String,
          op['key'] as String,
        );
        return {'opened': true, 'entry': entry};
      } on SealingException catch (e) {
        return {'opened': false, 'why': tag(e)};
      }
    case 'swap_encode':
      final text = op['text'] as String;
      return {
        'query': Uri.encodeQueryComponent(text),
        'component': Uri.encodeComponent(text),
      };
    case 'swap_status':
      final p = scripted(jsonEncode(op['body']), '');
      final quote = SwapQuote(
        depositAddress: 'u1provider',
        depositMemo: op['memo'] as String?,
        amountInZatoshi: 1,
        amountOut: '1',
        asset: usdcOnBase(),
        deadline: '2026-01-01T00:00:00.000Z',
      );
      try {
        final status = await p.swaps.statusOf(quote);
        return {
          'ok': true,
          'state': rustName(status.state),
          'hash': status.destinationTxHash,
          'detail': status.detail,
          'url': p.urls.isEmpty ? null : p.urls.last.toString(),
        };
      } on SwapException catch (e) {
        return {'ok': false, 'why': swapTag(e)};
      }
    case 'swap_quote':
      final p = scripted(jsonEncode(op['body']), op['deadline'] as String);
      try {
        final quote = await p.swaps.quote(
          asset: usdcOnBase(),
          amountInZatoshi: op['amount'] as int,
          recipient: op['recipient'] as String,
          refundTo: op['refundTo'] as String,
        );
        return {'ok': true, 'quote': quoteJson(quote)};
      } on SwapException catch (e) {
        return {'ok': false, 'why': swapTag(e)};
      }
    case 'swap_watch':
      final watch = SwapWatch.fromJson(
        (op['json'] as Map).cast<String, dynamic>(),
      );
      if (watch == null) return {'parsed': false};
      return {
        'parsed': true,
        'json': watch.toJson(),
        'quote': quoteJson(watch.asQuote),
      };
    case 'split_draft':
      final draft = draftFrom(op);
      final total = op['total'] as int;
      final allocation = draft.allocation(total);
      return {
        'wireType': draft.kind.wireType,
        'split': draft.toSplit(),
        'allocation': allocation,
        'refusalCode': draft.refusalCode(total),
        'participants': draft.participants.toList()..sort(),
      };
    case 'activity':
      final protocol.Bill bill;
      try {
        bill = protocol.decodeBill((op['bill'] as Map).cast<String, dynamic>());
      } on protocol.SplitError {
        return {'decoded': false};
      }
      final entries = [
        for (final e in op['entries'] as List)
          (e as Map).cast<String, dynamic>(),
      ];
      final setAside = [
        for (final s in op['setAside'] as List)
          protocol.SetAside((s as Map)['id'] as String, s['code'] as String),
      ];
      final withdrawn = [for (final w in op['withdrawn'] as List) w as String];
      final history = activityOf(
        entries,
        bill,
        setAside: setAside,
        withdrawn: withdrawn,
      );
      final awaiting = awaitingConfirmationBy(bill, op['me'] as String);
      return {
        'decoded': true,
        'history': [for (final e in history) eventJson(e)],
        'awaiting': [
          for (final p in awaiting)
            {
              'id': p.id,
              'from': p.from,
              'to': p.to,
              'amount': p.amount,
              'at': p.at,
            },
        ],
      };
    case 'store_read':
      final storage = InMemoryBillStorage();
      await storage.write('splitz_bill_b1', op['stored'] as String);
      final entries = await BillStore(storage).read('b1');
      return {'count': entries.length, 'entries': entries};
    case 'store_merge':
      final store = BillStore(InMemoryBillStorage());
      final held = (op['held'] as List)
          .map((e) => (e as Map).cast<String, dynamic>())
          .toList();
      final incoming = (op['incoming'] as List)
          .map((e) => (e as Map).cast<String, dynamic>())
          .toList();
      try {
        await store.merge('b1', held);
        final merged = await store.merge('b1', incoming);
        return {
          'merged': true,
          'ids': [for (final e in merged.entries) e['id']],
          'refused': merged.refused.length,
          'readBack': (await store.read('b1')).length,
        };
      } on protocol.SplitError {
        return {'merged': false};
      }
    case 'well_formed_key':
      return SplitsKeys.isWellFormedKey(op['key'] as String);
    case 'b64_round_trip':
      final text = op['text'] as String;
      try {
        final raw = SplitsSigner.decode(text);
        return {'bytes': raw.length, 'reencoded': SplitsSigner.encode(raw)};
      } on FormatException {
        return null;
      }
    default:
      return {'error': 'unknown op ${op['op']}'};
  }
}

Future<void> main() async {
  final out = StringBuffer();
  for (final line
      in await stdin
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .toList()) {
    if (line.trim().isEmpty) continue;
    final op = (jsonDecode(line) as Map).cast<String, dynamic>();
    out.writeln(jsonEncode(await answer(op)));
  }
  stdout.write(out);
}
