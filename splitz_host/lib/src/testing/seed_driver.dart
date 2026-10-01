/// Where a development build gets a seed phrase from.
///
/// **On loopback, at run time, and never from a `--dart-define`.** A phrase
/// passed as a define is compiled into the binary, printed by every tool that
/// dumps a build's defines, and kept in whatever log or screenshot the run
/// produced. A phrase fetched from `127.0.0.1` while the run is happening is
/// in memory and nowhere else, and the driver can be stopped the moment the
/// run ends.
///
/// This is development scaffolding. A build that is given no driver URL asks
/// for nothing and has nothing, which is the state every shipped build is in.
library;

import 'dart:convert';

/// Fetches a URL and returns the body. The wallet's own client, so a run takes
/// the same network route the rest of the app does.
typedef SeedFetch = Future<String> Function(Uri url);

/// Raised when the driver is not there, or does not answer with a phrase.
class SeedDriverException implements Exception {
  const SeedDriverException(this.message);

  final String message;

  @override
  String toString() => 'SeedDriverException: $message';
}

/// The development seed driver, as a build talks to it.
///
/// Two routes under [origin], which is the URL the driver printed when it
/// started — its per-run token is the last segment of the path:
///
///     GET <origin>/health        -> 200, so a script can wait for it
///     GET <origin>/seed/<index>  -> {"seed": "<phrase>", "name": "<label>"}
class SeedDriver {
  const SeedDriver({required this.origin, required SeedFetch fetch})
    : _fetch = fetch;

  /// Builds one from the URL a development build was given, or returns null
  /// when it was given none — which is every build that is not a test run.
  static SeedDriver? fromEnvironment({
    required SeedFetch fetch,
    String url = const String.fromEnvironment('SPLITS_SEED_DRIVER_URL'),
  }) {
    if (url.isEmpty) return null;
    final origin = Uri.tryParse(url);
    if (origin == null || !origin.hasScheme) return null;
    return SeedDriver(origin: origin, fetch: fetch);
  }

  final Uri origin;
  final SeedFetch _fetch;

  /// Whether the driver is answering.
  ///
  /// Asked before a run rather than discovered during one: a lane that starts
  /// without its seeds fails somewhere further in, where the message names the
  /// wrong thing.
  Future<bool> get isUp async {
    try {
      await _fetch(origin.replace(path: '${origin.path}/health'));
      return true;
    } on Object {
      return false;
    }
  }

  /// The phrase for [seedIndex], and the account it belongs to.
  ///
  /// The phrase is returned and not stored. Whatever holds it decides how long
  /// it lives; this does not put it in a field where it would outlive the call.
  Future<({String phrase, String? name})> seedAt(int seedIndex) async {
    final String body;
    try {
      body = await _fetch(
        origin.replace(path: '${origin.path}/seed/$seedIndex'),
      );
    } on Object catch (e) {
      throw SeedDriverException('The seed driver did not answer: $e');
    }

    final Object? decoded;
    try {
      decoded = jsonDecode(body);
    } on FormatException {
      throw const SeedDriverException(
        'The seed driver answered with something that is not JSON',
      );
    }
    if (decoded is! Map || decoded['seed'] is! String) {
      throw const SeedDriverException(
        'The seed driver answered without a seed',
      );
    }
    final phrase = decoded['seed'] as String;
    if (phrase.trim().isEmpty) {
      throw SeedDriverException('The driver has no wallet at index $seedIndex');
    }
    // The label is whatever the driver returned, so no wallet of anyone's is
    // described in this package. A caller that wants its own names holds them
    // itself and matches on the index, which is the identifier.
    final name = decoded['name'];
    return (phrase: phrase, name: name is String ? name : null);
  }
}
