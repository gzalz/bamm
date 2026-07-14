use solana_program_error::ProgramError;
use wincode::{SchemaRead, SchemaWrite};

pub trait OnchainAccount {
    const DISCRIMINATOR: [u8; 8];
}

/// The proPAMM pool account.
///
/// A proactive market maker quotes both sides off a single oracle-supplied
/// mid-price. This bare template keeps nothing but that mid-point; a swap
/// derives the executable bid/ask from it by applying a fixed spread.
///
/// The account is (de)serialized with `wincode`, producing a bincode-compatible
/// layout of the 8-byte `discriminator` followed by the 16-byte `mid`. Nothing
/// in the program addresses these fields by byte offset — reads and writes go
/// through [`Pool::load`] and [`Pool::store`].
///
/// Both legs are arbitrary SPL tokens supplied at pool init; neither side is
/// pinned to a specific mint.
///
/// `mid` is the price of the right token denominated in the left token, as a
/// Q64.64 fixed-point number:
///
///     right_units = (left_units * mid) >> 64
///     left_units = (right_units << 64) / mid
#[derive(SchemaWrite, SchemaRead)]
pub struct Pool {
    /// Account discriminator; must equal [`Pool::DISCRIMINATOR`].
    pub discriminator: [u8; 8],
    /// Oracle mid-price (Q64.64).
    pub mid: u128,
    /// Slot at which the mid-price was last updated.
    pub last_updated_slot: u64,
    /// Unix timestamp at which the mid-price was last updated.
    pub last_updated_timestamp: i64,
    /// Authority permitted to withdraw liquidity from the vault. Set once at
    /// [`super::instructions::InitPool`] to the pool creator.
    pub authority: [u8; 32],
}

impl Pool {
    /// Total serialized account size: discriminator (8) + mid (16) +
    /// last_updated_slot (8) + last_updated_timestamp (8) + authority (32).
    pub const SIZE: usize = 8 + 16 + 8 + 8 + 32;

    /// Byte offset of the `mid` field in the serialized account. The bincode
    /// layout places the 8-byte discriminator first, so `mid` starts at 8.
    /// Used by hot-path instructions that overwrite `mid` in place to avoid the
    /// CU cost of a full (de)serialization.
    pub const MID_OFFSET: usize = 8;

    /// Byte offset of `last_updated_slot` (follows the 16-byte `mid`).
    pub const SLOT_OFFSET: usize = Self::MID_OFFSET + 16;

    /// Byte offset of `last_updated_timestamp` (follows the 8-byte slot).
    pub const TIMESTAMP_OFFSET: usize = Self::SLOT_OFFSET + 8;

    /// Byte offset of `authority` (follows the 8-byte timestamp).
    pub const AUTHORITY_OFFSET: usize = Self::TIMESTAMP_OFFSET + 8;

    /// Build a pool with the canonical discriminator, the given mid-price, and
    /// the withdraw authority. The last-updated slot/timestamp start at zero
    /// until the first update.
    pub fn new(mid: u128, authority: [u8; 32]) -> Self {
        Self {
            discriminator: Self::DISCRIMINATOR,
            mid,
            last_updated_slot: 0,
            last_updated_timestamp: 0,
            authority,
        }
    }

    /// Deserialize a pool from raw account data with `wincode`, verifying the
    /// discriminator.
    pub fn load(data: &[u8]) -> Result<Self, ProgramError> {
        let pool = wincode::deserialize::<Pool>(data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if pool.discriminator != Self::DISCRIMINATOR {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(pool)
    }

    /// Serialize this pool back into raw account data with `wincode`.
    pub fn store(&self, data: &mut [u8]) -> Result<(), ProgramError> {
        let mut writer: &mut [u8] = data;
        wincode::serialize_into(&mut writer, self).map_err(|_| ProgramError::InvalidAccountData)
    }
}

impl OnchainAccount for Pool {
    const DISCRIMINATOR: [u8; 8] = *b"pool0000";
}
