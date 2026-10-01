/// §14.2's review-screen check, against a screen that shows every fact and
/// against the same screen with each fact taken away in turn.
library;

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

const benOld = 'u1benold0000000000000000';
const ben = 'u1ben1111111111111111111';
const dan = 'u1dan3333333333333333333';
const eve = 'u1eve4444444444444444444';
const eveLater = 'u1eve5555555555555555555';

/// Ana owes Ben 40.00, Cat 30.00, Dan 20.00 and Eve 10.00 (EUR). Ben moved
/// his address, Cat has none, Ana has recorded paying Dan, and Eve set the
/// rate. The request Ana sends carries Ben and Eve. Eve declares a second
/// address after her first, which [via] can choose (§14.8).
({splitz.PayerObligation obligation, splitz.FoldedBill folded}) bill({
  Map<String, int> via = const {},
}) {
  final hosts = {
    for (final id in ['ana', 'ben', 'cat', 'dan', 'eve']) id: FakeHost(me: id),
  };
  final ticks = <String, int>{};
  var step = 0;
  FakeHost at(String id) {
    step++;
    final h = hosts[id]!;
    while ((ticks[id] ?? 0) < step) {
      h.tick();
      ticks[id] = (ticks[id] ?? 0) + 1;
    }
    return h;
  }

  Map<String, dynamic> spent(String who, int amount, String id) =>
      splitz.addExpense(
        host: at(who),
        expenseId: id,
        paidBy: who,
        amount: amount,
        split: {
          'type': 'equal',
          'among': ['ana', who],
        },
      );

  final entries = [
    splitz.createBill(
      host: at('ana'),
      name: 'Trip',
      currency: 'EUR',
      creatorKey: fakeKey('ana'),
    ),
    splitz.joinBill(host: at('ana'), name: 'Ana', payTo: 'u1ana0000000000000'),
    splitz.joinBill(host: at('ben'), name: 'Ben', payTo: benOld),
    splitz.joinBill(host: at('cat'), name: 'Cat'),
    splitz.joinBill(host: at('dan'), name: 'Dan', payTo: dan),
    splitz.joinBill(
      host: at('eve'),
      name: 'Eve',
      payouts: [
        {'type': 'zec', 'address': eve},
        {'type': 'zec', 'address': eveLater},
      ],
    ),
    splitz.joinBill(host: at('ben'), name: 'Ben', payTo: ben),
    spent('ben', 8000, 'x1'),
    spent('cat', 6000, 'x2'),
    spent('dan', 4000, 'x3'),
    spent('eve', 2000, 'x4'),
    splitz.recordPayment(
      host: at('ana'),
      paymentId: 'p1',
      to: 'dan',
      amount: 2000,
    ),
    splitz.setRate(host: at('eve'), currency: 'EUR', minorUnitsPerZec: 51234),
  ];
  final log = splitz.BillLog(hosts['ana']!)..add(entries);
  final folded = log.fold();
  expect(folded.setAside, isEmpty);
  return (
    obligation: splitz.obligationVia(hosts['ana']!, folded, via)!,
    folded: folded,
  );
}

const reasons = {'no_address': 'has no address'};

/// One line per fact, so taking a line away takes exactly one fact away.
const screen = [
  'Cat', // unpayable: who
  'has no address', // unpayable: why
  'Ben', // replaced address
  'Dan', // awaiting
  '512.34', // rate figure
  'Eve', // rate author
  '0.07807316', // Ben's output
  'u1ben11111…', // Ben's address, the first 10 characters
  '0.01951829', // Eve's output
  eve, // Eve's address, whole
];

void main() {
  test('the facts are the ones this bill produces', () {
    final b = bill();
    expect(b.obligation.unpayable.map((u) => (u.id, u.reason)), [
      ('cat', 'no_address'),
    ]);
    expect(b.folded.replacedAddresses.map((r) => r.id), ['ben']);
    expect(b.obligation.awaiting.map((a) => a.to), ['dan']);
    expect(b.folded.rateAuthor, 'eve');
    expect(b.obligation.request.recipients, ['ben', 'eve']);
    expect(b.obligation.request.payments.map((p) => (p.address, p.zatoshi)), [
      (ben, 7807316),
      (eve, 1951829),
    ]);
  });

  test('a screen showing every fact passes', () {
    final b = bill();
    expect(
      checkPayerReview(
        obligation: b.obligation,
        folded: b.folded,
        visibleText: screen,
        reasonWords: reasons,
      ),
      isEmpty,
    );
  });

  test('each fact taken away is exactly its own finding', () {
    final b = bill();
    const expected = [
      (ReviewRule.unpayable, 'Cat'),
      (ReviewRule.unpayable, 'has no address'),
      (ReviewRule.replacedAddress, 'Ben'),
      (ReviewRule.awaiting, 'Dan'),
      (ReviewRule.rate, '512.34'),
      (ReviewRule.rate, 'Eve'),
      (ReviewRule.output, '0.07807316'),
      (ReviewRule.output, ben),
      (ReviewRule.output, '0.01951829'),
      (ReviewRule.output, eve),
    ];
    for (var i = 0; i < screen.length; i++) {
      final shown = [...screen]..removeAt(i);
      final found = checkPayerReview(
        obligation: b.obligation,
        folded: b.folded,
        visibleText: shown,
        reasonWords: reasons,
      );
      expect(found.map((f) => (f.rule, f.expected)), [
        expected[i],
      ], reason: 'without "${screen[i]}"');
    }
  });

  test('an amount inside a longer number is not shown', () {
    final b = bill();
    final shown = [...screen]..[6] = '0.078073169';
    expect(
      checkPayerReview(
        obligation: b.obligation,
        folded: b.folded,
        visibleText: shown,
        reasonWords: reasons,
      ).map((f) => f.expected),
      ['0.07807316'],
    );
  });

  test('a different address sharing the first ten characters is not '
      'shown', () {
    final b = bill();
    final shown = [...screen]..[7] = 'u1ben1111122222…';
    expect(
      checkPayerReview(
        obligation: b.obligation,
        folded: b.folded,
        visibleText: shown,
        reasonWords: reasons,
      ).map((f) => f.expected),
      [ben],
    );
  });

  test('a reason the wallet gives no words for is a finding', () {
    final b = bill();
    expect(
      checkPayerReview(
        obligation: b.obligation,
        folded: b.folded,
        visibleText: screen,
        reasonWords: const {},
      ).map((f) => (f.rule, f.expected)),
      [(ReviewRule.unpayable, 'no_address')],
    );
  });

  test('the rate figure keeps the currency\'s fractional digits', () {
    protocol.ExchangeRate rate(String currency, int units) =>
        protocol.ExchangeRate(
          currency: currency,
          minorUnitsPerZec: units,
          at: '2026-10-28T19:30:00.000Z',
        );
    expect(rateFigure(rate('EUR', 51234)), '512.34');
    expect(rateFigure(rate('EUR', 5)), '0.05');
    expect(rateFigure(rate('EUR', 5000)), '50.00');
    expect(rateFigure(rate('JPY', 7000)), '7000');
    expect(rateFigure(rate('BHD', 12345)), '12.345');
  });

  group('a lower preference (§14.8)', () {
    const lower = 'by a later choice';
    const eveSecond = {'eve': 1};
    final lowerScreen = [...screen]
      ..[9] = eveLater
      ..add(lower);

    List<(ReviewRule, String)> found(
      Map<String, int> via,
      Map<String, int> checked,
      List<String> shown,
      String words,
    ) {
      final b = bill(via: via);
      return checkPayerReview(
        obligation: b.obligation,
        folded: b.folded,
        visibleText: shown,
        reasonWords: reasons,
        via: checked,
        lowerWords: words,
      ).map((f) => (f.rule, f.expected)).toList();
    }

    test('is shown with the wallet\'s words', () {
      expect(
        bill(via: eveSecond).obligation.request.payments[1].address,
        eveLater,
      );
      expect(found(eveSecond, eveSecond, lowerScreen, lower), isEmpty);
      expect(
        found(eveSecond, eveSecond, [...lowerScreen]..removeLast(), lower),
        [(ReviewRule.lowerPreference, lower)],
      );
      expect(found(eveSecond, eveSecond, lowerScreen, ''), [
        (ReviewRule.lowerPreference, 'lower_preference'),
      ]);
    });

    test('names the recipient', () {
      // Eve's name is on the screen twice over — she set the rate — so the
      // name is removed from both lines to see the rule ask for it.
      final shown = lowerScreen.where((l) => l != 'Eve').toList();
      expect(found(eveSecond, eveSecond, shown, lower), [
        (ReviewRule.lowerPreference, 'Eve'),
        (ReviewRule.rate, 'Eve'),
      ]);
    });

    test('a first choice or somebody not paid needs nothing shown', () {
      expect(found(const {}, const {'eve': 0, 'dan': 1}, screen, ''), isEmpty);
    });
  });

  group("the payee's side", () {
    const txid =
        '1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809';
    protocol.PaymentRecord record(
      String method, {
      int? zatoshi,
      bool rate = false,
      String? reference,
    }) => protocol.PaymentRecord(
      id: 'p1',
      from: 'ana',
      to: 'ben',
      amount: 4000,
      currency: 'EUR',
      method: method,
      at: '2026-10-28T19:30:00.000Z',
      zatoshi: zatoshi,
      paidAtRate: rate
          ? const protocol.ExchangeRate(
              currency: 'EUR',
              minorUnitsPerZec: 51234,
              at: '2026-10-28T19:30:00.000Z',
            )
          : null,
      reference: reference,
    );
    const payeeScreen = [
      'Ana sent 0.07807316 ZEC',
      'at 512.34 EUR a ZEC',
      'tx 1a2b3c4d5e6f…c4d5e6f809',
    ];
    List<(ReviewRule, String)> payee(
      protocol.PaymentRecord p,
      List<String> shown, [
      String absent = '',
    ]) => checkPayeeReview(
      payment: p,
      visibleText: shown,
      absentWords: absent,
    ).map((f) => (f.rule, f.expected)).toList();

    final zec = record(
      'shieldedZec',
      zatoshi: 7807316,
      rate: true,
      reference: txid,
    );

    test('a reference prefix counts characters, not UTF-16 units or bytes', () {
      const none = 'ZEC and rate: not recorded';
      final fifteen = 'é' * 15;
      final swap = record('swap', reference: fifteen);
      expect(payee(swap, ['ref ${'é' * 5}… ok', none], 'not recorded'), [
        (ReviewRule.payeeReference, fifteen),
      ]);
      expect(
        payee(swap, ['ref ${'é' * 10}… ok', none], 'not recorded'),
        isEmpty,
      );
      final mixed = record('swap', reference: 'abcdefghiéxyzxyz');
      expect(payee(mixed, ['ref abcdefghié…', none], 'not recorded'), isEmpty);
      final emoji = record('swap', reference: '${'😀' * 12}x');
      expect(payee(emoji, ['ref ${'😀' * 5}…', none], 'not recorded'), [
        (ReviewRule.payeeReference, '${'😀' * 12}x'),
      ]);
    });

    test(
      'a screen showing the record passes; each missing figure is named',
      () {
        expect(payee(zec, payeeScreen), isEmpty);
        const expected = [
          (ReviewRule.payeeZec, '0.07807316'),
          (ReviewRule.payeeRate, '512.34'),
          (ReviewRule.payeeReference, txid),
        ];
        for (var i = 0; i < payeeScreen.length; i++) {
          final shown = [...payeeScreen]..removeAt(i);
          final missing = [expected[i]];
          expect(
            payee(zec, shown),
            missing,
            reason: 'without "${payeeScreen[i]}"',
          );
        }
      },
    );

    test('a different reference sharing its first ten characters is not '
        'shown', () {
      expect(payee(zec, [...payeeScreen]..[2] = 'tx 1a2b3c4d5e00…'), [
        (ReviewRule.payeeReference, txid),
      ]);
    });

    test(
      'a zec record missing a figure must say so in the wallet\'s words',
      () {
        final bare = record('shieldedZec', zatoshi: 7807316);
        const zecOnly = ['Ana sent 0.07807316 ZEC'];
        const said = [...zecOnly, 'rate and reference: not recorded'];
        expect(payee(bare, said, 'not recorded'), isEmpty);
        expect(payee(bare, zecOnly, 'not recorded'), [
          (ReviewRule.payeeRate, 'not recorded'),
          (ReviewRule.payeeReference, 'not recorded'),
        ]);
        expect(payee(bare, said), [
          (ReviewRule.payeeRate, 'absent'),
          (ReviewRule.payeeReference, 'absent'),
        ]);
      },
    );

    test('a cash record needs nothing shown', () {
      expect(payee(record('cash'), const []), isEmpty);
    });
  });
}
