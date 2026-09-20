@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';

import 'package:test/test.dart';
import 'package:splitz_host/splitz_host.dart';

/// Runs `tool/seed-driver.py` against a throwaway seed file.
///
/// The real one is pointed at a file of real phrases; this is pointed at four
/// lines of nonsense, so the lane proves the mechanism without a secret ever
/// being in the room.
/// Twelve words, which is what makes the driver recognise a line as a phrase.
/// Nonsense words on purpose: it counts words, and a dictionary check belongs
/// to the wallet that imports one.
String twelveWords(String tag) =>
    List<String>.generate(12, (i) => '$tag$i').join(' ');

Future<({Process process, int port, Directory dir})> startDriver({
  List<String>? phrases,
}) async {
  phrases ??= [
    for (final tag in ['zero', 'one', 'two', 'three']) twelveWords(tag),
  ];
  final dir = await Directory.systemTemp.createTemp('seed-driver-test');
  final file = File('${dir.path}/seeds')
    ..writeAsStringSync('${phrases.join('\n')}\n');
  await file.setLastModified(DateTime.now());
  // 600, so the driver's own warning about a world-readable file does not fire.
  await Process.run('chmod', ['600', file.path]);

  final port = 39200 + DateTime.now().microsecond % 2000;
  final process = await Process.start('python3', [
    'tool/seed-driver.py',
    file.path,
    '--port',
    '$port',
  ]);

  // Wait for it to bind rather than sleeping: a lane that starts before its
  // driver fails somewhere further in, where the message names the wrong
  // thing.
  final client = HttpClient();
  for (var i = 0; i < 100; i++) {
    try {
      final request = await client.getUrl(
        Uri.parse('http://127.0.0.1:$port/health'),
      );
      await (await request.close()).drain<void>();
      return (process: process, port: port, dir: dir);
    } on Object {
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
  }
  process.kill();
  throw StateError('the seed driver did not come up on $port');
}

Future<String> Function(Uri) ioFetch() {
  final client = HttpClient();
  return (Uri url) async {
    final request = await client.getUrl(url);
    final response = await request.close();
    if (response.statusCode >= 400) {
      throw HttpException('${response.statusCode} for $url');
    }
    return response.transform(utf8.decoder).join();
  };
}

void main() {
  test('a build given no driver URL asks for nothing', () {
    // Which is the state every shipped build is in.
    expect(SeedDriver.fromEnvironment(fetch: ioFetch(), url: ''), isNull);
    expect(
      SeedDriver.fromEnvironment(fetch: ioFetch(), url: 'not a url'),
      isNull,
    );
  });

  test('the driver serves a phrase by index, and names the wallet', () async {
    final driver = await startDriver();
    addTearDown(() async {
      driver.process.kill();
      await driver.dir.delete(recursive: true);
    });

    final client = SeedDriver(
      origin: Uri.parse('http://127.0.0.1:${driver.port}'),
      fetch: ioFetch(),
    );

    expect(await client.isUp, isTrue);

    // The index is the identifier. A label, if the driver sends one, is
    // passed through untouched; this package describes no wallet of its own.
    final two = await client.seedAt(1);
    expect(two.phrase, twelveWords('one'));

    final four = await client.seedAt(2);
    expect(four.phrase, isNot(two.phrase));
  });

  test(
    'an index the driver does not serve is refused, not guessed at',
    () async {
      final driver = await startDriver(phrases: [twelveWords('only')]);
      addTearDown(() async {
        driver.process.kill();
        await driver.dir.delete(recursive: true);
      });

      final client = SeedDriver(
        origin: Uri.parse('http://127.0.0.1:${driver.port}'),
        fetch: ioFetch(),
      );
      await expectLater(
        () => client.seedAt(3),
        throwsA(isA<SeedDriverException>()),
      );
    },
  );

  test('a shell-style file is read, and only its phrases are served', () async {
    // The shape the real seed file has: `NAME=phrase` lines beside other
    // variables. Serving an address or a height as a phrase produces a message
    // about word counts that names the wrong thing.
    final driver = await startDriver(
      phrases: [
        'WORDS1=${twelveWords('one')}',
        'ADDRESS1=u1someaddressthatisnotaphrase',
        '# a comment',
        'HEIGHT1=0',
        'export WORDS2="${twelveWords('two')}"',
      ],
    );
    addTearDown(() async {
      driver.process.kill();
      await driver.dir.delete(recursive: true);
    });

    final client = SeedDriver(
      origin: Uri.parse('http://127.0.0.1:${driver.port}'),
      fetch: ioFetch(),
    );
    expect((await client.seedAt(0)).phrase, twelveWords('one'));
    expect(
      (await client.seedAt(1)).phrase,
      twelveWords('two'),
      reason: 'the non-phrase lines between them are not served',
    );
    await expectLater(
      () => client.seedAt(2),
      throwsA(isA<SeedDriverException>()),
    );
  });

  test(
    'a driver that is not running is reported before a run starts',
    () async {
      final client = SeedDriver(
        origin: Uri.parse('http://127.0.0.1:1'),
        fetch: ioFetch(),
      );
      expect(await client.isUp, isFalse);
      await expectLater(
        () => client.seedAt(0),
        throwsA(isA<SeedDriverException>()),
      );
    },
  );

  test('the driver binds to loopback and nowhere else', () async {
    final driver = await startDriver();
    addTearDown(() async {
      driver.process.kill();
      await driver.dir.delete(recursive: true);
    });

    // A driver reachable off the machine is a seed phrase on a network.
    final addresses = await NetworkInterface.list(
      includeLoopback: false,
      type: InternetAddressType.IPv4,
    );
    for (final interface in addresses) {
      for (final address in interface.addresses) {
        final socket = await Socket.connect(
          address,
          driver.port,
          timeout: const Duration(milliseconds: 300),
        ).then<Socket?>((s) => s, onError: (_) => null);
        addTearDown(() => socket?.destroy());
        expect(
          socket,
          isNull,
          reason: 'answered on ${address.address}, which is not loopback',
        );
      }
    }
  });
}
