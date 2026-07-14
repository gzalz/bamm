use solana_program_error::ProgramError;

/// Program-specific error codes, surfaced to clients as
/// [`ProgramError::Custom`].
#[repr(u32)]
pub enum PammError {
    /// The oracle mid is stale: more than 200 ms elapsed between the pool's
    /// last update and the batch clock's current timestamp.
    StaleQuoteMillis = 0,
    /// The batch clock slot is not current: it lags the syscall clock slot, so
    /// the swap is rejected rather than trusting a stale wall-clock reading.
    StaleQuoteSlots = 1,
    /// The signer is not the pool authority recorded at InitPool.
    Unauthorized = 2,
    /// The swap output fell below the caller's `min_tokens_out` threshold.
    SlippageExceeded = 3,
    /// The batch clock's `slot_owner` is not in `trusted_signers`, so its
    /// slot/timestamp readings are not honoured.
    BlockBuilderNotTrusted = 4,
}

impl From<PammError> for ProgramError {
    fn from(e: PammError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
