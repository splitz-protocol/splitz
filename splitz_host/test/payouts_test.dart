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

  test(
    'a first payout this wallet cannot pay is passed over for the next it can',
    () {
      expect(payoutFallback([null, 'x']), isNull);
      final next = payoutFallback(['not on base', null])!;
      expect([next.index, next.passedOver], [1, 'not on base']);
      expect(payoutFallback(['a', 'b', null])!.index, 2);
      expect(payoutFallback(['a', 'b']), isNull);
      expect(payoutFallback([]), isNull);
      expect(payoutFallback(['a']), isNull);
    },
  );

  group('a transparent Zcash payout is a public fallback (§9.1)', () {
    const ua = Payout(
      type: 'zec',
      address:
          'u1l8xunezsvhq8fgzfl7404m450nwnd76zshscn6nfys7vyz2ywyh4cc5daaq0c7q2su5lqfh23sp7fkf3kt27ve5948mzpfdvckzaect2jtte308mkwlycj2u0eac077wu70vqcetkxf',
    );
    const zs = Payout(
      type: 'zec',
      address:
          'zs1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpq6d8g',
    );
    const t1 = Payout(
      type: 'zec',
      address: 't1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs',
    );
    const t3 = Payout(
      type: 'zec',
      address: 't3JZcvsuaXE6ygokL4XUiZSTrQBUoPYFnXJ',
    );
    const tex = Payout(
      type: 'zec',
      address: 'tex1s2rt77ggv6q989lr49rkgzmh5slsksa9khdgte',
    );
    String k(Payout p) => String.fromCharCodes(p.address!.runes.take(4));
    List<String> ranked(List<Payout> declared, Payout first, {String? payTo}) =>
        [
          for (final p in rankedPayouts(
            _who(payTo: payTo, payouts: declared),
            first,
          ))
            p.type == 'zec' ? k(p) : p.type,
        ];

    test('it goes after the shielded one, never ahead of it', () {
      expect(ranked([ua], t1), ['u1l8', 't1Hs']);
      expect(ranked([_usdcEth, ua, _cash], t1), [
        'swap',
        'u1l8',
        't1Hs',
        'cash',
      ]);
      expect(ranked(const [], t1, payTo: ua.address), ['u1l8', 't1Hs']);
    });

    test(
      'p2sh and tex are transparent too, and replace a transparent one in place',
      () {
        expect(ranked([ua, t1], t3), ['u1l8', 't3JZ']);
        expect(ranked([_cash, ua, t1], tex), ['cash', 'u1l8', 'tex1']);
      },
    );

    test('with no shielded payout it goes first', () {
      expect(ranked([_cash], t1), ['t1Hs', 'cash']);
      expect(ranked(const [], t1), ['t1Hs']);
    });

    test('a shielded payout replaces only a shielded one and goes first', () {
      expect(ranked([t1], ua), ['u1l8', 't1Hs']);
      expect(ranked([ua, t1, _cash], zs), ['zs1q', 't1Hs', 'cash']);
    });

    test('an address §8.6 does not decode is not taken for transparent', () {
      expect(ranked([_zecA], t1), ['zA', 't1Hs']);
      expect(ranked([t1, _zecA], ua), ['u1l8', 't1Hs']);
    });
  });

  group('an asset or a chain is compared by ASCII case alone', () {
    Payout swap(String asset, String chain) =>
        Payout(type: 'swap', asset: asset, chain: chain, address: '0x$asset');
    test('a payout replaces one whose asset differs only in ASCII case', () {
      expect(
        _keys(
          rankedPayouts(
            _who(payouts: [swap('usdc', 'eth')]),
            swap('USDC', 'sol'),
          ),
        ),
        ['swap:USDC:0xUSDC'],
      );
    });
    test('no other fold: ß is not SS, ﬁ is not FI', () {
      for (final (held, next) in [('ß', 'SS'), ('ﬁ', 'FI'), ('é', 'É')]) {
        expect(
          _keys(
            rankedPayouts(
              _who(payouts: [swap(held, 'eth')]),
              swap(next, 'eth'),
            ),
          ),
          ['swap:$next:0x$next', 'swap:$held:0x$held'],
          reason: held,
        );
      }
    });
    test('a quoted asset answers its payout by ASCII case alone', () {
      const asset = TradableAsset(
        assetId: 'a',
        symbol: 'USDC',
        chain: 'base',
        decimals: 6,
      );
      expect(asset.answers('usdc', 'BASE'), isTrue);
      const odd = TradableAsset(
        assetId: 'b',
        symbol: 'ÜSD',
        chain: 'base',
        decimals: 6,
      );
      expect(odd.answers('üsd', 'base'), isFalse);
      expect(odd.answers('ÜSD', 'base'), isTrue);
    });
  });
}
