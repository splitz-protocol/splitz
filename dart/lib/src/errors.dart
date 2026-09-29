/// Every refusal this library makes, and the codes that name them.
///
/// The code is part of the protocol; the message beside it is prose and is not
/// (SPEC.md §12). A user-facing string is derived from the code, never parsed
/// out of the message.
library;

/// The refusal codes of SPEC.md §12.
abstract final class SplitCode {
  // §3 allocation
  static const emptyWeights = 'empty_weights';
  static const negativeWeight = 'negative_weight';
  static const zeroWeightSum = 'zero_weight_sum';
  static const allocationOverflow = 'allocation_overflow';
  static const weightSumOverflow = 'weight_sum_overflow';

  // §4 split methods
  static const negativeShare = 'negative_share';
  static const amountOverflow = 'amount_overflow';
  static const amountTooLarge = 'amount_too_large';
  static const emptySplit = 'empty_split';
  static const exactTotalMismatch = 'exact_total_mismatch';
  static const percentageNotFullScale = 'percentage_not_full_scale';
  static const itemizedNoItems = 'itemized_no_items';
  static const itemizedUnassignedItem = 'itemized_unassigned_item';
  static const itemizedTotalMismatch = 'itemized_total_mismatch';

  // §2, §5, §6
  static const currencyMismatch = 'currency_mismatch';
  static const unknownParticipant = 'unknown_participant';
  static const unknownEntry = 'unknown_entry';
  static const duplicateParticipant = 'duplicate_participant';
  static const duplicatePayment = 'duplicate_payment';
  static const duplicateExpense = 'duplicate_expense';
  static const selfPayment = 'self_payment';
  static const balancesNonzeroResidual = 'balances_nonzero_residual';
  static const exactLimitTooLarge = 'exact_limit_too_large';

  // §7 rates
  static const rateCurrencyMismatch = 'rate_currency_mismatch';
  static const rateNotPositive = 'rate_not_positive';
  static const negativeAmount = 'negative_amount';
  static const rateAmountTooLarge = 'rate_amount_too_large';

  // §9 the bill wire format
  static const billMissingVersion = 'bill_missing_version';
  static const billFutureVersion = 'bill_future_version';
  static const billTypeError = 'bill_type_error';
  static const billNotScalarValues = 'bill_not_scalar_values';
  static const entryIdNotDerived = 'entry_id_not_derived';
  static const billBadParticipantId = 'bill_bad_participant_id';
  static const billMissingCurrency = 'bill_missing_currency';
  static const billBadCurrency = 'bill_bad_currency';
  static const billUnknownSplitType = 'bill_unknown_split_type';
  static const billUnknownSplitMode = 'bill_unknown_split_mode';
  static const billUnknownEntryKind = 'bill_unknown_entry_kind';
  static const billUnknownSettlementMethod = 'bill_unknown_settlement_method';
  static const billUnknownPayoutMethod = 'bill_unknown_payout_method';
  static const billUnknownConfirmationMethod =
      'bill_unknown_confirmation_method';
  static const billAmbiguousEntry = 'bill_ambiguous_entry';
  static const billMissingEntryPayload = 'bill_missing_entry_payload';
  static const canonicalJsonFloat = 'canonical_json_float';

  // §10 the log
  static const logEmpty = 'log_empty';
  static const logNoCreate = 'log_no_create';
  static const ambiguousCreate = 'ambiguous_create';
  static const createUnbound = 'create_unbound';
  static const createIdNotDerived = 'create_id_not_derived';
  static const unauthorizedEntry = 'unauthorized_entry';
  static const unauthorizedPayment = 'unauthorized_payment';
  static const unauthorizedConfirmation = 'unauthorized_confirmation';
  static const confirmationMissingReference = 'confirmation_missing_reference';
  static const unknownPayment = 'unknown_payment';
  static const amendKindMismatch = 'amend_kind_mismatch';
  static const participantStillNamed = 'participant_still_named';
  static const participantIdNotDerived = 'participant_id_not_derived';

  // §11 invites, payloads and sealing
  static const inviteNotAnInvite = 'invite_not_an_invite';
  static const inviteMissingVersion = 'invite_missing_version';
  static const inviteFutureVersion = 'invite_future_version';
  static const inviteMissingBillId = 'invite_missing_bill_id';
  static const inviteBadBillId = 'invite_bad_bill_id';
  static const inviteMissingKey = 'invite_missing_key';
  static const inviteBadExpiry = 'invite_bad_expiry';
  static const inviteBadLink = 'invite_bad_link';
  static const payloadNotAPayload = 'payload_not_a_payload';
  static const payloadDamaged = 'payload_damaged';
  static const payloadMissingBody = 'payload_missing_body';
  static const payloadFutureVersion = 'payload_future_version';
  static const payloadTooLarge = 'payload_too_large';
  static const sealedMalformed = 'sealed_malformed';
  static const sealedFutureVersion = 'sealed_future_version';

  // §8 payment requests
  static const zip321NoPayments = 'zip321_no_payments';
  static const zip321TooManyPayments = 'zip321_too_many_payments';
  static const zip321AmountNotPositive = 'zip321_amount_not_positive';
  static const zip321AmountTooLarge = 'zip321_amount_too_large';
  static const zip321MemoTooLarge = 'zip321_memo_too_large';
  static const zip321BadAddress = 'zip321_bad_address';
  static const zip321BadCurrencyCode = 'zip321_bad_currency_code';
  static const zip321FiatNotPositive = 'zip321_fiat_not_positive';
  static const zip321FiatTooManyDigits = 'zip321_fiat_too_many_digits';
  static const zip321NoAddress = 'zip321_no_address';
  static const zip321NotCanonical = 'zip321_not_canonical';
  static const zip321MemoUndeliverable = 'zip321_memo_undeliverable';

  // §8.6 addresses
  static const addressInvalid = 'address_invalid';
}

/// A refusal, carrying the code that names it.
class SplitError implements Exception {
  const SplitError(this.code, this.message);

  /// The code from SPEC.md §12. A caller branches on this, never on [message].
  final String code;

  /// Prose for a developer. Not part of the protocol.
  final String message;

  @override
  String toString() => 'SplitError($code): $message';
}

Never raise(String code, String message) => throw SplitError(code, message);

/// The sentence a host MAY show a person for each §12 code.
///
/// A default, not part of the protocol: the code is what a caller branches on,
/// and a host with its own words keeps them. `vectors/messages.json` holds the
/// same table, and both implementations reproduce it.
const Map<String, String> _plainMessages = {
  // Generated by tools/corpus/messages.py.
  'address_invalid': 'This isn\'t a Zcash address.',
  'allocation_overflow': 'This amount is too large to divide.',
  'ambiguous_create': 'This bill was started twice. It can\'t be opened.',
  'amend_kind_mismatch': 'A change must be the same kind as what it changes.',
  'amount_overflow': 'This amount is too large.',
  'amount_too_large': 'This amount is too large.',
  'balances_nonzero_residual':
      'The balances don\'t add up. The bill may be damaged.',
  'bill_ambiguous_entry': 'Part of this bill is damaged.',
  'bill_bad_currency': 'This bill\'s currency isn\'t valid.',
  'bill_bad_participant_id': 'Part of this bill is damaged.',
  'bill_future_version': 'This bill needs a newer app. Update to open it.',
  'bill_missing_currency': 'This bill has no currency.',
  'bill_missing_entry_payload': 'Part of this bill is damaged.',
  'bill_missing_version': 'This bill can\'t be read.',
  'bill_not_scalar_values': 'This bill is damaged.',
  'bill_type_error': 'This bill is damaged.',
  'bill_unknown_confirmation_method':
      'This bill needs a newer app. Update to open it.',
  'bill_unknown_entry_kind': 'This bill needs a newer app. Update to open it.',
  'bill_unknown_payout_method':
      'This bill needs a newer app. Update to open it.',
  'bill_unknown_settlement_method':
      'This bill needs a newer app. Update to open it.',
  'bill_unknown_split_mode': 'This bill needs a newer app. Update to open it.',
  'bill_unknown_split_type': 'This bill needs a newer app. Update to open it.',
  'canonical_json_float': 'This bill holds a number this app can\'t read.',
  'confirmation_missing_reference': 'Add the transaction to confirm this.',
  'create_id_not_derived': 'This bill\'s start is damaged.',
  'create_unbound': 'This bill\'s start is damaged.',
  'currency_mismatch': 'This uses a different currency from the bill.',
  'duplicate_expense': 'This expense is already on the bill.',
  'duplicate_participant': 'That person is already on this bill.',
  'duplicate_payment': 'This payment is already recorded.',
  'empty_split': 'Pick who shares this cost.',
  'empty_weights': 'Nobody is sharing this cost.',
  'entry_id_not_derived': 'Part of this bill is damaged.',
  'exact_limit_too_large': 'This bill has too many people to settle exactly.',
  'exact_total_mismatch': 'The amounts don\'t add up to the total.',
  'invite_bad_bill_id': 'This invite is damaged. Ask for a new one.',
  'invite_bad_expiry': 'This invite is damaged. Ask for a new one.',
  'invite_bad_link': 'This invite link can\'t be made from that address.',
  'invite_future_version': 'This invite needs a newer app. Update to join.',
  'invite_missing_bill_id': 'This invite is incomplete. Ask for a new one.',
  'invite_missing_key': 'This invite is incomplete. Ask for a new one.',
  'invite_missing_version': 'This invite can\'t be read.',
  'invite_not_an_invite': 'This isn\'t a bill invite.',
  'itemized_no_items': 'Add at least one item.',
  'itemized_total_mismatch': 'The items don\'t add up to the total.',
  'itemized_unassigned_item': 'Pick who shares each item.',
  'log_empty': 'This bill is empty.',
  'log_no_create': 'This bill\'s start is missing. Sync again.',
  'negative_amount': 'An amount can\'t be negative.',
  'negative_share': 'A share can\'t be negative.',
  'negative_weight': 'A share can\'t be negative.',
  'participant_id_not_derived': 'This person\'s id doesn\'t match their key.',
  'participant_still_named':
      'This person still has costs or payments on the bill.',
  'payload_damaged': 'This code is damaged. Scan it again.',
  'payload_future_version': 'This code needs a newer app. Update to scan it.',
  'payload_missing_body': 'This code is incomplete. Scan it again.',
  'payload_not_a_payload': 'This isn\'t a bill code.',
  'payload_too_large': 'This bill is too big for one code. Share an invite.',
  'percentage_not_full_scale': 'The percentages don\'t add up to 100.',
  'rate_amount_too_large': 'This amount is too large to price.',
  'rate_currency_mismatch':
      'This price is in a different currency from the bill.',
  'rate_not_positive': 'A price must be more than zero.',
  'sealed_future_version': 'An update to this bill needs a newer app.',
  'sealed_malformed': 'An update to this bill is damaged.',
  'self_payment': 'You can\'t pay yourself.',
  'unauthorized_confirmation': 'Only the person paid can confirm this.',
  'unauthorized_entry': 'Only the person who wrote this can change it.',
  'unauthorized_payment': 'Only the payer can record this payment.',
  'unknown_entry': 'That item isn\'t on this bill.',
  'unknown_participant': 'That person isn\'t on this bill.',
  'unknown_payment': 'That payment isn\'t on this bill.',
  'weight_sum_overflow': 'The shares are too large to add up.',
  'zero_weight_sum': 'The shares add up to nothing.',
  'zip321_amount_not_positive': 'An amount to pay must be more than zero.',
  'zip321_amount_too_large': 'This amount is more ZEC than exists.',
  'zip321_bad_address': 'An address on this bill isn\'t valid.',
  'zip321_bad_currency_code': 'This bill\'s currency isn\'t valid.',
  'zip321_fiat_not_positive': 'An amount must be more than zero.',
  'zip321_fiat_too_many_digits': 'This amount is too large.',
  'zip321_memo_too_large': 'This note is too long.',
  'zip321_memo_undeliverable': 'A note can\'t be sent to this address.',
  'zip321_no_address': 'Someone on this bill has no address to be paid at.',
  'zip321_no_payments': 'There\'s nothing to pay.',
  'zip321_not_canonical': 'This payment request wasn\'t made by this bill.',
  'zip321_too_many_payments': 'Too many people to pay at once.',
  // End of generated table.
};

/// A plain-language sentence for the §12 [code], or null for a code this
/// library does not define.
///
/// Null rather than a generic sentence: a code from a newer version is
/// something the caller can see and say, and a guess would hide it.
String? describeCode(String code) => _plainMessages[code];
