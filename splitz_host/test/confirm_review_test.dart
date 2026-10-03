/// What a payee is warned about before confirming a payment (§14.7).
library;

import 'package:splitz_core/host.dart' as host;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

protocol.ExchangeRate _eur(int per) => protocol.ExchangeRate(
  currency: 'EUR',
  minorUnitsPerZec: per,
  at: '2026-10-28T19:30:00.000Z',
);

host.FoldedBill _bill({required String? rateBy, int rate = 100000}) =>
    host.FoldedBill(
      bill: protocol.Bill(
        id: 'b',
        name: 'Dinner',
        currency: 'EUR',
        rate: _eur(rate),
      ),
      creatorId: 'ana',
      setAside: const [],
      withdrawn: const [],
      replacedAddresses: const [],
      identities: const protocol.Identities({}),
      rateAuthor: rateBy,
    );

protocol.PaymentRecord _paid({protocol.ExchangeRate? at}) =>
    protocol.PaymentRecord(
      id: 'ben:p1',
      from: 'ben',
      to: 'ana',
      amount: 1000,
      currency: 'EUR',
      method: 'shieldedZec',
      at: '2026-10-28T19:40:00.000Z',
      zatoshi: 1000000,
      paidAtRate: at,
    );

void main() {
  test('an honest payment raises nothing, with or without a live price', () {
    final bill = _bill(rateBy: 'ana');
    expect(concernsBeforeConfirming(_paid(at: _eur(100000)), bill), isEmpty);
    expect(
      concernsBeforeConfirming(_paid(at: _eur(100000)), bill, live: 104000),
      isEmpty,
    );
  });

  test('the payer set the rate', () {
    expect(concernsBeforeConfirming(_paid(), _bill(rateBy: 'ben')), [
      PaymentConcern.rateSetByPayer,
    ]);
  });

  test('priced at a rate other than the bill\'s', () {
    expect(
      concernsBeforeConfirming(_paid(at: _eur(90000)), _bill(rateBy: 'ana')),
      [PaymentConcern.pricedAtAnotherRate],
    );
  });

  test('five percent or more from the live price, either way', () {
    final bill = _bill(rateBy: 'ana');
    expect(concernsBeforeConfirming(_paid(), bill, live: 105264), [
      PaymentConcern.rateFarFromLive,
    ]);
    expect(concernsBeforeConfirming(_paid(), bill, live: 95238), [
      PaymentConcern.rateFarFromLive,
    ]);
    expect(concernsBeforeConfirming(_paid(), bill, live: 104000), isEmpty);
  });

  test('every concern at once, in order', () {
    expect(
      concernsBeforeConfirming(
        _paid(at: _eur(130000)),
        _bill(rateBy: 'ben'),
        live: 100000,
      ),
      PaymentConcern.values,
    );
  });

  test('the creator prices a bill only they have not priced', () {
    expect(creatorRateMissing(_bill(rateBy: 'ben'), 'ana'), isTrue);
    expect(creatorRateMissing(_bill(rateBy: null), 'ana'), isTrue);
    expect(creatorRateMissing(_bill(rateBy: 'ana'), 'ana'), isFalse);
    expect(creatorRateMissing(_bill(rateBy: 'ben'), 'ben'), isFalse);
  });
}
