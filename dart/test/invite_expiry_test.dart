import 'package:splitz_core/splitz_core.dart';
import 'package:test/test.dart';

const _bill = 'Ab3-_xyz';
final _key = 'k' * 43;

void main() {
  test('an invite is expired once the clock is past its expiry', () {
    final invite = Invite(billId: _bill, key: _key, expiry: 1793000000);
    expect(isInviteExpired(invite, 1792999999), isFalse);
    expect(isInviteExpired(invite, 1793000000), isFalse);
    expect(isInviteExpired(invite, 1793000001), isTrue);
  });

  test('an invite with no expiry never expires', () {
    expect(isInviteExpired(Invite(billId: _bill, key: _key), 1 << 62), isFalse);
  });

  test('the largest expiry an invite carries compares without overflow', () {
    final invite = parseInvite(
      'splitz://join?v=1&b=$_bill&k=$_key&x=9223372036854775807',
    );
    expect(isInviteExpired(invite, 1793000000), isFalse);
  });
}
