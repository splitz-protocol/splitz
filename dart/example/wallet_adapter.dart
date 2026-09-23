// A BillHost built on the API a Zcash wallet already has.
//
// The wallet types are declared here rather than imported: this file exists to
// show the seam is implementable and to fail to compile when it stops being,
// and a library cannot depend on the app that embeds it. Each declaration
// mirrors the shape a Zcash wallet's send API already has.
//
// ignore_for_file: avoid_print

import 'dart:typed_data';

import 'package:splitz_core/host.dart';

// --- what the wallet provides -----------------------------------------------

/// The result of proposing a transfer: an id to broadcast, the fee it will
/// cost, and whether the prover's parameters still have to be fetched.
class ProposalResult {
  ProposalResult({
    required this.proposalId,
    required this.feeZatoshi,
    required this.needsSaplingParams,
  });
  final String proposalId;
  final BigInt feeZatoshi;
  final bool needsSaplingParams;
}

/// How a broadcast ended, with the built-but-not-sent state kept separate.
enum SendBroadcastPhase { succeeded, pendingBroadcast, failed, aborted }

/// What a broadcast attempt reported.
class SendBroadcastOutcome {
  SendBroadcastOutcome({
    required this.phase,
    required this.proposalConsumed,
    this.txid,
    this.statusMessage,
    this.error,
  });
  final SendBroadcastPhase phase;
  final bool proposalConsumed;
  final String? txid;
  final String? statusMessage;
  final String? error;
}

/// Takes the ZIP 321 URI whole, which is why a payer who owes four people
/// signs once.
typedef ProposeSendMulti = Future<ProposalResult> Function({
  required String dbPath,
  required String network,
  required String accountUuid,
  required String sendFlowId,
  required String paymentUri,
});

/// Broadcasts a proposal the wallet has already built.
typedef RunSendBroadcast = Future<SendBroadcastOutcome> Function({
  required String proposalId,
  required String sendFlowId,
  required String accountUuid,
});

// --- the adapter ------------------------------------------------------------

/// Drives a wallet's propose-then-broadcast flow from one payment request.
class WalletBillHost extends BillHost {
  WalletBillHost({
    required this.me,
    required this.payToAddress,
    required this.now,
    required this.randomBytes,
    required ProposeSendMulti propose,
    required RunSendBroadcast send,
    required String dbPath,
    required String network,
    required String accountUuid,
    this.sign,
    this.verify,
  })  : _propose = propose,
        _send = send,
        _dbPath = dbPath,
        _network = network,
        _accountUuid = accountUuid;

  @override
  final String me;
  @override
  final String? payToAddress;
  @override
  final Clock now;
  @override
  final Randomness randomBytes;

  /// The wallet holds one Ed25519 seed per account as its splits signing
  /// identity, so both are supplied together or neither is. Absent them every
  /// participant is unauthenticated, and a folded bill reports that rather
  /// than claiming a binding it cannot make.
  @override
  final SignEntry? sign;
  @override
  final VerifyEntry? verify;

  final ProposeSendMulti _propose;
  final RunSendBroadcast _send;
  final String _dbPath;
  final String _network;
  final String _accountUuid;

  @override
  Broadcast get broadcast => (uri) async {
        // One flow id per attempt. It is what lets the wallet recognise a
        // retry of this send rather than a new one.
        final flowId =
            'splitz-${DateTime.now().microsecondsSinceEpoch}-${me.hashCode}';

        final proposal = await _propose(
          dbPath: _dbPath,
          network: _network,
          accountUuid: _accountUuid,
          sendFlowId: flowId,
          paymentUri: uri,
        );

        final outcome = await _send(
          proposalId: proposal.proposalId,
          sendFlowId: flowId,
          accountUuid: _accountUuid,
        );

        switch (outcome.phase) {
          case SendBroadcastPhase.succeeded:
            final txid = outcome.txid;
            // Pending, not failed: money left, and a retry could pay twice.
            if (txid == null) {
              return const Sent.pending(
                detail: 'the wallet reported a send with no transaction id',
              );
            }
            return Sent.sent(txid);

          // Built, signed, not handed to the network. It may still land, so
          // it is neither recorded nor retried.
          case SendBroadcastPhase.pendingBroadcast:
            return Sent.pending(
              detail: outcome.statusMessage ??
                  'The transaction was created but not broadcast yet. '
                      'Check its status before trying again.',
            );

          // Aborted before the proposal was consumed: nothing was spent.
          case SendBroadcastPhase.failed:
          case SendBroadcastPhase.aborted:
            return Sent.failed(
              detail: outcome.error ?? 'The transaction could not be sent.',
            );
        }
      };
}

Future<void> main() async {
  final host = WalletBillHost(
    me: 'ana',
    payToAddress: 'u1ana',
    now: DateTime.now,
    randomBytes: (n) => Uint8List(n),
    dbPath: '/tmp/wallet.sqlite',
    network: 'regtest',
    accountUuid: 'acct-1',
    propose: ({
      required dbPath,
      required network,
      required accountUuid,
      required sendFlowId,
      required paymentUri,
    }) async =>
        ProposalResult(
      proposalId: 'p1',
      feeZatoshi: BigInt.from(10000),
      needsSaplingParams: false,
    ),
    send: ({
      required proposalId,
      required sendFlowId,
      required accountUuid,
    }) async =>
        SendBroadcastOutcome(
      phase: SendBroadcastPhase.pendingBroadcast,
      proposalConsumed: false,
      statusMessage: 'created, not broadcast',
    ),
  );

  final log = BillLog(host)
    ..add([
      createBill(
        host: host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: 'k' * 43,
      ),
    ]);

  final owed = obligationFor(host, log.fold());
  print('signing as ${host.me}, entries: ${log.entries.length}');
  print(
      'obligation: ${owed == null ? 'no rate yet' : owed.settlements.length}');

  final sent = await host.broadcast('zcash:u1ben?amount=0.045');
  print('send result: ${sent.result} — ${sent.detail}');
}
