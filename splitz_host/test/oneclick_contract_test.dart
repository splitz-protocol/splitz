/// The swap client held to the provider's published schema and to its real
/// answers.
///
/// Every other swap test talks to a fake written here, which agrees with the
/// client by construction. These do not: the schema and the fixtures are the
/// provider's (see test/support/oneclick_contract.dart).
@TestOn('vm')
library;

import 'dart:convert';

import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/oneclick_contract.dart';

const usdcOnBase = TradableAsset(
  assetId: 'nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near',
  symbol: 'USDC',
  chain: 'base',
  decimals: 6,
);

/// The client, answering from captured fixtures and keeping what it sent.
({OneClickSwaps swaps, List<Map<String, dynamic>> sent}) live({
  String? referral = 'a-wallet',
  String status = 'status.json',
}) {
  final sent = <Map<String, dynamic>>[];
  return (
    sent: sent,
    swaps: OneClickSwaps(
      origin: Uri.parse('https://1click.chaindefuser.com'),
      zecAssetId: 'nep141:zec.omft.near',
      referral: referral,
      deadline: () => '2026-10-28T19:40:00.000Z',
      post: (url, body) async {
        sent.add(jsonDecode(body) as Map<String, dynamic>);
        return fixtureText('quote.json');
      },
      get: (url) async =>
          fixtureText(url.path.endsWith('/tokens') ? 'tokens.json' : status),
    ),
  );
}

Future<SwapQuote> quote(OneClickSwaps swaps) => swaps.quote(
  asset: usdcOnBase,
  amountInZatoshi: 1000000,
  recipient: '0x1111111111111111111111111111111111111111',
  refundTo: 't1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs',
);

void main() {
  group('what the client sends', () {
    test('is a quote request the schema accepts', () async {
      for (final referral in ['a-wallet', null]) {
        final p = live(referral: referral);
        await quote(p.swaps);
        expect(
          quoteRequestProblems(p.sent.single),
          isEmpty,
          reason: 'referral $referral',
        );
      }
    });

    test('the check refuses what the provider refuses', () {
      // The client's body without slippageTolerance: what the provider
      // answered with fixtures/quote_refused.json.
      final body = <String, dynamic>{
        'dry': true,
        'swapType': 'EXACT_INPUT',
        'originAsset': 'nep141:zec.omft.near',
        'depositType': 'ORIGIN_CHAIN',
        'destinationAsset': usdcOnBase.assetId,
        'amount': '1000000',
        'refundTo': 't1',
        'refundType': 'ORIGIN_CHAIN',
        'recipient': '0x1',
        'recipientType': 'DESTINATION_CHAIN',
        'deadline': '2026-10-28T19:40:00.000Z',
      };
      expect(quoteRequestProblems(body), ['slippageTolerance is required']);
      expect(
        (fixture('quote_refused.json') as Map)['message'],
        contains('slippageTolerance should not be empty'),
      );
      expect(
        quoteRequestProblems({...body, 'slippageTolerance': 0.5}),
        contains(contains('integer')),
      );
      expect(quoteRequestProblems({...body, 'slippageTolerance': 1, 'x': 1}), [
        'x is not a property the API declares',
      ]);
      expect(
        quoteRequestProblems({...body, 'slippageTolerance': 1, 'dry': 'no'}),
        ['dry is not a boolean'],
      );
    });
  });

  group('what the client reads', () {
    test('every field it reads is one the schema declares', () {
      for (final path in [
        'TokenResponse.assetId',
        'TokenResponse.symbol',
        'TokenResponse.blockchain',
        'TokenResponse.decimals',
        'QuoteResponse.quote',
        'QuoteResponse.correlationId',
        'Quote.depositAddress',
        'Quote.depositMemo',
        'Quote.amountOut',
        'Quote.minAmountOut',
        'Quote.deadline',
        'GetExecutionStatusResponse.status',
        'GetExecutionStatusResponse.swapDetails.destinationChainTxHashes[].hash',
        'GetExecutionStatusResponse.swapDetails.refundReason',
        'GetExecutionStatusResponse.swapDetails.refundedAmount',
        'BadRequestResponse.message',
      ]) {
        expect(declares(path), isTrue, reason: path);
      }
      expect(declares('GetExecutionStatusResponse.destinationTxHash'), isFalse);
    });

    test('a live quote reads into the quote the provider issued', () async {
      final q = await quote(live().swaps);
      final raw = fixture('quote.json') as Map<String, dynamic>;
      final issued = raw['quote'] as Map<String, dynamic>;
      expect(q.depositAddress, issued['depositAddress']);
      expect(q.amountOut, issued['amountOut']);
      // The floor once slippage is applied: what the payee is guaranteed.
      expect(q.minAmountOut, issued['minAmountOut']);
      expect(BigInt.parse(q.minAmountOut!) < BigInt.parse(q.amountOut), isTrue);
      expect(
        q.deadline,
        protocol.canonicalInstant(issued['deadline'] as String),
      );
      expect(q.reference, raw['correlationId']);
    });

    test(
      'a dry quote issues no deposit address, and is refused as a quote',
      () async {
        final swaps = OneClickSwaps(
          origin: Uri.parse('https://1click.chaindefuser.com'),
          zecAssetId: 'nep141:zec.omft.near',
          deadline: () => '2026-10-28T19:40:00.000Z',
          post: (_, _) async => fixtureText('quote_dry.json'),
          get: (_) async => '[]',
        );
        await expectLater(
          quote(swaps),
          throwsA(
            isA<SwapException>().having(
              (e) => e.message,
              'message',
              contains('depositAddress'),
            ),
          ),
        );
      },
    );

    test('the token list carries ZEC on its own chain', () async {
      final assets = await live().swaps.tradableAssets();
      final zec = assets.where((a) => a.assetId == 'nep141:zec.omft.near');
      expect(zec.single.chain, 'zec');
      expect(zec.single.decimals, 8);
      expect(assets.map((a) => a.assetId), contains(usdcOnBase.assetId));
    });

    test('a live status of an issued address reads', () async {
      final p = live();
      final status = await p.swaps.statusOf(await quote(p.swaps));
      expect((fixture('status.json') as Map)['status'], 'PENDING_DEPOSIT');
      expect(status.state, SwapState.awaitingDeposit);
      expect(status.destinationTxHash, isNull);
    });

    test('every status the API lists maps to a stated answer', () async {
      const expected = {
        'KNOWN_DEPOSIT_TX': SwapState.awaitingDeposit,
        'PENDING_DEPOSIT': SwapState.awaitingDeposit,
        'INCOMPLETE_DEPOSIT': SwapState.awaitingDeposit,
        'PROCESSING': SwapState.processing,
        'SUCCESS': SwapState.delivered,
        'REFUNDED': SwapState.failed,
        'FAILED': SwapState.failed,
      };
      final listed =
          ((schema('GetExecutionStatusResponse')['properties'] as Map)['status']
                  as Map)['enum']
              as List;
      expect(
        listed.toSet(),
        expected.keys.toSet(),
        reason: 'the API added or removed a status',
      );
      for (final MapEntry(:key, :value) in expected.entries) {
        final raw = Map<String, dynamic>.of(
          fixture('status.json') as Map<String, dynamic>,
        )..['status'] = key;
        final swaps = OneClickSwaps(
          origin: Uri.parse('https://1click.chaindefuser.com'),
          zecAssetId: 'nep141:zec.omft.near',
          deadline: () => '2026-10-28T19:40:00.000Z',
          post: (_, _) async => '{}',
          get: (_) async => jsonEncode(raw),
        );
        final q = await quote(live().swaps);
        expect((await swaps.statusOf(q)).state, value, reason: key);
      }
    });
  });
}
