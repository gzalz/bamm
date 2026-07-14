use crate::error::PammError;
use crate::state::{BatchClock, Pool};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::{clock::Clock, rent::Rent, Sysvar};
use pinocchio::{AccountView, Address, ProgramResult};
use pinocchio_system;
use solana_program_error::ProgramError;
use solana_program_log::log;

/// PDA seed for the pool state account.
pub const POOL_SEED: &[u8] = b"pool";
/// PDA seed for the vault. The vault holds no inventory itself; it is the common
/// authority over both program-owned token accounts (the left and right legs).
pub const VAULT_SEED: &[u8] = b"vault";
/// PDA seed for the program-owned left-token account.
pub const LEFT_ATA_SEED: &[u8] = b"leftata";
/// PDA seed for the program-owned right-token account.
pub const RIGHT_ATA_SEED: &[u8] = b"rightata";

/// SPL token account size in bytes.
const TOKEN_ACCOUNT_SIZE: u64 = 165;

pub trait Instruction {
    const DISCRIMINATOR: [u8; 8];

    fn process(
        &self,
        program_id: &Address,
        accounts: &[AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult;

    fn invoked(instruction_data: &[u8]) -> bool {
        instruction_data.len() >= 8 && instruction_data[..8] == Self::DISCRIMINATOR
    }
}

fn q64_mul(amount: u64, price: u128) -> Result<u64, ProgramError> {
    let result = (amount as u128)
        .checked_mul(price)
        .ok_or(ProgramError::InvalidInstructionData)?
        >> 64;
    u64::try_from(result).map_err(|_| ProgramError::InvalidInstructionData)
}

/// `(amount << 64) / price` — convert a right-token amount into left-token units.
fn q64_div(amount: u64, price: u128) -> Result<u64, ProgramError> {
    if price == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let result = ((amount as u128) << 64) / price;
    u64::try_from(result).map_err(|_| ProgramError::InvalidInstructionData)
}

pub struct InitPool;

impl Instruction for InitPool {
    const DISCRIMINATOR: [u8; 8] = *b"initpool";

    fn process(
        &self,
        program_id: &Address,
        accounts: &[AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult {
        if instruction_data.len() < 12 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let pool_bump = instruction_data[8];
        let vault_bump = instruction_data[9];
        let left_ata_bump = instruction_data[10];
        let right_ata_bump = instruction_data[11];

        // payer           - funds the account creations (signer)
        // pool            - PDA [b"pool"]: to be created, program-owned
        // vault           - PDA [b"vault"]: to be seeded; the common authority
        //                   over both leg token accounts
        // mint_left       - the left-token mint (any SPL mint)
        // left_ata        - PDA [b"leftata"]: to be created + initialized as a
        //                   left-token account owned by `vault`
        // mint_right      - the right-token mint (any SPL mint)
        // right_ata       - PDA [b"rightata"]: to be created + initialized as a
        //                   right-token account owned by `vault`
        // token_program   - SPL token program
        // system_program  - system program
        let [payer, pool, vault, mint_left, left_ata, mint_right, right_ata, _token_program, _system_program] =
            match accounts {
                [payer, pool, vault, mint_left, left_ata, mint_right, right_ata, token_program, system_program] => {
                    [
                        payer,
                        pool,
                        vault,
                        mint_left,
                        left_ata,
                        mint_right,
                        right_ata,
                        token_program,
                        system_program,
                    ]
                }
                _ => return Err(ProgramError::InvalidAccountData),
            };

        if !payer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let rent = Rent::get()?;

        // 1. Create the pool state account (program-owned).
        let pool_bump_seed = [pool_bump];
        let pool_seeds = [Seed::from(POOL_SEED), Seed::from(&pool_bump_seed)];
        pinocchio_system::instructions::CreateAccount {
            from: payer,
            to: pool,
            lamports: rent.try_minimum_balance(Pool::SIZE)?,
            space: Pool::SIZE as u64,
            owner: program_id,
        }
        .invoke_signed(&[Signer::from(&pool_seeds)])?;

        {
            // mid stays 0 until the first UpdateOracle. The payer becomes the
            // authority permitted to withdraw liquidity.
            let mut data = pool.try_borrow_mut()?;
            Pool::new(0, payer.address().to_bytes()).store(&mut data)?;
        }

        // 2. Seed the vault PDA. It holds no inventory of its own; it exists only
        //    as the common authority over both leg token accounts.
        let vault_bump_seed = [vault_bump];
        let vault_seeds = [Seed::from(VAULT_SEED), Seed::from(&vault_bump_seed)];
        pinocchio_system::instructions::CreateAccount {
            from: payer,
            to: vault,
            lamports: rent.try_minimum_balance(0)?,
            space: 0,
            owner: &pinocchio_system::id(),
        }
        .invoke_signed(&[Signer::from(&vault_seeds)])?;

        // 3. Create + initialize the program-owned left-token account.
        let left_ata_bump_seed = [left_ata_bump];
        let left_ata_seeds = [Seed::from(LEFT_ATA_SEED), Seed::from(&left_ata_bump_seed)];
        pinocchio_system::instructions::CreateAccount {
            from: payer,
            to: left_ata,
            lamports: rent.try_minimum_balance(TOKEN_ACCOUNT_SIZE as usize)?,
            space: TOKEN_ACCOUNT_SIZE,
            owner: &pinocchio_token::id(),
        }
        .invoke_signed(&[Signer::from(&left_ata_seeds)])?;

        pinocchio_token::instructions::InitializeAccount3 {
            account: left_ata,
            mint: mint_left,
            owner: vault.address(),
        }
        .invoke()?;

        // 4. Create + initialize the program-owned right-token account.
        let right_ata_bump_seed = [right_ata_bump];
        let right_ata_seeds = [Seed::from(RIGHT_ATA_SEED), Seed::from(&right_ata_bump_seed)];
        pinocchio_system::instructions::CreateAccount {
            from: payer,
            to: right_ata,
            lamports: rent.try_minimum_balance(TOKEN_ACCOUNT_SIZE as usize)?,
            space: TOKEN_ACCOUNT_SIZE,
            owner: &pinocchio_token::id(),
        }
        .invoke_signed(&[Signer::from(&right_ata_seeds)])?;

        pinocchio_token::instructions::InitializeAccount3 {
            account: right_ata,
            mint: mint_right,
            owner: vault.address(),
        }
        .invoke()?;

        log!("Pool initialized");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DepositLiquidity — move left and/or right tokens from the caller's accounts
// into the program-owned vault leg accounts. The caller signs for their own
// token accounts, so no vault signature is required. Either amount may be zero
// to deposit a single side.
// ---------------------------------------------------------------------------

/// Instruction data layout: discriminator(8) + amount_left(8) + amount_right(8).
pub struct DepositLiquidity;

impl Instruction for DepositLiquidity {
    const DISCRIMINATOR: [u8; 8] = *b"deposit0";

    fn process(
        &self,
        _program_id: &Address,
        accounts: &[AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult {
        if instruction_data.len() < 24 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let amount_left = u64::from_le_bytes(
            instruction_data[8..16]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );
        let amount_right = u64::from_le_bytes(
            instruction_data[16..24]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );
        if amount_left == 0 && amount_right == 0 {
            return Err(ProgramError::InvalidInstructionData);
        }

        // signer          - the liquidity provider (authority over their own legs)
        // left_ata        - program-owned left-token account (authority = vault)
        // user_left_ata   - provider's left-token account
        // right_ata       - program-owned right-token account (authority = vault)
        // user_right_ata  - provider's right-token account
        // token_program   - SPL token program
        let [signer, left_ata, user_left_ata, right_ata, user_right_ata, _token_program] =
            match accounts {
                [signer, left_ata, user_left_ata, right_ata, user_right_ata, token_program] => [
                    signer,
                    left_ata,
                    user_left_ata,
                    right_ata,
                    user_right_ata,
                    token_program,
                ],
                _ => return Err(ProgramError::InvalidAccountData),
            };

        if !signer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        if amount_left > 0 {
            pinocchio_token::instructions::Transfer {
                from: user_left_ata,
                to: left_ata,
                authority: signer,
                amount: amount_left,
            }
            .invoke()?;
        }
        if amount_right > 0 {
            pinocchio_token::instructions::Transfer {
                from: user_right_ata,
                to: right_ata,
                authority: signer,
                amount: amount_right,
            }
            .invoke()?;
        }

        log!("Liquidity deposited");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// WithdrawLiquidity — move left and/or right tokens out of the program-owned
// vault leg accounts into the caller's accounts. The vault PDA is the authority
// over both leg accounts, so it signs the transfers. Only the pool authority
// (recorded at InitPool) may withdraw. Either amount may be zero to withdraw a
// single side.
// ---------------------------------------------------------------------------

/// Instruction data layout: discriminator(8) + amount_left(8) + amount_right(8) + vault_bump(1).
pub struct WithdrawLiquidity;

impl Instruction for WithdrawLiquidity {
    const DISCRIMINATOR: [u8; 8] = *b"withdrw0";

    fn process(
        &self,
        _program_id: &Address,
        accounts: &[AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult {
        if instruction_data.len() < 25 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let amount_left = u64::from_le_bytes(
            instruction_data[8..16]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );
        let amount_right = u64::from_le_bytes(
            instruction_data[16..24]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );
        if amount_left == 0 && amount_right == 0 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let vault_bump = instruction_data[24];

        // signer          - the withdrawer; must equal the pool authority
        // pool            - holds the withdraw authority (read-only)
        // vault           - PDA [b"vault", bump]: the authority over both
        //                   program-owned leg token accounts
        // left_ata        - program-owned left-token account (authority = vault)
        // user_left_ata   - withdrawer's left-token account
        // right_ata       - program-owned right-token account (authority = vault)
        // user_right_ata  - withdrawer's right-token account
        // token_program   - SPL token program
        let [signer, pool, vault, left_ata, user_left_ata, right_ata, user_right_ata, _token_program] =
            match accounts {
                [signer, pool, vault, left_ata, user_left_ata, right_ata, user_right_ata, token_program] => {
                    [
                        signer,
                        pool,
                        vault,
                        left_ata,
                        user_left_ata,
                        right_ata,
                        user_right_ata,
                        token_program,
                    ]
                }
                _ => return Err(ProgramError::InvalidAccountData),
            };

        if !signer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // Only the pool authority may withdraw liquidity.
        let pool_data = pool.try_borrow()?;
        let pool_state = Pool::load(&pool_data)?;
        if pool_state.authority != signer.address().to_bytes() {
            return Err(PammError::Unauthorized.into());
        }
        drop(pool_data);

        let vault_seed_bump = [vault_bump];
        let vault_seeds = [Seed::from(VAULT_SEED), Seed::from(&vault_seed_bump)];

        if amount_left > 0 {
            pinocchio_token::instructions::Transfer {
                from: left_ata,
                to: user_left_ata,
                authority: vault,
                amount: amount_left,
            }
            .invoke_signed(&[Signer::from(&vault_seeds)])?;
        }
        if amount_right > 0 {
            pinocchio_token::instructions::Transfer {
                from: right_ata,
                to: user_right_ata,
                authority: vault,
                amount: amount_right,
            }
            .invoke_signed(&[Signer::from(&vault_seeds)])?;
        }

        log!("Liquidity withdrawn");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SetAuthority — transfer the pool's withdraw authority to a new pubkey. Only
// the current authority (recorded at InitPool) may reassign it.
// ---------------------------------------------------------------------------

/// Instruction data layout: discriminator(8) + new_authority(32).
pub struct SetAuthority;

impl Instruction for SetAuthority {
    const DISCRIMINATOR: [u8; 8] = *b"setauth0";

    fn process(
        &self,
        _program_id: &Address,
        accounts: &[AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult {
        if instruction_data.len() < 40 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let new_authority: [u8; 32] = instruction_data[8..40]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?;

        // signer          - the current pool authority
        // pool            - holds the withdraw authority (writable)
        let [signer, pool] = match accounts {
            [signer, pool] => [signer, pool],
            _ => return Err(ProgramError::InvalidAccountData),
        };

        if !signer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if !pool.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }

        // Only the current pool authority may reassign it.
        let pool_data = pool.try_borrow()?;
        let pool_state = Pool::load(&pool_data)?;
        if pool_state.authority != signer.address().to_bytes() {
            return Err(PammError::Unauthorized.into());
        }
        drop(pool_data);

        // Overwrite the authority field in place at its known offset.
        let mut data = pool.try_borrow_mut()?;
        data.get_mut(Pool::AUTHORITY_OFFSET..Pool::AUTHORITY_OFFSET + 32)
            .ok_or(ProgramError::InvalidAccountData)?
            .copy_from_slice(&new_authority);

        log!("Pool authority updated");
        Ok(())
    }
}

pub struct UpdateOracle;

/// Read-only account that must be present (and match this pubkey) to authorize
/// an oracle update. Its presence lets the caller supply the slot/timestamp the
/// mid was observed at, which are recorded on the pool state.
pub const SLOT_SOURCE: Address =
    Address::from_str_const("BPBFyBVuqnCTHuxQB6GmS8FxFq3JLvep8WfNXXfp1u8X");

/// Batch clock signers we trust to publish honest slot/timestamp readings. We
/// only copy the batch clock's ms time and slot when the `slot_owner` recorded
/// in the batch clock header is one of these keys. The batch clock is an open
/// standard: any trusted block builder can adapt it and, once added here, have
/// their readings honoured by this program.
#[allow(non_upper_case_globals)]
pub const trusted_signers: [Address; 1] = [Address::from_str_const(
    "BAMgx3XPWrXkNUuQiVWUZU6eB2HQZdwz9HNnT4tpo8LG",
)];

/// Maximum permitted age of the oracle mid, in milliseconds. Swaps require the
/// batch clock slot to be current, so age is always judged in wall-clock time.
pub const MAX_QUOTE_AGE_MS: i64 = 100;

impl Instruction for UpdateOracle {
    const DISCRIMINATOR: [u8; 8] = *b"setmid00";

    fn process(
        &self,
        _program_id: &Address,
        accounts: &[AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult {
        // Instruction data layout: discriminator(8) + mid(16) + ... with the
        // update slot (u64 LE) at offset 48 and the update timestamp
        // (i64 LE) at offset 64.
        const MID_OFFSET: usize = 8;
        const MID_END: usize = MID_OFFSET + 16;
        const SLOT_OFFSET: usize = 48;
        const SLOT_END: usize = SLOT_OFFSET + 8;
        const TIMESTAMP_OFFSET: usize = 64;
        const TIMESTAMP_END: usize = TIMESTAMP_OFFSET + 8;

        if instruction_data.len() < TIMESTAMP_END {
            return Err(ProgramError::InvalidInstructionData);
        }

        let [authority, pool, slot_source] = match accounts {
            [authority, pool, slot_source] => [authority, pool, slot_source],
            _ => return Err(ProgramError::InvalidAccountData),
        };

        if !authority.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if !pool.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }
        // Read-only account: assert its pubkey, nothing else is required of it.
        if slot_source.address() != &SLOT_SOURCE {
            return Err(ProgramError::InvalidAccountData);
        }

        let mid_bytes: [u8; 16] = instruction_data[MID_OFFSET..MID_END]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        if u128::from_le_bytes(mid_bytes) == 0 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let slot = u64::from_le_bytes(
            instruction_data[SLOT_OFFSET..SLOT_END]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );
        let timestamp = i64::from_le_bytes(
            instruction_data[TIMESTAMP_OFFSET..TIMESTAMP_END]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );

        // Hot path: the discriminator was written once at InitPool and never
        // changes, so overwrite only the `mid` and last-updated fields in place
        // at their known offsets. This skips the full wincode (de)serialize
        // round-trip to save CU and stay competitive in the scheduler.
        let mut data = pool.try_borrow_mut()?;
        // Read the previous update timestamp before overwriting it so we can
        // report how long it has been since the last successful update.
        let prev_timestamp = i64::from_le_bytes(
            data.get(Pool::TIMESTAMP_OFFSET..Pool::TIMESTAMP_OFFSET + 8)
                .ok_or(ProgramError::InvalidAccountData)?
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?,
        );
        data.get_mut(Pool::MID_OFFSET..Pool::MID_OFFSET + 16)
            .ok_or(ProgramError::InvalidAccountData)?
            .copy_from_slice(&mid_bytes);
        data.get_mut(Pool::SLOT_OFFSET..Pool::SLOT_OFFSET + 8)
            .ok_or(ProgramError::InvalidAccountData)?
            .copy_from_slice(&slot.to_le_bytes());
        data.get_mut(Pool::TIMESTAMP_OFFSET..Pool::TIMESTAMP_OFFSET + 8)
            .ok_or(ProgramError::InvalidAccountData)?
            .copy_from_slice(&timestamp.to_le_bytes());
        drop(data);

        // Only report elapsed time within a single slot: the batch clock slot
        // (carried in the instruction data) must equal the current syscall clock
        // slot. Time measured across slots isn't meaningful here, so skip it.
        if slot == Clock::get()?.slot {
            // Timestamps are Unix nanoseconds. A `prev_timestamp` of 0 means this
            // is the first update since InitPool (no prior reading to measure
            // against).
            if prev_timestamp == 0 {
                log!("Oracle updated (first update)");
            } else {
                let elapsed_ms = timestamp.saturating_sub(prev_timestamp) / 1_000_000;
                log!("Oracle updated ({} ms since last update)", elapsed_ms);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Swap — exchange the left token for the right token (or vice versa) around the
// oracle mid. Both legs are SPL tokens.
// ---------------------------------------------------------------------------

/// Swap direction: pay the left token, receive the right token.
pub const SWAP_SIDE_LEFT_TO_RIGHT: u8 = 0;
/// Swap direction: pay the right token, receive the left token.
pub const SWAP_SIDE_RIGHT_TO_LEFT: u8 = 1;

/// Spread charged on every swap, in basis points. Fixed at 1 bps for now; the
/// pool always keeps the spread, so the taker receives `(1 - spread)` of the
/// mid-priced output.
pub const SPREAD_BPS: u64 = 1;
const BPS_DENOM: u64 = 10_000;

/// Apply the fixed spread to a mid-priced output amount (rounds down, favouring
/// the pool).
fn apply_spread(gross: u64) -> u64 {
    ((gross as u128 * (BPS_DENOM - SPREAD_BPS) as u128) / BPS_DENOM as u128) as u64
}

/// Instruction data layout: discriminator(8) + amount_in(8) + side(1) +
/// vault_bump(1) + min_tokens_out(8).
pub struct Swap;

impl Instruction for Swap {
    const DISCRIMINATOR: [u8; 8] = *b"swap0000";

    fn process(
        &self,
        _program_id: &Address,
        accounts: &[AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult {
        if instruction_data.len() < 26 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let amount_in = u64::from_le_bytes(
            instruction_data[8..16]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );
        if amount_in == 0 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let side = instruction_data[16];
        let vault_bump = instruction_data[17];
        // Slippage floor: the swap output must be at least this many tokens, or
        // the swap is rejected on-chain.
        let min_tokens_out = u64::from_le_bytes(
            instruction_data[18..26]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );

        // signer          - the taker
        // pool            - holds the oracle mid (read-only)
        // vault           - PDA [b"vault", bump]: the authority over both
        //                   program-owned leg token accounts
        // left_ata        - program-owned left-token account (authority = vault)
        // user_left_ata   - taker's left-token account
        // right_ata       - program-owned right-token account (authority = vault)
        // user_right_ata  - taker's right-token account
        // token_program   - SPL token program
        // batch_clock     - the SLOT_SOURCE account; supplies the current slot
        //                   and timestamp used for the staleness check (read-only)
        let [signer, pool, vault, left_ata, user_left_ata, right_ata, user_right_ata, _token_program, batch_clock] =
            match accounts {
                [signer, pool, vault, left_ata, user_left_ata, right_ata, user_right_ata, token_program, batch_clock] => {
                    [
                        signer,
                        pool,
                        vault,
                        left_ata,
                        user_left_ata,
                        right_ata,
                        user_right_ata,
                        token_program,
                        batch_clock,
                    ]
                }
                _ => return Err(ProgramError::InvalidAccountData),
            };

        if !signer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if batch_clock.address() != &SLOT_SOURCE {
            return Err(ProgramError::InvalidAccountData);
        }

        // Read the oracle mid-price via wincode (validates the discriminator),
        // along with the slot/timestamp of its last update.
        let pool_data = pool.try_borrow()?;
        let pool_state = Pool::load(&pool_data)?;
        let mid = pool_state.mid;
        let last_updated_timestamp = pool_state.last_updated_timestamp;
        drop(pool_data);
        if mid == 0 {
            return Err(ProgramError::InvalidAccountData);
        }

        // Staleness gate: swaps are only permitted when the batch clock slot is
        // current (equal to the syscall clock slot) and the oracle mid is no
        // older than 200 ms.
        //
        // Requiring the batch clock to be current gives us a trustworthy
        // wall-clock reading, so we judge the mid's age in milliseconds
        // (timestamps are Unix nanoseconds). If the batch clock lags the
        // syscall clock we reject the swap outright.
        let current_slot = Clock::get()?.slot;
        let batch_data = batch_clock.try_borrow()?;
        // Deserialize the batch clock with wincode (validates the discriminator)
        // rather than reading fields by byte offset.
        let batch = BatchClock::load(&batch_data)?;
        drop(batch_data);

        // Only copy the batch clock's ms time and slot when its `slot_owner` is
        // one of the `trusted_signers`. This is an open standard: any trusted
        // block builder can adapt this batch clock and, once their key is in
        // `trusted_signers`, have their readings honoured here.
        if !trusted_signers
            .iter()
            .any(|s| s.as_ref() == batch.slot_owner)
        {
            return Err(PammError::BlockBuilderNotTrusted.into());
        }

        let batch_slot = batch.slot;
        let batch_timestamp = batch.timestamp_ns;
        let batch_sequence = batch.sequence;

        if current_slot != batch_slot {
            return Err(PammError::StaleQuoteSlots.into());
        }
        // A batch clock `sequence` of 0 on the current slot marks the slot's
        // first tick: the reading was taken at the very start of the slot, so
        // the oracle update age is exactly 0 ms. (The slot equality required
        // above already guarantees the batch slot matches the syscall clock
        // slot.) Skip the millisecond diff and treat the quote as fresh.
        let elapsed_ms = if batch_sequence == 0 {
            0
        } else {
            batch_timestamp.saturating_sub(last_updated_timestamp) / 1_000_000
        };
        if elapsed_ms > MAX_QUOTE_AGE_MS {
            log!(
                "StaleQuoteMillis: batch_timestamp {} ns, last_updated_timestamp {} ns, elapsed {} ms > {} ms",
                batch_timestamp,
                last_updated_timestamp,
                elapsed_ms,
                MAX_QUOTE_AGE_MS
            );
            return Err(PammError::StaleQuoteMillis.into());
        }

        let vault_seed_bump = [vault_bump];
        let vault_seeds = [Seed::from(VAULT_SEED), Seed::from(&vault_seed_bump)];
        let vault_signer = Signer::from(&vault_seeds);

        match side {
            SWAP_SIDE_LEFT_TO_RIGHT => {
                // Taker sends `amount_in` left tokens, receives right tokens
                // priced at mid less the spread.
                let right_out = apply_spread(q64_mul(amount_in, mid)?);
                if right_out == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if right_out < min_tokens_out {
                    return Err(PammError::SlippageExceeded.into());
                }

                // Taker pays the left leg (they sign for their own account).
                pinocchio_token::instructions::Transfer {
                    from: user_left_ata,
                    to: left_ata,
                    authority: signer,
                    amount: amount_in,
                }
                .invoke()?;

                // Pool pays out the right leg (vault signs as the ATA authority).
                pinocchio_token::instructions::Transfer {
                    from: right_ata,
                    to: user_right_ata,
                    authority: vault,
                    amount: right_out,
                }
                .invoke_signed(&[vault_signer])?;

                log!("Swapped left -> right (quote age {} ms)", elapsed_ms);
            }
            SWAP_SIDE_RIGHT_TO_LEFT => {
                // Taker sends `amount_in` right tokens, receives left tokens
                // priced at mid less the spread.
                let left_out = apply_spread(q64_div(amount_in, mid)?);
                if left_out == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if left_out < min_tokens_out {
                    return Err(PammError::SlippageExceeded.into());
                }

                // Taker pays the right leg (they sign for their own account).
                pinocchio_token::instructions::Transfer {
                    from: user_right_ata,
                    to: right_ata,
                    authority: signer,
                    amount: amount_in,
                }
                .invoke()?;

                // Pool pays out the left leg (vault signs as the ATA authority).
                pinocchio_token::instructions::Transfer {
                    from: left_ata,
                    to: user_left_ata,
                    authority: vault,
                    amount: left_out,
                }
                .invoke_signed(&[vault_signer])?;

                log!("Swapped right -> left (quote age {} ms)", elapsed_ms);
            }
            _ => return Err(ProgramError::InvalidInstructionData),
        }

        Ok(())
    }
}
