/// Runs the language-neutral corpus in ../vectors against this implementation.
///
/// A case carries either `expect` or `error`. Refusing for the wrong reason is
/// a failure, so the code is compared and not merely the fact of a refusal.
library;

import 'dart:convert';
import 'dart:io';

import 'package:splitz_core/splitz_core.dart';
import 'package:test/test.dart';

/// The corpus lives at the repository root, one level above this package, so a
/// published package cannot carry it. `SPLITZ_VECTORS` points at a checkout.
final vectorDir = Platform.environment['SPLITZ_VECTORS'] ?? '../vectors';

/// True when the corpus is not on disk. Every conformance group is skipped
/// rather than failed: a missing corpus is an absent input, not a divergence.
final corpusAbsent = !Directory(vectorDir).existsSync();

Map<String, dynamic> loadFile(String name) =>
    jsonDecode(File('$vectorDir/$name').readAsStringSync())
        as Map<String, dynamic>;

/// Runs [body] and returns the refusal code, or null when it returns.
String? refusalOf(void Function() body) {
  try {
    body();
    return null;
  } on SplitError catch (e) {
    return e.code;
  }
}

void runCases(
  String file,
  void Function(Map<String, dynamic> input, void Function(Object?) produce) run,
) {
  if (corpusAbsent) {
    test('$file skipped', () {
      printOnFailure('no corpus at $vectorDir');
    }, skip: 'no corpus at $vectorDir; set SPLITZ_VECTORS to a checkout');
    return;
  }
  final doc = loadFile(file);
  final cases = (doc['cases'] as List).cast<Map<String, dynamic>>();
  test('$file declares its own count', () {
    expect(cases.length, doc['count'], reason: 'count is out of step');
  });

  for (final c in cases) {
    test('$file: ${c['name']}', () {
      Object? produced;
      final code = refusalOf(() => run(c, (v) => produced = v));

      if (c.containsKey('error')) {
        expect(code, c['error'],
            reason: code == null
                ? 'accepted an input the corpus refuses'
                : 'refused for the wrong reason');
      } else if (c.containsKey('expect')) {
        expect(code, isNull, reason: 'refused an input the corpus accepts');
        expect(produced, c['expect']);
      }
    });
  }
}

ExchangeRate rateOf(Map<String, dynamic> raw) => ExchangeRate(
      currency: raw['currency'] as String,
      minorUnitsPerZec: raw['minorUnitsPerZec'] as int,
      at: raw['at'] as String? ?? '',
      source: raw['source'] as String?,
    );

const _rounding = {
  'up': RateRounding.up,
  'down': RateRounding.down,
  'nearest': RateRounding.nearest,
};

void main() {
  loneSurrogateTests();

  runCases('allocation.json', (c, produce) {
    produce(allocate(
      c['total'] as int,
      (c['weights'] as List).cast<int>(),
    ));
  });

  runCases('split-methods.json', (c, produce) {
    produce(splitExpense(
      c['total'] as int,
      (c['split'] as Map).cast<String, dynamic>(),
    ));
  });

  runCases('signing.json', (c, produce) {
    produce(signingMessage((c['entry'] as Map).cast<String, dynamic>()));
  });

  runCases('authority.json', (c, produce) {
    final entries = [
      for (final e in (c['log'] as List)) (e as Map).cast<String, dynamic>()
    ];
    final verified = (c['verifies'] as List).cast<String>().toSet();
    final create = entries.firstWhere((e) => e['kind'] == 'createBill');
    // The curve operation is the host's; the case says what it decided.
    final r =
        resolveIdentities(entries, create, (e, key) => _standIn(verified, e));
    produce({
      'bound': {for (final k in sortedUtf8(r.bound.keys)) k: r.bound[k]},
      'contested': sortedUtf8(r.contested),
    });
  });

  // Section 14 is addressed to a host, so none of it is reachable from the
  // wire format: an implementation can keep sections 1 to 12 and still ask a
  // payer for a debt they have already paid.
  runCases('delta.json', (c, produce) {
    final entries = (c['log'] as List).cast<Map<String, dynamic>>();
    final theyHave = {
      for (final id in c['theyHave'] as List) id as String,
    };
    final d = deltaFor(entries, theyHave);
    produce(switch (d) {
      NothingMissing() => {'state': 'nothing', 'entryCount': 0},
      DeltaSquare(:final uri, :final entryCount) => {
          'state': 'square',
          'uri': uri,
          'entryCount': entryCount,
        },
      TooBigForOneSquare(:final entryCount, :final code) => {
          'state': 'too_big',
          'entryCount': entryCount,
          'code': code,
        },
    });
  });

  runCases('withholdings.json', (c, produce) {
    final bill = decodeBill((c['bill'] as Map).cast<String, dynamic>());
    final plan = [
      for (final s in (c['plan'] as List).cast<Map<String, dynamic>>())
        Settlement(
          s['from'] as String,
          s['to'] as String,
          s['amount'] as int,
          covers: [
            for (final d in (s['covers'] as List? ?? const [])
                .cast<Map<String, dynamic>>())
              DirectDebt(
                  d['from'] as String, d['to'] as String, d['amount'] as int),
          ],
        ),
    ];
    final w = withholdings(
      plan,
      bill,
      c['payer'] as String,
      contestedIds: {
        for (final id in (c['contested'] as List?) ?? const []) id as String,
      },
      payAnyway: {
        for (final id in (c['payAnyway'] as List?) ?? const []) id as String,
      },
    );
    produce({
      'carried': [
        for (final s in w.carried)
          {
            'from': s.from,
            'to': s.to,
            'amount': s.amount,
            if (s.covers.isNotEmpty)
              'covers': [
                for (final d in s.covers)
                  {'from': d.from, 'to': d.to, 'amount': d.amount},
              ],
          },
      ],
      'awaiting': [
        for (final a in w.awaiting)
          {'to': a.to, 'owed': a.owed, 'paid': a.paid},
      ],
      'contested': [
        for (final x in w.contested)
          {'to': x.to, 'amount': x.amount, 'address': x.address},
      ],
    });
  });

  runCases('obligations.json', (c, produce) {
    final raw = (c['rate'] as Map).cast<String, dynamic>();
    final bill = Bill(
      id: 'b',
      name: '',
      currency: c['currency'] as String,
      participants: [
        for (final p
            in (c['participants'] as List).cast<Map<String, dynamic>>())
          Participant(
            id: p['id'] as String,
            name: p['name'] as String,
            payTo: p['payTo'] as String?,
            payouts: [
              for (final o
                  in (p['payouts'] as List?)?.cast<Map<String, dynamic>>() ??
                      const [])
                Payout(
                  type: o['type'] as String,
                  address: o['address'] as String?,
                  asset: o['asset'] as String?,
                  chain: o['chain'] as String?,
                ),
            ],
          ),
      ],
    );
    final settlements = [
      for (final s in (c['settlements'] as List).cast<Map<String, dynamic>>())
        Settlement(s['from'] as String, s['to'] as String, s['amount'] as int),
    ];
    final r = renderObligation(
      settlements,
      bill,
      rate: rateOf(raw),
      skipUnpayable: c['skipUnpayable'] as bool,
      includeFiat: c['includeFiat'] as bool,
    );
    produce({
      'uri': r.uri,
      'payments': [
        for (final p in r.payments)
          {'address': p.address, 'zatoshi': p.zatoshi},
      ],
      'unpayable': [
        for (final u in r.unpayable)
          {'id': u.id, 'reason': u.reason, 'minorUnits': u.minorUnits},
      ],
      'carriedMinorUnits': r.carriedMinorUnits,
      'withheldMinorUnits': r.withheldMinorUnits,
      'isComplete': r.isComplete,
    });
  });

  runCases('log.json', (c, produce) {
    if (c.containsKey('entry')) {
      checkEntry(c['entry']);
      produce({'accepted': true});
      return;
    }
    if (c.containsKey('left')) {
      final left = [
        for (final e in (c['left'] as List)) (e as Map).cast<String, dynamic>()
      ];
      final right = [
        for (final e in (c['right'] as List)) (e as Map).cast<String, dynamic>()
      ];
      Map<String, Object?> answer(MergeResult r) => {
            'merged': r.merged,
            'refused': [
              for (final a in r.refused) {'id': a.id, 'code': a.code},
            ],
          };
      // §10.2's union is commutative, which is a claim about this
      // implementation and not only about the reference that wrote the
      // expectation. Both orders must give one answer.
      final forward = answer(mergeLogs([left, right]));
      final backward = answer(mergeLogs([right, left]));
      expect(jsonEncode(backward), jsonEncode(forward),
          reason: 'merge is not commutative');
      produce(forward);
      return;
    }
    // A case listing `verifies` is driven with a verifier that accepts exactly
    // those entry ids; one without is driven with none (§10.3).
    final verifies = (c['verifies'] as List?)?.cast<String>().toSet();
    final r = foldLog(
      c['log'] as List,
      billId: c['billId'] as String?,
      verify:
          verifies == null ? null : (entry, key) => _standIn(verifies, entry),
    );
    // §9.1: the decoder carries confirmedPayments through, so the fold's
    // answer survives the round trip with no fixup here.
    final settled = decodeBill(r.bill);
    produce({
      'identities': {
        'bound': {
          for (final id in sortedUtf8(r.identities.bound.keys))
            id: r.identities.bound[id],
        },
        'contested': sortedUtf8(r.identities.contested),
      },
      'bill': r.bill,
      'creator': r.creator,
      'replacedAddresses': [
        for (final a in r.replacedAddresses)
          {'id': a.id, 'from': a.from, 'to': a.to},
      ],
      'withdrawn': r.withdrawn,
      'setAside': [
        // The reason is prose (SPEC.md §12); only the code is compared.
        for (final a in r.setAside) {'id': a.id, 'code': a.code},
      ],
      'balances': netBalances(settled),
    });
  });

  runCases('payload.json', (c, produce) {
    if (c.containsKey('encode')) {
      final spec = (c['encode'] as Map).cast<String, dynamic>();
      produce(encodePayload(
        spec['prefix'] as String,
        (spec['body'] as Map).cast<String, dynamic>(),
      ));
    } else {
      final p = decodePayload(c['payload'] as String);
      produce({
        'prefix': p.prefix,
        'version': p.version,
        'log': p.log,
        'invite': p.invite,
      });
    }
  });

  runCases('sealed.json', (c, produce) {
    final f = parseSealedFrame(c['frame'] as String);
    produce({
      'version': f.version,
      'nonce': f.nonce,
      'bodyBytes': f.bodyBytes,
    });
  });

  runCases('seal.json', (c, produce) {
    // Either an entry to seal, or a bill id to derive a channel from.
    if (c.containsKey('billId')) {
      produce({'channel': channelFor(c['billId'] as String)});
      return;
    }
    final plaintext = sealedPlaintext(c['entry'] as Map<String, dynamic>);
    produce({
      'plaintext': utf8.decode(plaintext),
      'nonce': base64UrlEncode(sealedNonce(plaintext)).replaceAll('=', ''),
    });
  });

  runCases('invite.json', (c, produce) {
    if (c.containsKey('uri')) {
      final invite = parseInvite(c['uri'] as String);
      produce({
        'version': inviteVersion,
        'billId': invite.billId,
        'key': invite.key,
        'name': invite.name,
        if (invite.expiry != null) 'expiry': invite.expiry,
      });
    } else {
      final raw = (c['invite'] as Map).cast<String, dynamic>();
      produce(renderInvite(Invite(
        billId: raw['billId'] as String,
        key: raw['key'] as String,
        name: raw['name'] as String? ?? '',
        expiry: raw['expiry'] as int?,
      )));
    }
  });

  runCases('balances.json', (c, produce) {
    // §9.1: a document that omits confirmedPayments has confirmed nothing.
    final settled = decodeBill(c['bill']);
    final net = netBalances(settled);
    produce({
      'net': net,
      'creditors': [
        for (final p in creditors(net)) {'id': p.id, 'amount': p.amount},
      ],
      'debtors': [
        for (final p in debtors(net)) {'id': p.id, 'amount': p.amount},
      ],
      'directDebts': [
        for (final d in directDebts(settled))
          {'from': d.from, 'to': d.to, 'amount': d.amount},
      ],
    });
  });

  runCases('bill-json.json', (c, produce) {
    produce(billToJson(decodeBill(c['json'])));
  });

  runCases('settlement.json', (c, produce) {
    final net = (c['balances'] as Map).cast<String, int>();
    final plan = settleBalances(net, exactLimit: c['exactLimit'] as int);
    produce({
      'settlements': [
        for (final s in plan.settlements)
          {'from': s.from, 'to': s.to, 'amount': s.amount},
      ],
      'isOptimal': plan.isOptimal,
      'paymentCount': plan.paymentCount,
    });
  });

  runCases('coverage.json', (c, produce) {
    final limit = c['exactLimit'] as int;
    // A case carries a bill or bare balances: §6.3 coverage needs the debts,
    // and net balances do not carry them.
    final plan = c['bill'] != null
        ? settleBill(decodeBill(c['bill']), exactLimit: limit)
        : settleBalances((c['balances'] as Map).cast<String, int>(),
            exactLimit: limit);
    produce({
      'settlements': [
        for (final s in plan.settlements)
          {
            'from': s.from,
            'to': s.to,
            'amount': s.amount,
            'covers': [
              for (final d in s.covers)
                {'from': d.from, 'to': d.to, 'amount': d.amount},
            ],
            'rerouted': s.isRerouted,
            'unexplained': s.unexplained,
          },
      ],
      'isOptimal': plan.isOptimal,
      'paymentCount': plan.paymentCount,
    });
  });

  runCases('zip321.json', (c, produce) {
    final count = c['paymentCount'] as int?;
    final List<Zip321Payment> payments;
    if (count != null) {
      // Carried as a count rather than a literal list; §8.2's index cap.
      final repeat = (c['repeatPayment'] as Map).cast<String, dynamic>();
      payments = [
        for (var i = 0; i < count; i++)
          Zip321Payment(
            address: repeat['address'] as String,
            zatoshi: (repeat['zatoshiFrom'] as int) + i,
          ),
      ];
    } else {
      payments = [
        for (final raw in (c['payments'] as List).cast<Map<String, dynamic>>())
          Zip321Payment(
            address: raw['address'] as String,
            zatoshi: raw['zatoshi'] as int,
            fiat: raw['fiat'] == null
                ? null
                : FiatPrice((raw['fiat'] as List)[0] as String,
                    (raw['fiat'] as List)[1] as int),
            memo:
                raw['memo'] == null ? null : utf8.encode(raw['memo'] as String),
            label: raw['label'] as String?,
            message: raw['message'] as String?,
          ),
      ];
    }
    final uri =
        renderUri(payments, includeFiat: c['includeFiat'] as bool? ?? false);
    if (c.containsKey('expectLength')) {
      expect(uri.length, c['expectLength'], reason: 'URI length');
      produce(null);
    } else {
      produce(uri);
    }
  });

  runCases('rate.json', (c, produce) {
    final rate = rateOf((c['rate'] as Map).cast<String, dynamic>());
    if (c['direction'] == 'zatoshiToFiat') {
      produce(zatoshiToFiat(c['zatoshi'] as int, rate));
    } else {
      produce(fiatToZatoshi(
        c['minorUnits'] as int,
        rate,
        amountCurrency: c['amountCurrency'] as String?,
        rounding: _rounding[c['rounding'] as String]!,
      ));
    }
  });
}

/// §2.3. A lone surrogate has no UTF-8 encoding, so two ids that are not equal
/// compare equal once encoded and "ascending id" stops being a total order.
///
/// This cannot live in `vectors/`: a conformant JSON reader refuses a document
/// carrying one, so the vector file itself would be unparseable (§12).
void loneSurrogateTests() {
  const high = '\uD800'; // a high surrogate with nothing after it
  const low = '\uDC00'; // a low surrogate with nothing before it

  // §10.1's depth bound, past what a JSON reader's own recursion limit
  // admits. This cannot be a corpus vector: the file would fail to parse and
  // take the whole corpus down rather than test one rule, which is the reason
  // §12 gives for `bill_not_scalar_values`. Built here in code instead.
  test('an entry nested far past the limit is refused, not a stack overflow',
      () {
    Object? value = 1;
    for (var i = 0; i < 20000; i++) {
      value = [value];
    }
    final entry = <String, dynamic>{
      'v': 1,
      'id': 'x',
      'author': 'ana',
      'kind': 'addExpense',
      'at': '2026-10-28T19:30:00.000Z',
      'expense': {
        'id': 'x1',
        'paidBy': 'ana',
        'amount': 1,
        'at': '2026-10-28T19:30:00.000Z',
        'split': {
          'type': 'equal',
          'among': ['ana']
        },
        'note': value,
      },
    };
    expect(
        () => checkEntry(entry),
        throwsA(isA<SplitError>()
            .having((e) => e.code, 'code', SplitCode.billTypeError)),
        reason: 'a relay entry never passes §11.2, so §10.1 has to bound it');
  });

  test('a lone surrogate is not a scalar value', () {
    expect(hasLoneSurrogate(high), isTrue);
    expect(hasLoneSurrogate(low), isTrue);
    expect(hasLoneSurrogate('ana'), isFalse);
    expect(hasLoneSurrogate('😀'), isFalse, reason: 'a well-formed pair');
  });

  test('two ids that differ compare equal once encoded', () {
    expect(high == low, isFalse);
    expect(compareUtf8(high, low), 0,
        reason: 'the hazard §2.3 names: not equal, yet ordered the same');
  });

  test('decodeBill refuses a document carrying one', () {
    expect(
      refusalOf(() => decodeBill({
            'v': billVersion,
            'id': 'b',
            'name': '',
            'currency': 'EUR',
            'participants': [
              {'id': high, 'name': 'A'},
              {'id': low, 'name': 'B'},
            ],
          })),
      SplitCode.billNotScalarValues,
    );
  });

  test('checkEntry refuses one at log ingress', () {
    expect(
      refusalOf(() => checkEntry({
            'v': billVersion,
            'id': 'j1',
            'author': 'ana',
            'kind': 'joinBill',
            'at': '2026-10-28T19:30:00.000Z',
            'participant': {'id': high, 'name': 'A'},
          })),
      SplitCode.billNotScalarValues,
    );
  });
}

/// The vectors' stand-in for the host's curve operation.
///
/// An item names an entry id, and every copy of that entry verifies; or an id
/// and a signature joined by `|`, and only that copy does. The key is not
/// consulted: a case states which copies verify against the key the fold asks
/// about.
bool _standIn(Set<String> verifies, Map<String, dynamic> entry) =>
    verifies.contains(entry['id']) ||
    (entry['sig'] is String &&
        verifies.contains('${entry['id']}|${entry['sig']}'));
