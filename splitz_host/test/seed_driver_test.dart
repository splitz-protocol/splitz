@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';

import 'package:test/test.dart';
import 'package:splitz_host/splitz_host.dart';
import 'support/process_port.dart';

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

Future<({Process process, int port, Uri url, Directory dir})> startDriver({
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

  final up = await startOnFreePort('tool/seed-driver.py', [file.path]);
  return (process: up.process, port: up.port, url: Uri.parse(up.url), dir: dir);
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

  test(
    'a request naming another host, or sent by a browser, gets no phrase',
    () async {
      // A web page whose name resolves to 127.0.0.1 (DNS rebinding) reaches the
      // driver with its own name as the Host, and a browser sends an Origin.
      // A local caller does neither.
      final driver = await startDriver();
      addTearDown(() async {
        driver.process.kill();
        await driver.dir.delete(recursive: true);
      });
      final client = HttpClient();
      Future<int> status(Map<String, String> headers) async {
        final request = await client.getUrl(Uri.parse('${driver.url}/seed/0'));
        headers.forEach(request.headers.set);
        final response = await request.close();
        await response.drain<void>();
        return response.statusCode;
      }

      expect(await status({'Host': 'rebind.example:${driver.port}'}), 403);
      expect(await status({'Origin': 'https://rebind.example'}), 403);
      expect(await status(const {}), 200, reason: 'a local caller is served');
    },
  );

  test('a caller without the run\'s token gets no phrase', () async {
    // Any process on this machine reaches loopback. The token is printed to
    // the one that started the driver, and is what it hands its run.
    final driver = await startDriver();
    addTearDown(() async {
      driver.process.kill();
      await driver.dir.delete(recursive: true);
    });
    final client = HttpClient();
    Future<int> status(String path) async {
      final request = await client.getUrl(
        Uri.parse('http://127.0.0.1:${driver.port}$path'),
      );
      final response = await request.close();
      await response.drain<void>();
      return response.statusCode;
    }

    expect(await status('/seed/0'), 403);
    expect(await status('/health'), 403);
    expect(await status('/wrong-token/seed/0'), 403);
    expect(await status('${driver.url.path}/seed/0'), 200);
  });

  test('the driver serves a phrase by index, and names the wallet', () async {
    final driver = await startDriver();
    addTearDown(() async {
      driver.process.kill();
      await driver.dir.delete(recursive: true);
    });

    final client = SeedDriver(origin: driver.url, fetch: ioFetch());

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

      final client = SeedDriver(origin: driver.url, fetch: ioFetch());
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

    final client = SeedDriver(origin: driver.url, fetch: ioFetch());
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

  group('a define file', () {
    Future<(Process, Directory, File)> start(File define) async {
      final dir = await Directory.systemTemp.createTemp('seed-driver-define');
      final seeds = File('${dir.path}/seeds')
        ..writeAsStringSync('${twelveWords('zero')}\n');
      await Process.run('chmod', ['600', seeds.path]);
      final process = await Process.start('python3', [
        'tool/seed-driver.py',
        seeds.path,
        '--port',
        '0',
        '--define-file',
        define.path,
      ]);
      // Stopped whatever the test's outcome: a failed expectation would
      // otherwise leave the driver serving until someone kills it.
      addTearDown(process.kill);
      process.stdout.drain<void>();
      process.stderr.drain<void>();
      return (process, dir, seeds);
    }

    Future<void> until(bool Function() ready) async {
      for (var i = 0; i < 300 && !ready(); i++) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
    }

    test(
      'holds the URL, owner-only, and is gone when the driver stops',
      () async {
        final home = await Directory.systemTemp.createTemp('define');
        final define = File('${home.path}/driver.json');
        final (process, dir, _) = await start(define);
        await until(() => define.existsSync() && define.lengthSync() > 0);
        // The permission bits, read through Dart rather than `stat`, whose
        // flags differ between BSD and GNU.
        expect(define.statSync().mode & 0x1ff, 0x180, reason: 'mode 0600');
        final url =
            (jsonDecode(define.readAsStringSync())
                    as Map)['SPLITS_SEED_DRIVER_URL']
                as String;
        expect(url, startsWith('http://127.0.0.1:'));
        final health = await ioFetch()(Uri.parse('$url/health'));
        final answer = jsonDecode(health) as Map;
        expect(answer['ok'], isTrue);
        expect(answer['wallets'], 1);

        process.kill(ProcessSignal.sigint);
        await process.exitCode;
        expect(define.existsSync(), isFalse);
        await dir.delete(recursive: true);
        await home.delete(recursive: true);
      },
    );

    test('is never written over a file already at its path', () async {
      final home = await Directory.systemTemp.createTemp('define');
      final define = File('${home.path}/driver.json')..writeAsStringSync('x');
      final (process, dir, _) = await start(define);
      expect(await process.exitCode, isNot(0));
      expect(define.readAsStringSync(), 'x');
      await dir.delete(recursive: true);
      await home.delete(recursive: true);
    });
  });
}
