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
