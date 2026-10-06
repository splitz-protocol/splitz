/// §14.10: one transaction pays a request's ZEC payees and one swap deposit,
/// and its note records each half after a restart.
library;

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

/// `tools/corpus/_spec.py` ADDRESSES[1], so the request renders (§8.6).
const caiZec =
    'u1nztelxna9h7w0vtpd2xjhxt4lpu8s9cmdl8n8vcr7actf2ny45nd07cy8cyuhuvw3axcp545y0ktq9cezuzx84jyhex8dk4tdvwhu4dl';
const benBase = '0xben000000000000000000000000000000000000';

const usdcBase = TradableAsset(
  assetId: 'nep141:base-usdc',
  symbol: 'USDC',
  chain: 'base',
  decimals: 6,
);

/// Ana owes Ben 40.00 EUR, paid in USDC on Base, and Cai 20.00 EUR, paid in
/// ZEC. Closed for settling.
({
  splitz.FoldedBill folded,
  splitz.PayerObligation obligation,
  FakeHost ana,
  List<Map<String, dynamic>> entries,
})
_bill() {
  final hosts = {
    for (final id in ['ana', 'ben', 'cai']) id: FakeHost(me: id),
  };
  FakeHost at(String id) {
    for (final h in hosts.values) {
      h.tick();
    }
    return hosts[id]!;
  }

  final entries = <Map<String, dynamic>>[
    splitz.createBill(
      host: at('ana'),
      name: 'Trip',
      currency: 'EUR',
      creatorKey: fakeKey('ana'),
    ),
    splitz.joinBill(host: at('ana'), name: 'Ana', payTo: caiZec),
    splitz.joinBill(
      host: at('ben'),
      name: 'Ben',
      payouts: [
        {'type': 'swap', 'address': benBase, 'asset': 'USDC', 'chain': 'base'},
      ],
    ),
    splitz.joinBill(host: at('cai'), name: 'Cai', payTo: caiZec),
    splitz.addExpense(
      host: at('ben'),
      expenseId: 'x1',
      paidBy: 'ben',
      amount: 8000,
      split: {
        'type': 'equal',
        'among': ['ana', 'ben'],
      },
    ),
    splitz.addExpense(
      host: at('cai'),
      expenseId: 'x2',
      paidBy: 'cai',
      amount: 4000,
      split: {
        'type': 'equal',
        'among': ['ana', 'cai'],
      },
    ),
    splitz.setRate(host: at('ana'), currency: 'EUR', minorUnitsPerZec: 51234),
  ];
  final log = splitz.BillLog(hosts['ana']!, entries: entries);
  entries.add(splitz.closeFor(at('ana'), log.fold()));
  final folded = splitz.BillLog(hosts['ana']!, entries: entries).fold();
  return (
    folded: folded,
    obligation: splitz.obligationFor(hosts['ana']!, folded)!,
    ana: hosts['ana']!,
    entries: entries,
  );
}

/// Ben's 40.00 EUR at the bill's 512.34 EUR a ZEC, rounded up as a deposit is
/// sized.
final _deposit = protocol.fiatToZatoshi(
  4000,
  const protocol.ExchangeRate(
    currency: 'EUR',
    minorUnitsPerZec: 51234,
    at: '2026-10-28T19:30:00Z',
  ),
  amountCurrency: 'EUR',
);

SwapQuote _quote({
  String? memo,
  String deadline = '2099-01-01T00:00:00.000Z',
  String recipient = benBase,
  int? zatoshi,
}) => SwapQuote(
  depositAddress: 't1deposit000000000000000000000000',
  amountInZatoshi: zatoshi ?? _deposit,
  amountOut: '40000000',
  asset: usdcBase,
  deadline: deadline,
  recipient: recipient,
  reference: 'intent-1',
  depositMemo: memo,
);

SwapDeposit _combined(
  ({
    splitz.FoldedBill folded,
    splitz.PayerObligation obligation,
    FakeHost ana,
    List<Map<String, dynamic>> entries,
  })
  b, {
  SwapQuote? quote,
  String to = 'ben',
  int amount = 4000,
}) => combinedSend(
  billId: b.folded.bill.id,
  bill: b.folded.bill,
  obligation: b.obligation,
  quote: quote ?? _quote(),
  to: to,
  amountMinorUnits: amount,
  at: '2026-10-28T20:00:00.000Z',
);

void main() {
  test('one request carries every ZEC payee and the deposit', () {
    final b = _bill();
    expect(b.obligation.carriedTo, {'cai': 2000});
    expect(b.obligation.unpayable.single.id, 'ben');
    final sent = _combined(b);
    final outputs = Uri.parse(
      sent.uri,
    ).queryParameters.keys.where((k) => k.startsWith('address')).length;
    expect(outputs, 2, reason: 'Cai and the deposit');
    expect(sent.uri, contains('t1deposit'));
    expect(sent.uri, contains(caiZec));
    expect(sent.note.carried, {'cai': 2000, 'ben': 4000});
    expect(sent.note.sent, b.obligation.carriedZatoshi);
    expect(sent.note.zatoshi, _deposit);
    expect(sent.note.swap!.to, 'ben');
  });

  test('a deposit that needs a memo, and a payee the request already pays, '
      'are refused', () {
    final b = _bill();
    expect(
      () => _combined(b, quote: _quote(memo: 'needed')),
      throwsA(isA<SwapRefused>()),
    );
    expect(
      () => _combined(b, to: 'cai', amount: 2000),
      throwsA(isA<SwapException>()),
      reason: 'Cai is already in the request',
    );
  });

  test('a swap leg a deposit alone would be refused is refused here too, '
      'and the honest one beside them is not', () {
    final b = _bill();
    SwapSendRefused refused(SwapQuote q) {
      try {
        _combined(b, quote: q);
      } on SwapRefused catch (e) {
        return e.refusal.refused;
      }
      fail('not refused');
    }

    expect(
      refused(_quote(deadline: '2020-01-01T00:00:00.000Z')),
      SwapSendRefused.expired,
    );
    expect(refused(_quote(zatoshi: _deposit - 1)), SwapSendRefused.rateChanged);
    expect(
      refused(_quote(recipient: '0xsomebodyelse')),
      SwapSendRefused.recipientChanged,
    );
    expect(refused(_quote(memo: 'needed')), SwapSendRefused.needsMemo);
    expect(_combined(b).note.swap!.to, 'ben');
  });

  test("a restart records the request's half by its transaction id, and leaves "
      'the swap to its own record', () async {
    final b = _bill();
    final sent = _combined(b);
    final store = PendingSends(InMemoryBillStorage());
    final log = splitz.BillLog(
      b.ana,
      entries: b.entries,
      billId: b.folded.bill.id,
    );
    final records = await store.recordsFor(b.ana, log, sent.note, 'a' * 64);
    expect(
      [for (final r in records) (r['payment'] as Map)['to']],
      ['cai'],
      reason: 'the swap is recorded by its reference, not this txid',
    );
  });

  test('a deposit sent alone is still recorded by its reference', () async {
    final b = _bill();
    final alone = swapDeposit(
      billId: b.folded.bill.id,
      quote: _quote(),
      to: 'ben',
      amountMinorUnits: 4000,
      rate: b.obligation.rate,
      at: '2026-10-28T20:00:00.000Z',
    );
    final store = PendingSends(InMemoryBillStorage());
    final log = splitz.BillLog(
      b.ana,
      entries: b.entries,
      billId: b.folded.bill.id,
    );
    await expectLater(
      store.recordsFor(b.ana, log, alone.note, 'a' * 64),
      throwsA(
        isA<Unrecordable>().having(
          (e) => e.reason,
          'reason',
          UnrecordableReason.isASwap,
        ),
      ),
    );
  });

  test("the word that nothing left is held by a transaction sending both "
      'halves, and not by one sending something else', () {
    final b = _bill();
    final sent = _combined(b);
    final both =
        sent.note.sent.values.fold<int>(0, (n, z) => n + z) +
        sent.note.zatoshi!;
    OwnTransaction tx(String id, int amount) => OwnTransaction(
      txid: id * 64,
      created: '2026-10-28T20:00:05.000Z',
      sent: amount,
    );
    expect(
      unsentClaimRefusal(
        sent.note,
        stillSending: false,
        own: [tx('b', both)],
      )?.claim,
      UnsentClaim.builtSince,
    );
    expect(
      unsentClaimRefusal(
        sent.note,
        stillSending: false,
        own: [tx('c', both + 1)],
      ),
      isNull,
    );
  });
}
