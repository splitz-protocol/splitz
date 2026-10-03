/// §15.7's two swap rules: a deposit checked against the bill as stored
/// before it is sent, and a failed swap's record withdrawn.
library;

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

const benBase = '0xben000000000000000000000000000000000000';
const benArb = '0xbenarb0000000000000000000000000000000000';
const reference = 'intent-1';

const usdcBase = TradableAsset(
  assetId: 'nep141:base-usdc',
  symbol: 'USDC',
  chain: 'base',
  decimals: 6,
);

const rate = protocol.ExchangeRate(
  currency: 'EUR',
  minorUnitsPerZec: 51234,
  at: '2026-10-28T19:40:00.000Z',
);

/// Ana owes Ben 40.00 EUR, and Ben is paid in USDC: on Base first, on
/// Arbitrum second. [paid] adds Ana's swap record to Ben under [reference],
/// [paidBy] who wrote it, [method] its method, and [confirmed] Ben's
/// confirmation of it.
({splitz.FoldedBill folded, splitz.PayerObligation? obligation, String? entry})
bill({
  bool paid = false,
  String paidBy = 'ana',
  String method = 'swap',
  bool confirmed = false,
  bool priced = true,
  Map<String, int> via = const {},
}) {
  final hosts = {
    for (final id in ['ana', 'ben']) id: FakeHost(me: id),
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
    splitz.joinBill(host: at('ana'), name: 'Ana', payTo: 'u1ana0000000000000'),
    splitz.joinBill(
      host: at('ben'),
      name: 'Ben',
      payouts: [
        {'type': 'swap', 'address': benBase, 'asset': 'USDC', 'chain': 'base'},
        {'type': 'swap', 'address': benArb, 'asset': 'USDC', 'chain': 'arb'},
      ],
    ),
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
    if (priced)
      splitz.setRate(host: at('ana'), currency: 'EUR', minorUnitsPerZec: 51234),
  ];
  String? entry;
  if (paid) {
    final record = splitz.recordPayment(
      host: at(paidBy),
      paymentId: 'p1',
      to: paidBy == 'ana' ? 'ben' : 'ana',
      amount: 4000,
      method: method,
      reference: reference,
    );
    entry = record['id'] as String;
    entries.add(record);
  }
  final log = splitz.BillLog(hosts['ana']!)..add(entries);
  if (confirmed) {
    final paymentId = splitz.authoredId(paidBy, 'p1');
    log.add([
      splitz.confirmPayment(
        host: at('ben'),
        paymentId: paymentId,
        method: 'recipientConfirmed',
        record: log.fold().paymentDigests[paymentId]!,
      ),
    ]);
  }
  final folded = log.fold();
  expect(folded.setAside, isEmpty);
  return (
    folded: folded,
    obligation: splitz.obligationVia(hosts['ana']!, folded, via),
    entry: entry,
  );
}

final int zatoshi = protocol.fiatToZatoshi(4000, rate, amountCurrency: 'EUR');

SwapQuote quote({
  String deadline = '2026-10-28T20:30:00.000Z',
  String? memo,
  String? recipient = benBase,
  TradableAsset asset = usdcBase,
  int? amountInZatoshi,
}) => SwapQuote(
  depositAddress: 't1deposit000000000000000000000000',
  amountInZatoshi: amountInZatoshi ?? zatoshi,
  amountOut: '39990000',
  asset: asset,
  deadline: deadline,
  depositMemo: memo,
  reference: reference,
  recipient: recipient,
);

const now = '2026-10-28T20:00:00.000Z';

SwapSendRefused? refused(
  SwapQuote q, {
  String at = now,
  int amount = 4000,
  protocol.Payout? chosen,
  bool paid = false,
  bool priced = true,
  bool withObligation = true,
}) {
  final b = bill(paid: paid, priced: priced);
  final payouts = b.folded.bill.participant('ben')!.payouts;
  final index = chosen == null ? null : declaredPayoutIndex(payouts, chosen);
  final read = index == null
      ? b
      : bill(paid: paid, priced: priced, via: {'ben': index});
  return swapSendRefusal(
    q,
    now: at,
    bill: read.folded.bill,
    obligation: withObligation ? read.obligation : null,
    to: 'ben',
    amountMinorUnits: amount,
    chosen: chosen,
  )?.refused;
}

void main() {
  test('the figures are the ones this bill produces', () {
    final b = bill();
    expect(zatoshi, 7807316);
    expect(b.obligation!.unpayable.map((u) => (u.id, u.reason, u.minorUnits)), [
      ('ben', 'payout_not_zec', 4000),
    ]);
  });

  group('swapSendRefusal', () {
    test('a quote that still answers the bill may be sent', () {
      expect(refused(quote()), isNull);
    });

    test('an expired quote is refused, at its deadline and after', () {
      expect(
        refused(quote(), at: '2026-10-28T20:30:00.000Z'),
        SwapSendRefused.expired,
      );
      expect(
        refused(quote(), at: '2026-10-28T21:00:00.000Z'),
        SwapSendRefused.expired,
      );
      expect(refused(quote(), at: '2026-10-28T20:29:59.999Z'), isNull);
    });

    test('a deposit that needs a memo is refused; an empty one is none', () {
      expect(refused(quote(memo: '12345')), SwapSendRefused.needsMemo);
      expect(refused(quote(memo: '')), isNull);
    });

    test('expiry is reported before the memo', () {
      expect(
        refused(quote(memo: '1'), at: '2026-10-28T20:30:00.000Z'),
        SwapSendRefused.expired,
      );
    });

    test('a payout the payee no longer declares is refused', () {
      const gone = protocol.Payout(
        type: 'swap',
        address: '0xgone',
        asset: 'USDC',
        chain: 'base',
      );
      expect(refused(quote(), chosen: gone), SwapSendRefused.payoutGone);
      // Matched on all four fields: the declared address on another chain
      // is not declared.
      const otherChain = protocol.Payout(
        type: 'swap',
        address: benBase,
        asset: 'USDC',
        chain: 'arb',
      );
      expect(refused(quote(), chosen: otherChain), SwapSendRefused.payoutGone);
    });

    test('the payout chosen is the one the quote is held to', () {
      const second = protocol.Payout(
        type: 'swap',
        address: benArb,
        asset: 'USDC',
        chain: 'arb',
      );
      const usdcArb = TradableAsset(
        assetId: 'nep141:arb-usdc',
        symbol: 'USDC',
        chain: 'arb',
        decimals: 6,
      );
      expect(
        refused(
          quote(recipient: benArb, asset: usdcArb),
          chosen: second,
        ),
        isNull,
      );
      // The first payout's quote no longer answers once the second is
      // chosen.
      expect(
        refused(quote(), chosen: second),
        SwapSendRefused.recipientChanged,
      );
    });

    test('a debt already paid and waiting is held, naming who confirms', () {
      final b = bill(paid: true);
      final r = swapSendRefusal(
        quote(),
        now: now,
        bill: b.folded.bill,
        obligation: b.obligation,
        to: 'ben',
        amountMinorUnits: 4000,
      );
      expect(r?.refused, SwapSendRefused.held);
      expect(r?.paidTo, ['ben']);
    });

    test('a debt no longer owed in exactly the quoted amount is refused', () {
      expect(refused(quote(), amount: 3999), SwapSendRefused.notOwed);
      expect(refused(quote(), amount: 4001), SwapSendRefused.notOwed);
      expect(refused(quote(), withObligation: false), SwapSendRefused.notOwed);
    });

    test('an unpriced bill owes nothing a quote can pay', () {
      expect(refused(quote(), priced: false), SwapSendRefused.notOwed);
    });

    test('a quote for another address is refused', () {
      expect(
        refused(quote(recipient: '0xbenold')),
        SwapSendRefused.recipientChanged,
      );
      expect(refused(quote(recipient: null)), SwapSendRefused.recipientChanged);
    });

    test('a quote for another asset or chain is refused; case is not', () {
      const usdcEth = TradableAsset(
        assetId: 'nep141:eth-usdc',
        symbol: 'USDC',
        chain: 'eth',
        decimals: 6,
      );
      const usdtBase = TradableAsset(
        assetId: 'nep141:base-usdt',
        symbol: 'USDT',
        chain: 'base',
        decimals: 6,
      );
      const lower = TradableAsset(
        assetId: 'nep141:base-usdc',
        symbol: 'usdc',
        chain: 'BASE',
        decimals: 6,
      );
      expect(refused(quote(asset: usdcEth)), SwapSendRefused.assetChanged);
      expect(refused(quote(asset: usdtBase)), SwapSendRefused.assetChanged);
      expect(refused(quote(asset: lower)), isNull);
    });

    test('a quote priced at another rate is refused', () {
      expect(
        refused(quote(amountInZatoshi: zatoshi + 1)),
        SwapSendRefused.rateChanged,
      );
      expect(
        refused(quote(amountInZatoshi: zatoshi - 1)),
        SwapSendRefused.rateChanged,
      );
    });
  });

  group('failedSwapWithdrawals', () {
    test("the payer's unconfirmed swap record is withdrawn", () {
      final b = bill(paid: true);
      expect(failedSwapWithdrawals(b.folded, me: 'ana', reference: reference), [
        b.entry,
      ]);
    });

    test('a record under another reference is left', () {
      final b = bill(paid: true);
      expect(
        failedSwapWithdrawals(b.folded, me: 'ana', reference: 'intent-2'),
        isEmpty,
      );
    });

    test('a confirmed record is left', () {
      final b = bill(paid: true, confirmed: true);
      expect(b.folded.bill.confirmedPayments, isNotEmpty);
      expect(
        failedSwapWithdrawals(b.folded, me: 'ana', reference: reference),
        isEmpty,
      );
    });

    test('a record somebody else wrote is theirs to withdraw', () {
      final b = bill(paid: true, paidBy: 'ben');
      expect(
        failedSwapWithdrawals(b.folded, me: 'ana', reference: reference),
        isEmpty,
      );
      expect(failedSwapWithdrawals(b.folded, me: 'ben', reference: reference), [
        b.entry,
      ]);
    });

    test('a record that is not a swap is left', () {
      final b = bill(paid: true, method: 'shieldedZec');
      expect(
        failedSwapWithdrawals(b.folded, me: 'ana', reference: reference),
        isEmpty,
      );
    });
  });

  group('declaredPayoutIndex', () {
    const a = protocol.Payout(type: 'zec', address: 'u1a');
    const b = protocol.Payout(
      type: 'swap',
      address: '0xb',
      asset: 'USDC',
      chain: 'base',
    );
    test('finds a payout by all four fields', () {
      expect(declaredPayoutIndex([a, b], b), 1);
      expect(declaredPayoutIndex([a, b], a), 0);
      expect(
        declaredPayoutIndex([
          a,
          b,
        ], const protocol.Payout(type: 'swap', address: '0xb', asset: 'USDC')),
        isNull,
      );
      expect(declaredPayoutIndex(const [], a), isNull);
    });
  });

  test('base units read as whole tokens', () {
    expect(formatBaseUnits('39990000', 6), '39.99');
    expect(formatBaseUnits('1000000', 6), '1');
    expect(formatBaseUnits('5', 6), '0.000005');
    expect(formatBaseUnits('0', 6), '0');
    expect(formatBaseUnits('007', 0), '7');
    for (final bad in ['', '1.5', '-1', '1e6', ' 1']) {
      expect(formatBaseUnits(bad, 6), isNull, reason: bad);
    }
    expect(formatBaseUnits('1', -1), isNull);
    // A provider's decimals are a uint8; past that the rendering would be
    // sized by whatever it answered.
    final tiny = formatBaseUnits('1', maxTokenDecimals)!;
    expect(tiny.length, 2 + 255);
    expect(tiny, allOf(startsWith('0.000'), endsWith('1')));
    expect(formatBaseUnits('1', 256), isNull);
    expect(formatBaseUnits('1', 0x7fffffff), isNull);
  });

  test(
    'a swap record names its asset and chain, and the floor when quoted',
    () {
      expect(swapRecordNote('USDC', 'base'), 'USDC on base');
      expect(
        swapRecordNote('USDC', 'base', guaranteed: '39.5'),
        'at least 39.5 USDC on base',
      );
    },
  );

  test('a deposit is one request and a note carrying the swap', () {
    const eur = protocol.ExchangeRate(
      currency: 'EUR',
      minorUnitsPerZec: 51234,
      at: '2026-10-28T19:30:00.000Z',
    );
    final deposit = swapDeposit(
      billId: 'bill-1',
      quote: quote(amountInZatoshi: 7807316),
      to: 'ben',
      amountMinorUnits: 4000,
      rate: eur,
      at: '2026-10-28T19:31:00.000Z',
    );
    expect(
      deposit.uri,
      'zcash:t1deposit000000000000000000000000?amount=0.07807316'
      '&label=swap%20to%20USDC',
    );
    final note = deposit.note;
    expect(note.uri, deposit.uri);
    expect(note.carried, {'ben': 4000});
    expect(note.zatoshi, 7807316);
    expect(note.rate, eur);
    expect(note.swap?.reference, reference);
    expect([note.swap?.assetSymbol, note.swap?.assetChain], ['USDC', 'base']);

    // A deposit that needs a memo, and a debt of nothing, are refused.
    expect(
      () => swapDeposit(
        billId: 'bill-1',
        quote: quote(memo: '123'),
        to: 'ben',
        amountMinorUnits: 4000,
        rate: eur,
        at: 't',
      ),
      throwsA(isA<SwapException>()),
    );
    expect(
      () => swapDeposit(
        billId: 'bill-1',
        quote: quote(),
        to: 'ben',
        amountMinorUnits: 0,
        rate: eur,
        at: 't',
      ),
      throwsArgumentError,
    );
    // An empty memo is no memo.
    expect(
      swapDeposit(
        billId: 'bill-1',
        quote: quote(memo: ''),
        to: 'ben',
        amountMinorUnits: 4000,
        rate: eur,
        at: 't',
      ).uri,
      startsWith('zcash:t1deposit'),
    );
  });
}
