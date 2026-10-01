@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:test/test.dart';
import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';

import 'support/fake_wallet.dart';

/// The two calls `HttpSplitsRelay` is given, over a real socket.
///
/// The package declares no HTTP client of its own so that bill sync takes the
/// same network route as the rest of the wallet — on a build that routes
/// through Tor it goes over Tor and fails closed rather than being the one
/// path that quietly leaves in the clear. Here the wallet is `dart:io`.
({JsonPost post, JsonGet get}) ioClient() {
  final client = HttpClient();
  return (
    post: (Uri url, String body) async {
      final request = await client.postUrl(url);
      request.headers.contentType = ContentType.json;
      request.write(body);
      final response = await request.close();
      return response.transform(utf8.decoder).join();
    },
    get: (Uri url) async {
      final request = await client.getUrl(url);
      final response = await request.close();
      return response.transform(utf8.decoder).join();
    },
  );
}

/// A relay, in as few lines as the two routes take.
///
/// Storage is channel -> set of blobs: deduplicated, so pushing a blob twice
/// is a no-op and a client that re-pushes after a dropped connection cannot
/// create duplicates. It holds no key and reads nothing.
Future<HttpServer> startRelay({
  int maxBlobChars = 64 * 1024,
  Map<String, Set<String>>? store,
}) async {
  final channels = store ?? <String, Set<String>>{};
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((request) async {
    final match = RegExp(r'^/c/([0-9a-f]{64})$').firstMatch(request.uri.path);
    if (match == null) {
      request.response.statusCode = HttpStatus.notFound;
      await request.response.close();
      return;
    }
    final channel = match.group(1)!;
    request.response.headers.contentType = ContentType.json;

    if (request.method == 'POST') {
      final body = jsonDecode(await utf8.decoder.bind(request).join());
      final blobs = (body as Map)['blobs'];
      if (blobs is! List || blobs.any((b) => b is! String)) {
        request.response.write(jsonEncode({'ok': false}));
      } else if (blobs.any((b) => (b as String).length > maxBlobChars)) {
        // The client's bound, kept on this side too: a bound only one side
        // keeps is not a bound.
        request.response.write(jsonEncode({'ok': false}));
      } else {
        (channels[channel] ??= <String>{}).addAll(blobs.cast<String>());
        request.response.write(jsonEncode({'ok': true}));
      }
    } else {
      request.response.write(
        jsonEncode({'blobs': (channels[channel] ?? const <String>{}).toList()}),
      );
    }
    await request.response.close();
  });
  return server;
}

/// One device: its own store and keychain, sharing a relay with the others.
class Device {
  Device(this.wallet, SplitsRelay relay, {int seed = 1})
    : store = BillStore(InMemoryBillStorage()),
      keys = SplitsKeys(store: InMemorySecretStore(), random: Random(seed)) {
    sync = SplitsSync(store: store, keys: keys, relay: relay);
    host = WalletBillHost(wallet);
  }

  final FakeWallet wallet;
  final BillStore store;
  final SplitsKeys keys;
  late final SplitsSync sync;
  late final WalletBillHost host;
}

void main() {
  late HttpServer server;
  late HttpSplitsRelay relay;

  setUp(() async {
    server = await startRelay();
    final io = ioClient();
    relay = HttpSplitsRelay(
      origin: Uri.parse('http://${server.address.host}:${server.port}'),
      post: io.post,
      get: io.get,
    );
  });

  tearDown(() => server.close(force: true));

  test('two phones sync a whole bill over real HTTP', () async {
    final anaWallet = FakeWallet();
    final ana = Device(anaWallet, relay, seed: 1);
    final ben = Device(FakeWallet(id: 'ben', payTo: 'u1ben'), relay, seed: 2);

    final create = splitz.createBill(
      host: ana.host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final id = create['id'] as String;
    await ana.keys.storeBillKey(id, ana.keys.generateKey());
    await ana.store.merge(id, [create]);
    anaWallet.tick();
    await ana.store.merge(id, [
      splitz.joinBill(host: ana.host, name: 'Ana', payTo: 'u1ana'),
    ]);
    await ana.sync.sync(id);

    // Ben scanned the invite, so he holds the id and the key — and nothing
    // else. Everything he learns about the bill comes off the relay.
    await ben.keys.storeBillKey(id, (await ana.keys.readBillKey(id))!);
    final pulled = await ben.sync.pull(id);
    expect(pulled.entries.length, 2);
    expect(pulled.unopenable, 0);
    expect(
      foldUnverified(ben.wallet, pulled.entries, billId: id).bill.name,
      'Dinner',
    );

    ben.wallet.tick();
    ben.wallet.tick();
    await ben.store.merge(id, [
      splitz.joinBill(host: ben.host, name: 'Ben', payTo: 'u1ben'),
    ]);
    await ben.sync.push(id);
    final back = await ana.sync.pull(id);

    expect(
      foldUnverified(
        anaWallet,
        back.entries,
        billId: id,
      ).bill.participants.map((p) => p.id),
      ['ana', 'ben'],
    );
    expect(
      await ana.store.read(id),
      await ben.store.read(id),
      reason: '§10.2 merges by set union, so order cannot separate them',
    );
  });

  test('the relay never sees the bill id, only its hash', () async {
    final seen = <String, Set<String>>{};
    final watched = await startRelay(store: seen);
    final io = ioClient();
    final through = HttpSplitsRelay(
      origin: Uri.parse('http://${watched.address.host}:${watched.port}'),
      post: io.post,
      get: io.get,
    );
    addTearDown(() => watched.close(force: true));

    final ana = Device(FakeWallet(), through, seed: 1);
    final create = splitz.createBill(
      host: ana.host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final id = create['id'] as String;
    await ana.keys.storeBillKey(id, ana.keys.generateKey());
    await ana.store.merge(id, [create]);
    await ana.sync.sync(id);

    expect(seen.keys.single, SplitsChannel.forBill(id));
    expect(seen.keys.single, isNot(id));
    // And what it holds is ciphertext: nothing in a blob reads as the bill.
    for (final blob in seen.values.single) {
      expect(blob, isNot(contains('Dinner')));
      expect(blob, isNot(contains(id)));
    }
  });

  test('pushing the same log twice stores one copy of each entry', () async {
    final seen = <String, Set<String>>{};
    final watched = await startRelay(store: seen);
    final io = ioClient();
    final through = HttpSplitsRelay(
      origin: Uri.parse('http://${watched.address.host}:${watched.port}'),
      post: io.post,
      get: io.get,
    );
    addTearDown(() => watched.close(force: true));

    final ana = Device(FakeWallet(), through, seed: 1);
    final create = splitz.createBill(
      host: ana.host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final id = create['id'] as String;
    await ana.keys.storeBillKey(id, ana.keys.generateKey());
    await ana.store.merge(id, [create]);
    await ana.sync.sync(id);
    final after = seen.values.single.length;
    await ana.sync.sync(id);
    await ana.sync.sync(id);
    expect(
      seen.values.single.length,
      after,
      reason: 'a blob is keyed by its content, so a re-push is a no-op',
    );
  });

  test('a relay that is not there is reported, not swallowed', () async {
    final io = ioClient();
    final dead = HttpSplitsRelay(
      // A port nothing is listening on.
      origin: Uri.parse('http://127.0.0.1:1'),
      post: io.post,
      get: io.get,
    );
    await expectLater(
      () => dead.fetch(SplitsChannel.forBill('b1')),
      throwsA(
        isA<SplitsRelayException>().having(
          (e) => e.isTransient,
          'isTransient',
          isTrue,
        ),
      ),
    );
  });

  test(
    'a relay answering with something that is not a channel is refused',
    () async {
      final rogue = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(() => rogue.close(force: true));
      rogue.listen((request) async {
        request.response.write('<html>not a relay</html>');
        await request.response.close();
      });

      final io = ioClient();
      final through = HttpSplitsRelay(
        origin: Uri.parse('http://${rogue.address.host}:${rogue.port}'),
        post: io.post,
        get: io.get,
      );
      await expectLater(
        () => through.fetch(SplitsChannel.forBill('b1')),
        throwsA(isA<SplitsRelayException>()),
      );
    },
  );

  test('a push past one body is split into bodies that each fit', () {
    // 600 blobs at the per-blob cap are about 39 MB of JSON: past one body.
    final blobs = [
      for (var i = 0; i < 600; i++)
        '${i.toString().padLeft(5, '0')}${'b' * (HttpSplitsRelay.maxBlobChars - 5)}',
    ];
    final bodies = HttpSplitsRelay.pushBodies(blobs);
    expect(bodies.length, greaterThan(1));
    for (final body in bodies) {
      expect(
        utf8.encode(body).length,
        lessThanOrEqualTo(HttpSplitsRelay.maxBodyBytes),
      );
    }
    expect([
      for (final body in bodies) ...(jsonDecode(body)['blobs'] as List),
    ], blobs);
    expect(HttpSplitsRelay.pushBodies(const []), isEmpty);
  });

  test('a blob over the cap is refused before it is sent', () async {
    await expectLater(
      () => relay.push(SplitsChannel.forBill('b1'), [
        'x' * (HttpSplitsRelay.maxBlobChars + 1),
      ]),
      throwsA(
        isA<SplitsRelayException>().having(
          (e) => e.isTransient,
          'isTransient',
          isFalse,
        ),
      ),
    );
  });

  test('an origin carrying a query or a fragment is refused', () {
    // The channel is appended to the path. An origin carrying either would put
    // it after them, addressing something else entirely.
    for (final origin in [
      'https://relay.example?t=1',
      'https://relay.example#top',
    ]) {
      expect(
        () => HttpSplitsRelay(
          origin: Uri.parse(origin),
          post: (url, body) async => '{"ok":true}',
          get: (url) async => '{"blobs":[]}',
        ),
        throwsA(isA<SplitsRelayException>()),
        reason: origin,
      );
    }
    expect(
      HttpSplitsRelay(
        origin: Uri.parse('https://relay.example/base'),
        post: (url, body) async => '{"ok":true}',
        get: (url) async => '{"blobs":[]}',
      ).origin.path,
      '/base',
    );
  });

  test('a channel nobody has pushed to is empty, not an error', () async {
    expect(await relay.fetch(SplitsChannel.forBill('b-unknown')), isEmpty);
  });
}
