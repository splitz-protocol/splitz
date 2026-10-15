/// The order a wallet's send keeps, and how a swap provider's answer is read.
library;

import 'dart:convert';

import 'package:splitz_core/host.dart' as host;
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

final _request = splitz.renderUri(const [
  splitz.Zip321Payment(address: 'u1ana', zatoshi: 7004),
  splitz.Zip321Payment(address: 'u1ben', zatoshi: 9246),
]);

const _both = [
  host.ProposedOutput('u1ben', 9246),
  host.ProposedOutput('u1ana', 7004),
];

void main() {
  group('what the wallet read is held against the request (§14.6)', () {
    test('the same payments, in any order, pass', () {
      expect(proposalProblem(_request, _both), isNull);
    });

    test('a reader that kept only the first payment is refused', () {
      expect(
        proposalProblem(_request, const [host.ProposedOutput('u1ana', 7004)]),
        contains('Nothing was sent'),
      );
    });

    test('a request this protocol did not write is refused in words', () {
      expect(
        proposalProblem('zcash:u1ana?amount=1&foo=bar', const [
          host.ProposedOutput('u1ana', 100000000),
        ]),
        splitz.describeCode('zip321_not_canonical'),
      );
    });
  });

  group('a send', () {
    Future<WalletSendOutcome> send({
      List<host.ProposedOutput> read = _both,
      Future<int> Function()? propose,
      Future<WalletSendOutcome> Function(int)? broadcast,
    }) => sendPaymentRequest<int>(
      uri: _request,
      read: () async => read,
      propose: propose ?? () async => 1,
      broadcast:
          broadcast ??
          (_) async => const WalletSendOutcome(
            phase: WalletSendPhase.succeeded,
            txid: 'cd34',
          ),
    );

    test(
      'a request read differently spends nothing: no proposal is built',
      () async {
        var proposed = 0;
        final outcome = await send(
          read: const [host.ProposedOutput('u1ana', 7004)],
          propose: () async => ++proposed,
        );
        expect(outcome.phase, WalletSendPhase.failed);
        expect(outcome.error, contains('Nothing was sent'));
        expect(proposed, 0);
      },
    );

    test(
      'a reader that raised is a failure, not a send that may land',
      () async {
        final outcome = await sendPaymentRequest<int>(
          uri: _request,
          read: () async => throw StateError('unreadable'),
          propose: () async => fail('never proposed'),
          broadcast: (_) async => fail('never broadcast'),
        );
        expect(outcome.phase, WalletSendPhase.failed);
        expect(outcome.error, contains('unreadable'));
      },
    );

    test('refused before any transaction is built is a failure, not a send '
        'that may still land', () async {
      var broadcasts = 0;
      final outcome = await send(
        propose: () async => throw StateError('insufficient funds'),
        broadcast: (_) async {
          broadcasts++;
          return const WalletSendOutcome(phase: WalletSendPhase.succeeded);
        },
      );
      expect(outcome.phase, WalletSendPhase.failed);
      expect(outcome.error, contains('insufficient funds'));
      expect(broadcasts, 0);
    });

    test("the broadcast's own outcome is returned as it is", () async {
      final pending = await send(
        broadcast: (_) async => const WalletSendOutcome(
          phase: WalletSendPhase.pendingBroadcast,
          txid: 'ab12',
          statusMessage: 'stored, not broadcast',
        ),
      );
      expect(pending.phase, WalletSendPhase.pendingBroadcast);
      expect(pending.txid, 'ab12');
      expect(pending.statusMessage, 'stored, not broadcast');
      final sent = await send();
      expect((sent.phase, sent.txid), (WalletSendPhase.succeeded, 'cd34'));
    });
  });

  group("reading the swap provider's answer", () {
    List<int> bytes(String s) => utf8.encode(s);

    test('a 200 gives its body back', () {
      expect(swapAnswer(200, bytes('{"quote":1}')), '{"quote":1}');
    });

    test('a 400 is refused on the merits, not retried', () {
      expect(
        () => swapAnswer(400, bytes('{"error":"no route"}')),
        throwsA(isA<SwapException>().having((e) => e.isTransient, 't', false)),
      );
    });

    test('a refusal carries the reason the provider gave', () {
      expect(
        () => swapAnswer(
          400,
          bytes(
            '{"message":"slippageTolerance should not be empty",'
            '"statusCode":400}',
          ),
        ),
        throwsA(
          isA<SwapException>().having(
            (e) => e.message,
            'message',
            'The swap provider answered 400: '
                'slippageTolerance should not be empty',
          ),
        ),
      );
      expect(
        () => swapAnswer(400, bytes('no')),
        throwsA(
          isA<SwapException>().having(
            (e) => e.message,
            'message',
            'The swap provider answered 400',
          ),
        ),
      );
    });

    test('a 500 is transient, so a retry is allowed', () {
      expect(
        () => swapAnswer(503, bytes('upstream down')),
        throwsA(isA<SwapException>().having((e) => e.isTransient, 't', true)),
      );
    });

    test('399 and 400 are the boundary', () {
      expect(swapAnswer(399, bytes('ok')), 'ok');
      expect(() => swapAnswer(400, bytes('no')), throwsA(isA<SwapException>()));
    });

    test('a body that is not valid UTF-8 does not throw a decoder error', () {
      expect(swapAnswer(200, const [0xff, 0xfe, 0x41]), contains('A'));
    });
  });

  group('the fee shown is a ceiling (§14.2)', () {
    test('a raised fee is refused in words; an equal or lower one is not', () {
      expect(
        feeRaisedProblem(reviewedFee: 10000, fee: 15000),
        'The fee went up from 0.0001 ZEC to 0.00015 ZEC since you reviewed '
        'this payment. Nothing was sent; review it again.',
      );
      expect(feeRaisedProblem(reviewedFee: 10000, fee: 10001), isNotNull);
      expect(feeRaisedProblem(reviewedFee: 10000, fee: 10000), isNull);
      expect(feeRaisedProblem(reviewedFee: 10000, fee: 5000), isNull);
    });

    test('a figure no amount renders is said in zatoshi', () {
      expect(
        feeRaisedProblem(reviewedFee: 0, fee: 1),
        startsWith('The fee went up from 0 zatoshi to 0.00000001 ZEC'),
      );
      expect(
        feeRaisedProblem(reviewedFee: -5, fee: splitz.maxZatoshi + 1),
        contains('-5 zatoshi to 2100000000000001 zatoshi'),
      );
    });

    Future<(WalletSendOutcome, List<String>)> send({
      required int fee,
      List<host.ProposedOutput> read = _both,
      bool discardThrows = false,
    }) async {
      final calls = <String>[];
      final outcome = await sendReviewedPaymentRequest<int>(
        uri: _request,
        reviewedFee: 10000,
        read: () async => read,
        propose: () async {
          calls.add('propose');
          return (7, fee);
        },
        discard: (p) async {
          calls.add('discard $p');
          if (discardThrows) throw StateError('gone');
        },
        broadcast: (p) async {
          calls.add('broadcast $p');
          return const WalletSendOutcome(
            phase: WalletSendPhase.succeeded,
            txid: 'cd34',
          );
        },
      );
      return (outcome, calls);
    }

    test('a raised fee discards the proposal and broadcasts nothing', () async {
      final (outcome, calls) = await send(fee: 10001);
      expect(outcome.phase, WalletSendPhase.failed);
      expect(outcome.error, contains('Nothing was sent'));
      expect(calls, ['propose', 'discard 7']);
    });

    test('a discard that raises leaves the refusal standing', () async {
      final (outcome, calls) = await send(fee: 20000, discardThrows: true);
      expect(outcome.phase, WalletSendPhase.failed);
      expect(outcome.error, contains('The fee went up'));
      expect(calls, ['propose', 'discard 7']);
    });

    test('the reviewed fee, or less, is broadcast', () async {
      for (final fee in [10000, 9999, 0]) {
        final (outcome, calls) = await send(fee: fee);
        expect(outcome.phase, WalletSendPhase.succeeded, reason: '$fee');
        expect(calls, ['propose', 'broadcast 7']);
      }
    });

    test('a request read differently builds nothing to discard', () async {
      final (outcome, calls) = await send(
        fee: 10000,
        read: const [host.ProposedOutput('u1ana', 7004)],
      );
      expect(outcome.phase, WalletSendPhase.failed);
      expect(calls, isEmpty);
    });
  });
}
