import 'package:splitz_core/host.dart' show Payout;
import 'package:splitz_core/splitz_core.dart' show Participant;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

const _zecA = Payout(type: 'zec', address: 'zA');
const _zecB = Payout(type: 'zec', address: 'zB');
const _usdcEth = Payout(
  type: 'swap',
  asset: 'USDC',
  chain: 'eth',
  address: '0xa',
);
const _usdcSol = Payout(
  type: 'swap',
  asset: 'usdc',
  chain: 'sol',
  address: 'sA',
);
const _usdt = Payout(type: 'swap', asset: 'USDT', chain: 'eth', address: '0xb');
const _cash = Payout(type: 'cash');

Participant _who({String? payTo, List<Payout> payouts = const []}) =>
    Participant(id: 'p', name: 'P', payTo: payTo, payouts: payouts);

List<String> _keys(List<Payout> ps) => [
  for (final p in ps) '${p.type}:${p.asset ?? ''}:${p.address ?? ''}',
];

void main() {
  group('rankedPayouts', () {
    test(
      'a payTo-only record declares one zec payout, which a zec replaces',
      () {
        expect(_keys(rankedPayouts(_who(payTo: 'zOld'), _zecA)), ['zec::zA']);
      },
    );

    test(
      'a payTo-only record keeps its zec behind a payout of another kind',
      () {
        expect(_keys(rankedPayouts(_who(payTo: 'zOld'), _usdcEth)), [
          'swap:USDC:0xa',
          'zec::zOld',
        ]);
      },
    );

    test('a record declaring nothing yields only the new payout', () {
      expect(_keys(rankedPayouts(_who(), _cash)), ['cash::']);
      expect(_keys(rankedPayouts(_who(payTo: ''), _cash)), ['cash::']);
    });

    test('the new payout goes first and replaces one of its own kind', () {
      final who = _who(payouts: [_usdcEth, _zecA, _cash]);
      expect(_keys(rankedPayouts(who, _zecB)), [
        'zec::zB',
        'swap:USDC:0xa',
        'cash::',
      ]);
    });

    test('every other payout keeps its declared order', () {
      final who = _who(payouts: [_cash, _usdt, _zecA, _usdcEth]);
      expect(_keys(rankedPayouts(who, _zecB)), [
        'zec::zB',
        'cash::',
        'swap:USDT:0xb',
        'swap:USDC:0xa',
      ]);
    });

    test('a swap replaces only the same asset, whatever its case or chain', () {
      final who = _who(payouts: [_zecA, _usdcEth, _usdt]);
      expect(_keys(rankedPayouts(who, _usdcSol)), [
        'swap:usdc:sA',
        'zec::zA',
        'swap:USDT:0xb',
      ]);
    });

    test('a swap leaves a zec and cash where they were', () {
      final who = _who(payouts: [_cash, _zecA]);
      expect(_keys(rankedPayouts(who, _usdt)), [
        'swap:USDT:0xb',
        'cash::',
        'zec::zA',
      ]);
    });

    test('payouts take precedence over payTo', () {
      final who = _who(payTo: 'zOld', payouts: [_cash]);
      expect(_keys(rankedPayouts(who, _zecA)), ['zec::zA', 'cash::']);
    });

    test('declared payouts of one kind are all replaced', () {
      final who = _who(payouts: [_zecA, _cash, _zecB]);
      expect(_keys(rankedPayouts(who, _zecA)), ['zec::zA', 'cash::']);
    });

    test('the input is not modified', () {
      final declared = [_zecA, _cash];
      final who = _who(payouts: declared);
      rankedPayouts(who, _zecB);
      expect(_keys(declared), ['zec::zA', 'cash::']);
    });
  });
}
