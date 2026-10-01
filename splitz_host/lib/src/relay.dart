/// The optional transport: a store of ciphertext blobs, grouped per bill.
library;

import 'dart:convert';

import 'package:splitz_core/splitz_core.dart' as splitz;

/// A dumb store of ciphertext blobs, grouped into per-bill channels.
///
/// It moves bytes for the participant who left before dessert and cannot be
/// handed a QR code across the table. It holds no key, so every blob is opaque.
/// What it can observe is deliberately the minimum — that a channel has some
/// blobs, how large they are, and when they last changed — and never who owes
/// whom.
///
/// Optional, and meant to stay so: a bill works with no relay at all. It is
/// created, split and settled locally and shared by QR; only the asynchronous
/// catch-up is missing without one.
///
/// Specified in SPEC.md §15.5.
abstract interface class SplitsRelay {
  /// Adds [blobs] to [channel]. Pushing a blob already present is a no-op, so
  /// a retry after a dropped connection cannot create duplicates.
  Future<void> push(String channel, List<String> blobs);

  /// Every blob currently held for [channel].
  ///
  /// The caller opens and merges by entry id, and merging is idempotent, so
  /// returning blobs it already holds is harmless — the relay is not asked to
  /// track per-caller state it has no identity to key on.
  Future<List<String>> fetch(String channel);
}

/// Raised when the relay could not be reached, refused, or answered with
/// something that is not a channel.
///
/// Reported, never swallowed. A bill works with no relay at all, so a transport
/// that quietly does nothing looks exactly like one that is working — and the
/// difference is whether the person who left early ever sees what they owe.
class SplitsRelayException implements Exception {
  const SplitsRelayException(this.message, {this.isTransient = true});

  final String message;

  /// Whether retrying later could plausibly succeed. False for a relay this
  /// build cannot use at all.
  final bool isTransient;

  @override
  String toString() => 'SplitsRelayException: $message';
}

/// Derives the channel a bill syncs under.
abstract final class SplitsChannel {
  /// The channel for [billId]: its SHA-256, hex encoded.
  ///
  /// A hash rather than the id itself, because the id is also live in every
  /// invite and every QR code. Participants all know the bill id and so all
  /// compute the same channel; somebody who only sees relay traffic cannot run
  /// it backwards to the id, let alone to the contents.
  static String forBill(String billId) => splitz.channelFor(billId);
}

/// The relay for a build that names none.
///
/// Every call fails, and says why. The alternative is a sync indicator that
/// never resolves.
class UnconfiguredSplitsRelay implements SplitsRelay {
  const UnconfiguredSplitsRelay();

  static const String reason =
      'This build has no bill relay, so bills stay on this device. '
      'Share them by QR code instead.';

  @override
  Future<void> push(String channel, List<String> blobs) async =>
      throw const SplitsRelayException(reason, isTransient: false);

  @override
  Future<List<String>> fetch(String channel) async =>
      throw const SplitsRelayException(reason, isTransient: false);
}

/// A relay that keeps blobs in memory.
///
/// For tests, and for wiring two in-process participants together. Not for
/// moving bytes between devices: what is pushed here does not outlive the
/// process and is visible to nobody else.
class InMemorySplitsRelay implements SplitsRelay {
  final Map<String, Set<String>> _channels = {};

  @override
  Future<void> push(String channel, List<String> blobs) async {
    (_channels[channel] ??= <String>{}).addAll(blobs);
  }

  @override
  Future<List<String>> fetch(String channel) async =>
      List.unmodifiable(_channels[channel] ?? const <String>{});
}

/// Posts a JSON body and returns the response body.
typedef JsonPost = Future<String> Function(Uri url, String body);

/// Fetches a URL and returns the response body.
typedef JsonGet = Future<String> Function(Uri url);

/// A relay backed by an HTTP blob store.
///
/// Two routes under [origin]: `POST /c/<channel>` with `{"blobs":[…]}` adds
/// blobs, `GET /c/<channel>` returns them. The server stores opaque bytes keyed
/// by the channel hash — never the bill id, never plaintext.
///
/// The two calls are injected rather than made here, so bill sync takes the
/// same network route as the rest of the wallet. On a build that routes through
/// Tor it goes over Tor and fails closed while Tor is starting or broken,
/// instead of being the one path that quietly leaves in the clear.
class HttpSplitsRelay implements SplitsRelay {
  /// [origin] is a scheme, a host and an optional path. A query or a fragment
  /// is refused: the channel is appended to the path, and an origin carrying
  /// either would address something else entirely.
  HttpSplitsRelay({
    required this.origin,
    required JsonPost post,
    required JsonGet get,
  }) : _post = post,
       _get = get {
    if (origin.hasQuery || origin.hasFragment) {
      throw const SplitsRelayException(
        'A relay origin carries no query and no fragment',
        isTransient: false,
      );
    }
  }

  final Uri origin;
  final JsonPost _post;
  final JsonGet _get;

  /// A blob longer than this is refused rather than sent. Mirrored by the
  /// server: a bound only one side keeps is not a bound.
  static const int maxBlobChars = 64 * 1024;

  /// A push body longer than this, in UTF-8 bytes, is refused by the server,
  /// so a push is split into requests that each fit. Mirrored by the server.
  static const int maxBodyBytes = 32 * 1024 * 1024;

  /// [blobs] as the push bodies that carry them, in order, each at most
  /// [maxBodyBytes]. Every blob is at most [maxBlobChars], so every body
  /// holds at least one.
  static List<String> pushBodies(List<String> blobs) {
    const open = '{"blobs":[', close = ']}';
    const empty = open.length + close.length;
    final bodies = <String>[];
    var batch = <String>[];
    var size = empty;
    for (final blob in blobs) {
      final encoded = jsonEncode(blob);
      final bytes = utf8.encode(encoded).length;
      // One more blob costs its bytes and, after the first, a comma.
      if (batch.isNotEmpty && size + 1 + bytes > maxBodyBytes) {
        bodies.add('$open${batch.join(',')}$close');
        batch = [];
        size = empty;
      }
      size += (batch.isEmpty ? 0 : 1) + bytes;
      batch.add(encoded);
    }
    if (batch.isNotEmpty) bodies.add('$open${batch.join(',')}$close');
    return bodies;
  }

  Uri _channelUrl(String channel) =>
      origin.replace(path: '${origin.path}/c/$channel');

  @override
  Future<void> push(String channel, List<String> blobs) async {
    if (blobs.isEmpty) return;
    for (final blob in blobs) {
      if (blob.length > maxBlobChars) {
        throw SplitsRelayException(
          'A blob of ${blob.length} characters is over the '
          '$maxBlobChars the relay accepts',
          isTransient: false,
        );
      }
    }
    for (final request in pushBodies(blobs)) {
      final String body;
      try {
        body = await _post(_channelUrl(channel), request);
      } on Object catch (e) {
        throw SplitsRelayException('Could not reach the relay: $e');
      }
      final decoded = _decode(body);
      if (decoded['ok'] != true) {
        throw SplitsRelayException('The relay refused the push: $body');
      }
    }
  }

  @override
  Future<List<String>> fetch(String channel) async {
    final String body;
    try {
      body = await _get(_channelUrl(channel));
    } on Object catch (e) {
      throw SplitsRelayException('Could not reach the relay: $e');
    }
    final blobs = _decode(body)['blobs'];
    if (blobs is! List) {
      throw const SplitsRelayException('The relay answered without a channel');
    }
    // Everything here was written by somebody else and is opened under the
    // bill key afterwards. A non-string is dropped rather than cast: a cast
    // that fails throws a type error from inside a sync loop, and this is
    // reached by talking to a server nobody here runs.
    return [
      for (final blob in blobs)
        if (blob is String) blob,
    ];
  }

  Map<String, dynamic> _decode(String body) {
    final Object? decoded;
    try {
      decoded = jsonDecode(body);
    } on FormatException {
      throw const SplitsRelayException(
        'The relay answered with something that is not JSON',
      );
    }
    if (decoded is! Map<String, dynamic>) {
      throw const SplitsRelayException(
        'The relay answered with something that is not a channel',
      );
    }
    return decoded;
  }
}
