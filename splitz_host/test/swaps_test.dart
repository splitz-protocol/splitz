/// Settling a debt in an asset that is not ZEC.
///
/// No network: the transport is a function, so what is asserted is the
/// request this builds, the answer it reads, and what it refuses.
library;

import 'dart:convert';

import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/oneclick_contract.dart';

const String origin = 'https://swaps.example';

/// The provider's token list, as two chains carrying one symbol.
const List<Map<String, Object?>> tokens = [
  {
    'assetId': 'nep141:zec',
    'symbol': 'ZEC',
    'blockchain': 'zec',
    'decimals': 8,
  },
  {
    'assetId': 'nep141:base-usdc',
    'symbol': 'USDC',
    'blockchain': 'base',
    'decimals': 6,
  },
  {
    'assetId': 'nep141:eth-usdc',
    'symbol': 'USDC',
    'blockchain': 'eth',
    'decimals': 6,
  },
];

({OneClickSwaps swaps, List<Uri> gets, List<Map<String, Object?>> posts})
provider({
  Map<String, Object?>? quote,
  Map<String, Object?>? status,
  Object? throwOnCall,
}) {
  final gets = <Uri>[];
  final posts = <Map<String, Object?>>[];
  return (
    gets: gets,
    posts: posts,
    swaps: OneClickSwaps(
      origin: Uri.parse(origin),
      zecAssetId: 'nep141:zec',
      referral: 'a-wallet',
      // Ten minutes after the clock this test holds still.
      deadline: () => '2026-10-28T19:40:00.000Z',
      get: (url) async {
        if (throwOnCall != null) throw throwOnCall;
        gets.add(url);
        if (url.path.endsWith('/tokens')) return jsonEncode(tokens);
        return jsonEncode(status ?? {'status': 'PENDING_DEPOSIT'});
      },
      post: (url, body) async {
        if (throwOnCall != null) throw throwOnCall;
        final sent = jsonDecode(body) as Map<String, dynamic>;
        posts.add({'url': url, 'body': sent});
        // Refused as the provider refuses it: a fake that accepts anything
        // agrees with the client by construction.
        final problems = quoteRequestProblems(sent);
        if (problems.isNotEmpty) {
          throw SwapException('answered 400: ${problems.join(', ')}');
        }
        final answer = <String, Object?>{
          ...(quote ??
              {
                'correlationId': 'near-intent-7f3a',
                'quote': {
                  'depositAddress': 'u1provider',
                  'amountOut': '2500000',
                  'deadline': '2026-10-28T19:40:00.000Z',
                },
              }),
        };
        // As the provider answers (tools/contracts/fixtures/quote.json): the
        // request it quoted is echoed, and the quote states the amount in.
        answer.putIfAbsent('quoteRequest', () => sent);
        final q = answer['quote'];
        if (q is Map) {
          answer['quote'] = {'amountIn': sent['amount'], ...q};
        }
        return jsonEncode(answer);
      },
    ),
  );
}

TradableAsset usdcOnBase() => const TradableAsset(
  assetId: 'nep141:base-usdc',
  symbol: 'USDC',
  chain: 'base',
  decimals: 6,
);

void main() {
  group('matching what the recipient asked for', () {
    test('an asset is matched on BOTH the symbol and the chain', () async {
      final p = provider();
      final assets = await p.swaps.tradableAssets();
      final base = assets.firstWhere((a) => a.answers('USDC', 'base'));

      expect(base.assetId, 'nep141:base-usdc');
      // The same symbol on another chain is a different token, delivered
      // somewhere the recipient cannot reach.
      expect(base.answers('USDC', 'eth'), isFalse);
      expect(
        assets.firstWhere((a) => a.answers('usdc', 'ETH')).assetId,
        'nep141:eth-usdc',
      );
    });

    test('the token list is read once', () async {
      final p = provider();
      await p.swaps.tradableAssets();
      await p.swaps.tradableAssets();
      expect(p.gets.where((u) => u.path.endsWith('/tokens')).length, 1);
    });
  });

  group('the quote', () {
    test('states ZEC in, the asset out, and where a refund goes', () async {
      final p = provider();
      final quote = await p.swaps.quote(
        asset: usdcOnBase(),
        amountInZatoshi: 1000000,
        recipient: '0xcara',
        refundTo: 'u1ana',
      );

      final body = p.posts.single['body'] as Map<String, dynamic>;
      expect(body['originAsset'], 'nep141:zec');
      expect(body['destinationAsset'], 'nep141:base-usdc');
      expect(body['amount'], '1000000');
      expect(body['recipient'], '0xcara');
      expect(body['refundTo'], 'u1ana');
      expect(body['referral'], 'a-wallet');
      // Not a dry run: a quote a payer is shown is one the provider will
      // honour.
      expect(body['dry'], false);
      // Required by the provider: a quote without it is refused with a 400.
      expect(body['slippageTolerance'], OneClickSwaps.slippageBasisPoints);
      expect(body['slippageTolerance'], 100);

      // The deposit address is the PROVIDER's, not the recipient's.
      expect(quote.depositAddress, 'u1provider');
      expect(quote.depositAddress, isNot('0xcara'));
      expect(quote.amountInZatoshi, 1000000);
      expect(quote.amountOut, '2500000');
    });

    test('the reference is the intent id, not a txid', () async {
      final p = provider();
      final quote = await p.swaps.quote(
        asset: usdcOnBase(),
        amountInZatoshi: 1000000,
        recipient: '0xcara',
        refundTo: 'u1ana',
      );
      // §9.2: this is what a payment record carries. A reader rendering it as
      // a Zcash transaction is wrong for every swap.
      expect(quote.paymentReference, 'near-intent-7f3a');
    });

    test(
      'a provider naming no intent is referenced by its deposit address',
      () async {
        final p = provider(
          quote: {
            'quote': {'depositAddress': 'u1provider', 'amountOut': '2500000'},
          },
        );
        final quote = await p.swaps.quote(
          asset: usdcOnBase(),
          amountInZatoshi: 1000000,
          recipient: '0xcara',
          refundTo: 'u1ana',
        );
        expect(quote.paymentReference, 'u1provider');
      },
    );

    test("the provider's own deadline wins over ours", () async {
      // Honouring a longer deadline of ours would quote a price it has
      // stopped holding.
      final p = provider();
      final quote = await p.swaps.quote(
        asset: usdcOnBase(),
        amountInZatoshi: 1000000,
        recipient: '0xcara',
        refundTo: 'u1ana',
      );
      expect(quote.deadline, '2026-10-28T19:40:00.000Z');
      expect(quote.hasExpired('2026-10-28T19:39:00.000Z'), isFalse);
      expect(quote.hasExpired('2026-10-28T19:41:00.000Z'), isTrue);
    });
  });

  group('what it refuses', () {
    test('a swap that sends nothing', () async {
      final p = provider();
      await expectLater(
        p.swaps.quote(
          asset: usdcOnBase(),
          amountInZatoshi: 0,
          recipient: '0xcara',
          refundTo: 'u1ana',
        ),
        throwsA(isA<SwapException>()),
      );
      expect(p.posts, isEmpty, reason: 'refused before anything was sent');
    });

    test('a quote with no refund address', () async {
      // The one failure a payer cannot recover from: a failed swap with
      // nowhere to send the ZEC back to.
      final p = provider();
      await expectLater(
        p.swaps.quote(
          asset: usdcOnBase(),
          amountInZatoshi: 1000000,
          recipient: '0xcara',
          refundTo: '',
        ),
        throwsA(isA<SwapException>()),
      );
      expect(p.posts, isEmpty);
    });

    test('a quote for another recipient', () async {
      // The provider's answer names the request it quoted. One naming another
      // recipient would send this payer's money to them.
      final p = provider(
        quote: {
          'quote': {'depositAddress': 'u1provider', 'amountOut': '2500000'},
          'quoteRequest': {
            'recipient': '0xsomebodyelse',
            'destinationAsset': 'nep141:base-usdc',
            'originAsset': 'nep141:zec',
            'amount': '1000000',
            'refundTo': 'u1ana',
            'swapType': 'EXACT_INPUT',
          },
        },
      );
      await expectLater(
        p.swaps.quote(
          asset: usdcOnBase(),
          amountInZatoshi: 1000000,
          recipient: '0xcara',
          refundTo: 'u1ana',
        ),
        throwsA(
          isA<SwapException>().having(
            (e) => e.message,
            'message',
            contains('recipient'),
          ),
        ),
      );
    });

    test('a quote for another amount in', () async {
      final p = provider(
        quote: {
          'quote': {
            'depositAddress': 'u1provider',
            'amountOut': '2500000',
            'amountIn': '5000000',
          },
        },
      );
      await expectLater(
        p.swaps.quote(
          asset: usdcOnBase(),
          amountInZatoshi: 1000000,
          recipient: '0xcara',
          refundTo: 'u1ana',
        ),
        throwsA(isA<SwapException>()),
      );
    });

    test('a response with no deposit address', () async {
      final p = provider(
        quote: {
          'quote': {'amountOut': '1'},
        },
      );
      await expectLater(
        p.swaps.quote(
          asset: usdcOnBase(),
          amountInZatoshi: 1000000,
          recipient: '0xcara',
          refundTo: 'u1ana',
        ),
        throwsA(isA<SwapException>()),
      );
    });

    test('a transport failure is reported as retryable', () async {
      final p = provider(throwOnCall: const SocketishError());
      await expectLater(
        p.swaps.tradableAssets(),
        throwsA(
          isA<SwapException>().having(
            (e) => e.isTransient,
            'isTransient',
            isTrue,
          ),
        ),
      );
    });
  });

  group('status', () {
    test('the provider vocabulary maps onto three answers', () async {
      for (final (word, expected) in [
        ('PENDING_DEPOSIT', SwapState.awaitingDeposit),
        ('KNOWN_DEPOSIT_TX', SwapState.awaitingDeposit),
        ('SUCCESS', SwapState.delivered),
        ('INCOMPLETE_DEPOSIT', SwapState.awaitingDeposit),
        ('PROCESSING', SwapState.processing),
        ('FAILED', SwapState.failed),
        ('REFUNDED', SwapState.failed),
      ]) {
        final p = provider(status: {'status': word});
        final s = await p.swaps.statusOf(
          SwapQuote(
            depositAddress: 'u1provider',
            amountInZatoshi: 1,
            amountOut: '1',
            asset: usdcOnBase(),
            deadline: '2026-01-01T00:00:00.000Z',
          ),
        );
        expect(s.state, expected, reason: word);
      }
    });

    test('a word nobody here defined is NOT read as delivered', () async {
      // Reading an unknown status as success tells a payer their debt is
      // settled on the strength of a string this code has never seen.
      final p = provider(status: {'status': 'SOME_NEW_WORD'});
      final s = await p.swaps.statusOf(
        SwapQuote(
          depositAddress: 'u1provider',
          amountInZatoshi: 1,
          amountOut: '1',
          asset: usdcOnBase(),
          deadline: '2026-01-01T00:00:00.000Z',
        ),
      );
      expect(s.state, SwapState.processing);
      expect(s.state, isNot(SwapState.delivered));
    });

    test('the hash and the reason come from swapDetails', () async {
      final quote = SwapQuote(
        depositAddress: 'u1provider',
        amountInZatoshi: 1,
        amountOut: '1',
        asset: usdcOnBase(),
        deadline: '2026-01-01T00:00:00.000Z',
      );
      final delivered = await provider(
        status: {
          'status': 'SUCCESS',
          'swapDetails': {
            'destinationChainTxHashes': [
              {'hash': '0xbase-tx', 'explorerUrl': 'https://x'},
            ],
          },
        },
      ).swaps.statusOf(quote);
      expect(delivered.destinationTxHash, '0xbase-tx');

      final refunded = await provider(
        status: {
          'status': 'REFUNDED',
          'swapDetails': {'refundReason': 'deposit below the minimum'},
        },
      ).swaps.statusOf(quote);
      expect(refunded.detail, 'deposit below the minimum');

      // Keys the provider does not send at the top level are not read there.
      final stray = await provider(
        status: {
          'status': 'SUCCESS',
          'destinationTxHash': '0xtop',
          'message': 'top-level',
        },
      ).swaps.statusOf(quote);
      expect(stray.destinationTxHash, isNull);
      expect(stray.detail, isNull);
    });

    test('the memo travels with the address', () async {
      // Some chains lose a deposit sent without its memo.
      final p = provider();
      await p.swaps.statusOf(
        SwapQuote(
          depositAddress: 'u1provider',
          depositMemo: 'memo-1',
          amountInZatoshi: 1,
          amountOut: '1',
          asset: usdcOnBase(),
          deadline: '2026-01-01T00:00:00.000Z',
        ),
      );
      expect(p.gets.last.queryParameters['depositMemo'], 'memo-1');
    });
  });

  group('a build with no provider', () {
    test('says so rather than doing nothing', () async {
      const swaps = UnconfiguredSwaps();
      expect(await swaps.tradableAssets(), isEmpty);
      await expectLater(
        swaps.quote(
          asset: usdcOnBase(),
          amountInZatoshi: 1,
          recipient: 'x',
          refundTo: 'y',
        ),
        throwsA(isA<SwapException>()),
      );
    });
  });
}

/// Stands in for whatever the embedding wallet's transport throws.
class SocketishError implements Exception {
  const SocketishError();
}
