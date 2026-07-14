use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

/// The system program id is the all-zero pubkey.
fn system_program_id() -> Pubkey {
    Pubkey::default()
}

/// Program ID for the jitosol-pamm program.
/// Replace with the actual deployed program ID.
pub const PROGRAM_ID: &str = "11111111111111111111111111111111";

/// Instruction discriminators.
pub mod discriminators {
    pub const INIT_POOL: [u8; 8] = *b"initpool";
    pub const DEPOSIT_LIQUIDITY: [u8; 8] = *b"deposit0";
    pub const WITHDRAW_LIQUIDITY: [u8; 8] = *b"withdrw0";
    pub const UPDATE_ORACLE: [u8; 8] = *b"setmid00";
    pub const SWAP: [u8; 8] = *b"swap0000";
    pub const SET_AUTHORITY: [u8; 8] = *b"setauth0";
}

/// The account whose pubkey authorizes oracle updates and supplies the batch
/// clock (slot + timestamp) used for the swap staleness check.
pub const SLOT_SOURCE: &str = "BPBFyBVuqnCTHuxQB6GmS8FxFq3JLvep8WfNXXfp1u8X";

/// Swap direction: pay the left token, receive the right token.
pub const SWAP_SIDE_LEFT_TO_RIGHT: u8 = 0;
/// Swap direction: pay the right token, receive the left token.
pub const SWAP_SIDE_RIGHT_TO_LEFT: u8 = 1;

/// Derive the pool state PDA (holds the oracle mid).
pub fn derive_pool_pda(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"pool"], program_id)
}

/// Derive the vault PDA. The vault holds no inventory itself; it is the common
/// authority over both program-owned leg token accounts.
pub fn derive_vault_pda(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"vault"], program_id)
}

/// Derive the program-owned left-token account PDA.
pub fn derive_left_ata_pda(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"leftata"], program_id)
}

/// Derive the program-owned right-token account PDA.
pub fn derive_right_ata_pda(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"rightata"], program_id)
}

/// Initialize the pool: creates the pool state account, the vault
/// authority, and the two program-owned leg token accounts. Fully
/// self-contained — no accounts need to exist beforehand except the payer and
/// the two mints. Both legs are arbitrary SPL mints.
pub fn init_pool(
    program_id: &Pubkey,
    payer: &Pubkey,
    mint_left: &Pubkey,
    mint_right: &Pubkey,
    token_program_id: &Pubkey,
) -> Instruction {
    let (pool, pool_bump) = derive_pool_pda(program_id);
    let (vault, vault_bump) = derive_vault_pda(program_id);
    let (left_ata, left_ata_bump) = derive_left_ata_pda(program_id);
    let (right_ata, right_ata_bump) = derive_right_ata_pda(program_id);

    let mut data = Vec::new();
    data.extend_from_slice(&discriminators::INIT_POOL);
    data.push(pool_bump);
    data.push(vault_bump);
    data.push(left_ata_bump);
    data.push(right_ata_bump);

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(*mint_left, false),
            AccountMeta::new(left_ata, false),
            AccountMeta::new_readonly(*mint_right, false),
            AccountMeta::new(right_ata, false),
            AccountMeta::new_readonly(*token_program_id, false),
            AccountMeta::new_readonly(system_program_id(), false),
        ],
        data,
    }
}

/// Deposit left and/or right tokens from the provider's accounts into the
/// program-owned vault leg accounts. The provider signs for their own token
/// accounts, so no vault signature is required. Either amount may be zero to
/// deposit a single side, but not both.
///
/// Both `user_left_ata` and `user_right_ata` must always be supplied even when
/// the corresponding amount is zero — the program's account list is fixed.
pub fn deposit_liquidity(
    program_id: &Pubkey,
    signer: &Pubkey,
    left_ata: &Pubkey,
    user_left_ata: &Pubkey,
    right_ata: &Pubkey,
    user_right_ata: &Pubkey,
    token_program_id: &Pubkey,
    amount_left: u64,
    amount_right: u64,
) -> Instruction {
    let mut data = Vec::new();
    data.extend_from_slice(&discriminators::DEPOSIT_LIQUIDITY);
    data.extend_from_slice(&amount_left.to_le_bytes());
    data.extend_from_slice(&amount_right.to_le_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new(*left_ata, false),
            AccountMeta::new(*user_left_ata, false),
            AccountMeta::new(*right_ata, false),
            AccountMeta::new(*user_right_ata, false),
            AccountMeta::new_readonly(*token_program_id, false),
        ],
        data,
    }
}

/// Withdraw left and/or right tokens out of the program-owned vault leg accounts
/// into the withdrawer's accounts. The vault PDA is the authority over both leg
/// accounts, so it signs the transfers. Only the pool authority (recorded at
/// [`init_pool`]) may withdraw. Either amount may be zero to withdraw a single
/// side, but not both.
///
/// Both `user_left_ata` and `user_right_ata` must always be supplied even when
/// the corresponding amount is zero — the program's account list is fixed.
pub fn withdraw_liquidity(
    program_id: &Pubkey,
    signer: &Pubkey,
    pool: &Pubkey,
    left_ata: &Pubkey,
    user_left_ata: &Pubkey,
    right_ata: &Pubkey,
    user_right_ata: &Pubkey,
    token_program_id: &Pubkey,
    amount_left: u64,
    amount_right: u64,
) -> Instruction {
    let (vault, vault_bump) = derive_vault_pda(program_id);

    let mut data = Vec::new();
    data.extend_from_slice(&discriminators::WITHDRAW_LIQUIDITY);
    data.extend_from_slice(&amount_left.to_le_bytes());
    data.extend_from_slice(&amount_right.to_le_bytes());
    data.push(vault_bump);

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new_readonly(*pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(*left_ata, false),
            AccountMeta::new(*user_left_ata, false),
            AccountMeta::new(*right_ata, false),
            AccountMeta::new(*user_right_ata, false),
            AccountMeta::new_readonly(*token_program_id, false),
        ],
        data,
    }
}

/// Transfer the pool's withdraw authority to `new_authority`. Only the current
/// authority (recorded at [`init_pool`]) may reassign it, so `signer` must be
/// the current authority.
pub fn set_authority(
    program_id: &Pubkey,
    signer: &Pubkey,
    pool: &Pubkey,
    new_authority: &Pubkey,
) -> Instruction {
    let mut data = Vec::new();
    data.extend_from_slice(&discriminators::SET_AUTHORITY);
    data.extend_from_slice(&new_authority.to_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new(*pool, false),
        ],
        data,
    }
}

/// Update the oracle mid-price (Q64.64) stored in the pool account, recording
/// the `slot` and `timestamp` (Unix nanoseconds) the mid was observed at.
///
/// The instruction data is zero-padded so the program can read the mid at
/// offset 8, the slot (u64 LE) at offset 48, and the timestamp (i64 LE) at
/// offset 64. `slot_source` must be the [`SLOT_SOURCE`] account; the program
/// asserts its pubkey to authorize the update.
pub fn update_oracle(
    program_id: &Pubkey,
    authority: &Pubkey,
    pool: &Pubkey,
    slot_source: &Pubkey,
    mid: u128,
    slot: u64,
    timestamp: i64,
) -> Instruction {
    // Layout the program reads: discriminator(8) + mid(16) at offset 8, slot
    // (u64 LE) at offset 48, timestamp (i64 LE) at offset 64. Total 72 bytes.
    let mut data = vec![0u8; 72];
    data[0..8].copy_from_slice(&discriminators::UPDATE_ORACLE);
    data[8..24].copy_from_slice(&mid.to_le_bytes());
    data[48..56].copy_from_slice(&slot.to_le_bytes());
    data[64..72].copy_from_slice(&timestamp.to_le_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*authority, true),
            AccountMeta::new(*pool, false),
            AccountMeta::new_readonly(*slot_source, false),
        ],
        data,
    }
}

/// Swap the left token <-> the right token around the oracle mid, less a fixed
/// spread. Both legs are SPL tokens.
///
/// `amount_in` is denominated in the token being paid in (left-token units for
/// `SWAP_SIDE_LEFT_TO_RIGHT`, right-token units for `SWAP_SIDE_RIGHT_TO_LEFT`).
///
/// `batch_clock` must be the [`SLOT_SOURCE`] account; it supplies the slot and
/// timestamp used for the staleness check.
///
/// `min_tokens_out` is the slippage floor: the swap is rejected unless it
/// delivers at least this many output tokens (in the received token's base
/// units). Pass `0` to disable the check.
pub fn swap(
    program_id: &Pubkey,
    signer: &Pubkey,
    pool: &Pubkey,
    left_ata: &Pubkey,
    user_left_ata: &Pubkey,
    right_ata: &Pubkey,
    user_right_ata: &Pubkey,
    token_program_id: &Pubkey,
    batch_clock: &Pubkey,
    amount_in: u64,
    side: u8,
    min_tokens_out: u64,
) -> Instruction {
    let (vault, vault_bump) = derive_vault_pda(program_id);

    let mut data = Vec::new();
    data.extend_from_slice(&discriminators::SWAP);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.push(side);
    data.push(vault_bump);
    data.extend_from_slice(&min_tokens_out.to_le_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new_readonly(*pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(*left_ata, false),
            AccountMeta::new(*user_left_ata, false),
            AccountMeta::new(*right_ata, false),
            AccountMeta::new(*user_right_ata, false),
            AccountMeta::new_readonly(*token_program_id, false),
            AccountMeta::new_readonly(*batch_clock, false),
        ],
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_pool() {
        let program_id = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let mint_left = Pubkey::new_unique();
        let mint_right = Pubkey::new_unique();
        let token_program_id = Pubkey::new_unique();

        let ix = init_pool(
            &program_id,
            &payer,
            &mint_left,
            &mint_right,
            &token_program_id,
        );
        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 9);
        assert_eq!(ix.data[0..8], discriminators::INIT_POOL);
    }

    #[test]
    fn test_deposit_liquidity() {
        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();
        let token_program_id = Pubkey::new_unique();

        let ix = deposit_liquidity(
            &program_id,
            &signer,
            &left_ata,
            &user_left_ata,
            &right_ata,
            &user_right_ata,
            &token_program_id,
            1_000,
            2_000,
        );
        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 6);
        assert_eq!(ix.data.len(), 24);
        assert_eq!(ix.data[0..8], discriminators::DEPOSIT_LIQUIDITY);
        assert_eq!(ix.data[8..16], 1_000u64.to_le_bytes());
        assert_eq!(ix.data[16..24], 2_000u64.to_le_bytes());
    }

    #[test]
    fn test_withdraw_liquidity() {
        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();
        let token_program_id = Pubkey::new_unique();

        let ix = withdraw_liquidity(
            &program_id,
            &signer,
            &pool,
            &left_ata,
            &user_left_ata,
            &right_ata,
            &user_right_ata,
            &token_program_id,
            1_000,
            2_000,
        );
        let (_, vault_bump) = derive_vault_pda(&program_id);
        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 8);
        assert_eq!(ix.data.len(), 25);
        assert_eq!(ix.data[0..8], discriminators::WITHDRAW_LIQUIDITY);
        assert_eq!(ix.data[8..16], 1_000u64.to_le_bytes());
        assert_eq!(ix.data[16..24], 2_000u64.to_le_bytes());
        assert_eq!(ix.data[24], vault_bump);
    }

    #[test]
    fn test_update_oracle() {
        let program_id = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let slot_source = Pubkey::new_unique();

        let mid = 1u128 << 64;
        let ix = update_oracle(&program_id, &authority, &pool, &slot_source, mid, 123, 456);
        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.data.len(), 72);
        assert_eq!(ix.data[0..8], discriminators::UPDATE_ORACLE);
        assert_eq!(ix.data[8..24], mid.to_le_bytes());
        assert_eq!(ix.data[48..56], 123u64.to_le_bytes());
        assert_eq!(ix.data[64..72], 456i64.to_le_bytes());
    }

    #[test]
    fn test_swap() {
        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();
        let token_program_id = Pubkey::new_unique();
        let batch_clock = Pubkey::new_unique();

        let ix = swap(
            &program_id,
            &signer,
            &pool,
            &left_ata,
            &user_left_ata,
            &right_ata,
            &user_right_ata,
            &token_program_id,
            &batch_clock,
            1_000_000,
            SWAP_SIDE_LEFT_TO_RIGHT,
            990_000,
        );
        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 9);
        assert_eq!(ix.data.len(), 26);
        assert_eq!(ix.data[0..8], discriminators::SWAP);
        assert_eq!(ix.data[18..26], 990_000u64.to_le_bytes());
    }
}
