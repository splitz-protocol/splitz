/// Builds the entries a bill's log is made of.
///
/// The protocol derives an entry's id and refuses one that does not match
/// (§9.4, §9.5) but exports nothing that assembles an entry, because the
/// shape of an entry is the specification's business and building one is the
/// wallet's. That leaves every wallet to hand-assemble a map and derive a
/// digest, which is a thing to get wrong once per wallet.
///
/// Every function here returns an entry whose `id` is already the digest the
/// protocol will check. Nothing here writes to a log; that is [BillLog]'s job.
library;

import 'dart:convert';

import 'package:splitz/splitz.dart' as splitz;

import 'host.dart';

/// The version every entry this layer writes carries.
const int entryVersion = 1;

/// A createBill's `creatorKey` is 32 bytes and its `nonce` is 16, both
/// unpadded base64url — §9.4 refuses an entry whose members are any other
/// length, and the bill id is the digest of the entry that states them.
const int creatorKeyBytes = 32;
const int nonceBytes = 16;

/// Opens a bill. Its id is the digest of this entry (§9.4).
///
/// `creatorKey` is the key §10.7 binds the creator by, so the creator needs no
/// prior acquaintance. The nonce makes two bills opened in one second by one
/// person two bills.
Map<String, dynamic> createBill({
  required BillHost host,
  required String name,
  required String currency,
  String splitMode = 'equal',
  required String creatorKey,
}) {
  final entry = <String, dynamic>{
    'v': entryVersion,
    'author': host.me,
    'kind': 'createBill',
    'at': _at(host),
    'name': name,
    'currency': currency,
    'splitMode': splitMode,
    'creatorKey': creatorKey,
    'nonce': base64UrlNoPad(host.randomBytes(nonceBytes)),
  };
  entry['id'] = splitz.deriveBillId(entry);
  return entry;
}

/// Joins a bill, or restates this device's own participant record.
///
/// `identityKey` is a self-claim: §10.7 binds it only when the entry's author
/// is the participant it names and the signature verifies against it. Naming
/// somebody else proves nothing about them.
Map<String, dynamic> joinBill({
  required BillHost host,
  String? name,
  String? payTo,
  String? identityKey,
}) {
  final participant = <String, dynamic>{'id': host.me};
  if (name != null) participant['name'] = name;
  if (payTo != null) participant['payTo'] = payTo;
  if (identityKey != null) participant['identityKey'] = identityKey;
  return _sealed(host, <String, dynamic>{
    'kind': 'joinBill',
    'participant': participant,
  });
}

/// Adds an expense. `amount` is minor units of the bill's currency (§2.1).
///
/// `split` is §4's own shape and is passed through untouched: nothing here
/// invents a split method the specification does not define.
Map<String, dynamic> addExpense({
  required BillHost host,
  required String expenseId,
  required String paidBy,
  required int amount,
  required Map<String, dynamic> split,
  String? description,
}) {
  final expense = <String, dynamic>{
    'id': expenseId,
    'paidBy': paidBy,
    'amount': amount,
    'at': _at(host),
    'split': split,
  };
  if (description != null) expense['description'] = description;
  return _sealed(host, <String, dynamic>{
    'kind': 'addExpense',
    'expense': expense,
  });
}

/// Records a payment that was made. It moves no balance until a confirmation
/// settles it (§10.5) — a record is a claim, not a settlement.
///
/// `paymentId` is the transaction id, so the record and the transaction carry
/// one identifier and a reader can check the second from the first.
Map<String, dynamic> recordPayment({
  required BillHost host,
  required String paymentId,
  required String to,
  required int amount,
  String method = 'shieldedZec',
}) {
  return _sealed(host, <String, dynamic>{
    'kind': 'recordPayment',
    'payment': <String, dynamic>{
      'id': paymentId,
      'from': host.me,
      'to': to,
      'amount': amount,
      'method': method,
      'at': _at(host),
    },
  });
}

/// Confirms a payment. Which methods settle a debt, and who may claim each, is
/// §10.5's decision and nothing here widens it.
Map<String, dynamic> confirmPayment({
  required BillHost host,
  required String paymentId,
  required String method,
  String? reference,
}) {
  final confirmation = <String, dynamic>{
    'paymentId': paymentId,
    'method': method,
  };
  if (reference != null) confirmation['reference'] = reference;
  return _sealed(host, <String, dynamic>{
    'kind': 'confirmPayment',
    'confirmation': confirmation,
  });
}

/// Snapshots a price onto the bill. §7 makes only `source` optional.
Map<String, dynamic> setRate({
  required BillHost host,
  required String currency,
  required int minorUnitsPerZec,
  String? source,
}) {
  final rate = <String, dynamic>{
    'currency': currency,
    'minorUnitsPerZec': minorUnitsPerZec,
    'at': _at(host),
  };
  if (source != null) rate['source'] = source;
  return _sealed(host, <String, dynamic>{
    'kind': 'setRate',
    'rate': rate,
  });
}

/// Withdraws an entry. Who may is §10.8's decision.
Map<String, dynamic> voidEntry({
  required BillHost host,
  required String targetId,
}) {
  return _sealed(host, <String, dynamic>{
    'kind': 'voidEntry',
    'targetId': targetId,
  });
}

String _at(BillHost host) =>
    splitz.canonicalInstant(host.now().toUtc().toIso8601String());

/// Fills in the members every entry carries and derives §9.5's id.
///
/// The id is derived last, over the finished entry, because the digest covers
/// every member but `id`, `sig` and `v` — deriving it earlier would digest an
/// entry that is not the one written.
Map<String, dynamic> _sealed(BillHost host, Map<String, dynamic> body) {
  final entry = <String, dynamic>{
    'v': entryVersion,
    'author': host.me,
    'at': _at(host),
    ...body,
  };
  entry['id'] = splitz.deriveEntryId(entry);
  return entry;
}

/// Signs [entry] with the host's signer (§10.6), or returns it unchanged when
/// the host does not sign.
///
/// Signing is a separate step rather than part of each builder because the
/// curve operation is the host's (§13) and may be asynchronous — a hardware
/// signer, a user prompt — while assembling an entry is neither.
///
/// Safe to call after the id has been derived, and it must be: §9.5's digest
/// covers every member but `id`, `sig` and `v`, so attaching a signature does
/// not move the id, while §10.6's message covers `id` and would be a message
/// about a different entry if it were taken first.
///
/// A host with no signer gets its entry back unsigned rather than an error.
/// §10.7 then binds no key to that author, and a folded bill reports no
/// identity binding rather than claiming one it cannot make.
Future<Map<String, dynamic>> signEntry({
  required BillHost host,
  required Map<String, dynamic> entry,
}) async {
  final sign = host.sign;
  if (sign == null) return entry;
  final message = utf8.encode(splitz.signingMessage(entry));
  return <String, dynamic>{...entry, 'sig': await sign(message)};
}

/// Unpadded base64url, the encoding every key, nonce and id in the protocol
/// uses (§9.4).
///
/// Written here because the protocol keeps its own encoder private: it checks
/// lengths and alphabets on the way in and never asks a caller to produce one.
/// A wallet has to, for the creator key and the nonce, so it is one line here
/// rather than one line in every wallet.
String base64UrlNoPad(List<int> bytes) =>
    base64UrlEncode(bytes).replaceAll('=', '');
