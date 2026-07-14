pub mod error;
pub mod instructions;
pub mod state;

use crate::instructions::{
    DepositLiquidity, InitPool, Instruction, SetAuthority, Swap, UpdateOracle, WithdrawLiquidity,
};
use pinocchio::{entrypoint, AccountView, Address, ProgramResult};
use solana_program_error::ProgramError;
use solana_program_log::log;

entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Address,
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    if InitPool::invoked(instruction_data) {
        return InitPool.process(program_id, accounts, instruction_data);
    }
    if UpdateOracle::invoked(instruction_data) {
        return UpdateOracle.process(program_id, accounts, instruction_data);
    }
    if Swap::invoked(instruction_data) {
        return Swap.process(program_id, accounts, instruction_data);
    }
    if DepositLiquidity::invoked(instruction_data) {
        return DepositLiquidity.process(program_id, accounts, instruction_data);
    }
    if WithdrawLiquidity::invoked(instruction_data) {
        return WithdrawLiquidity.process(program_id, accounts, instruction_data);
    }
    if SetAuthority::invoked(instruction_data) {
        return SetAuthority.process(program_id, accounts, instruction_data);
    }
    log!("Discriminator not found");
    Err(ProgramError::InvalidInstructionData)
}

#[cfg(test)]
mod test {
    use crate::instructions::{Instruction as _, Swap, UpdateOracle, SPREAD_BPS};
    use crate::state::{Pool, OnchainAccount};
    use mollusk_svm::{result::Check, Mollusk};
    use solana_sdk::{
        account::Account,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    };

    const ELF: &str = "target/sbpf-solana-solana/release/jito_bamm";
    const ONE_Q64_64: u128 = 1u128 << 64;

    fn mollusk(program_id: &Pubkey) -> Mollusk {
        let mut m = Mollusk::new(program_id, ELF);
        m.add_program(&pinocchio_token::id(), "elf/Token");
        m
    }

    // Minimal SPL token account (165 bytes).
    fn token_account_data(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
        let mut data = vec![0u8; 165];
        data[0..32].copy_from_slice(&mint.to_bytes());
        data[32..64].copy_from_slice(&owner.to_bytes());
        data[64..72].copy_from_slice(&amount.to_le_bytes());
        data[108] = 1; // state = Initialized
        data
    }

    fn pool_account(program_id: &Pubkey, mid: u128) -> Account {
        pool_account_at(program_id, mid, 0, 0, [0u8; 32])
    }

    // Pool account with explicit last-updated slot/timestamp (ns) and authority.
    fn pool_account_at(
        program_id: &Pubkey,
        mid: u128,
        last_updated_slot: u64,
        last_updated_timestamp: i64,
        authority: [u8; 32],
    ) -> Account {
        let mut data = vec![0u8; Pool::SIZE];
        let pool = Pool {
            discriminator: Pool::DISCRIMINATOR,
            mid,
            last_updated_slot,
            last_updated_timestamp,
            authority,
        };
        pool.store(&mut data).unwrap();
        Account {
            lamports: 1_000_000,
            data,
            owner: *program_id,
            ..Account::default()
        }
    }

    // The batch clock account: slot (u64 LE) at offset 48, timestamp in Unix
    // nanoseconds (i64 LE) at offset 64.
    fn batch_clock_account(slot: u64, timestamp_nanos: i64) -> (Pubkey, Account) {
        use crate::instructions::SLOT_SOURCE;
        let mut data = vec![0u8; 72];
        data[48..56].copy_from_slice(&slot.to_le_bytes());
        data[64..72].copy_from_slice(&timestamp_nanos.to_le_bytes());
        (
            Pubkey::new_from_array(SLOT_SOURCE.to_bytes()),
            Account {
                lamports: 1_000_000,
                data,
                ..Account::default()
            },
        )
    }

    #[test]
    fn init_pool_creates_accounts() {
        use crate::instructions::InitPool;

        let program_id = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let (pool, pool_bump) = Pubkey::find_program_address(&[b"pool"], &program_id);
        let (vault, vault_bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
        let (left_ata, left_ata_bump) = Pubkey::find_program_address(&[b"leftata"], &program_id);
        let (right_ata, right_ata_bump) =
            Pubkey::find_program_address(&[b"rightata"], &program_id);
        let mint_left = Pubkey::new_unique();
        let mint_right = Pubkey::new_unique();

        // minimal initialized SPL mint
        let mint_data = |mint: &Pubkey| {
            let mut d = vec![0u8; 82];
            d[0..4].copy_from_slice(&1u32.to_le_bytes());
            d[4..36].copy_from_slice(&mint.to_bytes());
            d[44] = 9;
            d[45] = 1;
            d
        };

        let mut data = Vec::new();
        data.extend_from_slice(&InitPool::DISCRIMINATOR);
        data.push(pool_bump);
        data.push(vault_bump);
        data.push(left_ata_bump);
        data.push(right_ata_bump);

        let ix = Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(mint_left, false),
                AccountMeta::new(left_ata, false),
                AccountMeta::new_readonly(mint_right, false),
                AccountMeta::new(right_ata, false),
                AccountMeta::new_readonly(pinocchio_token::id(), false),
                AccountMeta::new_readonly(solana_sdk::pubkey::Pubkey::default(), false),
            ],
        );

        let accounts = vec![
            (
                payer,
                Account {
                    lamports: 1_000_000_000,
                    owner: solana_sdk::pubkey::Pubkey::default(),
                    ..Account::default()
                },
            ),
            (pool, Account::default()),
            (vault, Account::default()),
            (
                mint_left,
                Account {
                    lamports: 1_000_000,
                    data: mint_data(&mint_left),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (left_ata, Account::default()),
            (
                mint_right,
                Account {
                    lamports: 1_000_000,
                    data: mint_data(&mint_right),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (right_ata, Account::default()),
            mollusk_svm::program::keyed_account_for_system_program(),
            (
                pinocchio_token::id(),
                mollusk_svm::program::create_program_account_loader_v3(&pinocchio_token::id()),
            ),
        ];

        mollusk(&program_id).process_and_validate_instruction(
            &ix,
            &accounts,
            &[
                Check::success(),
                // pool account is program-owned with the right discriminator
                Check::account(&pool)
                    .owner(&program_id)
                    .data_slice(0, &Pool::DISCRIMINATOR)
                    .build(),
                // left_ata is a token account (mint at offset 0, authority = vault at offset 32)
                Check::account(&left_ata)
                    .owner(&pinocchio_token::id())
                    .data_slice(0, &mint_left.to_bytes())
                    .data_slice(32, &vault.to_bytes())
                    .build(),
                // right_ata is a token account (mint at offset 0, authority = vault at offset 32)
                Check::account(&right_ata)
                    .owner(&pinocchio_token::id())
                    .data_slice(0, &mint_right.to_bytes())
                    .data_slice(32, &vault.to_bytes())
                    .build(),
            ],
        );
    }

    #[test]
    fn update_oracle_writes_mid() {
        use crate::instructions::SLOT_SOURCE;

        let program_id = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let slot_source = Pubkey::new_from_array(SLOT_SOURCE.to_bytes());
        let mid = 2u128 << 64;
        let slot = 123_456u64;
        let timestamp = 1_700_000_000i64;

        // Instruction data: discriminator(8) + mid(16), zero-padded so the slot
        // (u64 LE) lands at offset 48 and the timestamp (i64 LE) at offset 64.
        let mut data = vec![0u8; 72];
        data[0..8].copy_from_slice(&UpdateOracle::DISCRIMINATOR);
        data[8..24].copy_from_slice(&mid.to_le_bytes());
        data[48..56].copy_from_slice(&slot.to_le_bytes());
        data[64..72].copy_from_slice(&timestamp.to_le_bytes());

        let ix = Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
                AccountMeta::new(authority, true),
                AccountMeta::new(pool, false),
                AccountMeta::new_readonly(slot_source, false),
            ],
        );

        let accounts = vec![
            (
                authority,
                Account {
                    lamports: 1_000_000_000,
                    owner: solana_sdk::pubkey::Pubkey::default(),
                    ..Account::default()
                },
            ),
            (pool, pool_account(&program_id, 0)),
            (slot_source, Account::default()),
        ];

        mollusk(&program_id).process_and_validate_instruction(
            &ix,
            &accounts,
            &[
                Check::success(),
                Check::account(&pool)
                    .data_slice(Pool::MID_OFFSET, &mid.to_le_bytes())
                    .data_slice(Pool::SLOT_OFFSET, &slot.to_le_bytes())
                    .data_slice(Pool::TIMESTAMP_OFFSET, &timestamp.to_le_bytes())
                    .build(),
            ],
        );
    }

    #[test]
    fn swap_left_to_right_applies_spread() {
        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let (vault, vault_bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
        let mint_left = Pubkey::new_unique();
        let mint_right = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();

        // mid = 1.0 (Q64.64): 1 left unit -> 1 right unit before spread.
        let mid = ONE_Q64_64;
        let amount_in = 1_000_000u64;
        let expected_out = amount_in - (amount_in * SPREAD_BPS) / 10_000;
        let right_inventory = 10_000_000u64;

        let mut data = Vec::new();
        data.extend_from_slice(&Swap::DISCRIMINATOR);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.push(0); // side: left -> right
        data.push(vault_bump);
        data.extend_from_slice(&0u64.to_le_bytes()); // min_tokens_out (no floor)

        let (batch_clock, batch_clock_acc) = batch_clock_account(0, 0);

        let ix = Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
                AccountMeta::new(signer, true),
                AccountMeta::new_readonly(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(left_ata, false),
                AccountMeta::new(user_left_ata, false),
                AccountMeta::new(right_ata, false),
                AccountMeta::new(user_right_ata, false),
                AccountMeta::new_readonly(pinocchio_token::id(), false),
                AccountMeta::new_readonly(batch_clock, false),
            ],
        );

        let accounts = vec![
            (
                signer,
                Account {
                    lamports: 100_000_000,
                    owner: solana_sdk::pubkey::Pubkey::default(),
                    ..Account::default()
                },
            ),
            (pool, pool_account(&program_id, mid)),
            (
                vault,
                Account {
                    lamports: 1_000_000,
                    owner: solana_sdk::pubkey::Pubkey::default(),
                    ..Account::default()
                },
            ),
            (
                left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &vault, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &signer, amount_in),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &vault, right_inventory),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &signer, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                pinocchio_token::id(),
                mollusk_svm::program::create_program_account_loader_v3(&pinocchio_token::id()),
            ),
            (batch_clock, batch_clock_acc),
        ];

        mollusk(&program_id).process_and_validate_instruction(
            &ix,
            &accounts,
            &[
                Check::success(),
                // pool received the paid-in left tokens
                Check::account(&left_ata)
                    .data_slice(64, &amount_in.to_le_bytes())
                    .build(),
                // taker received the right tokens, less the spread
                Check::account(&user_right_ata)
                    .data_slice(64, &expected_out.to_le_bytes())
                    .build(),
            ],
        );
    }

    #[test]
    fn swap_right_to_left_applies_spread() {
        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let (vault, vault_bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
        let mint_left = Pubkey::new_unique();
        let mint_right = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();

        let mid = ONE_Q64_64; // 1 right unit -> 1 left unit before spread.
        let amount_in = 1_000_000u64;
        let expected_out = amount_in - (amount_in * SPREAD_BPS) / 10_000;
        let left_inventory = 10_000_000u64;

        let mut data = Vec::new();
        data.extend_from_slice(&Swap::DISCRIMINATOR);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.push(1); // side: right -> left
        data.push(vault_bump);
        data.extend_from_slice(&0u64.to_le_bytes()); // min_tokens_out (no floor)

        let (batch_clock, batch_clock_acc) = batch_clock_account(0, 0);

        let ix = Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
                AccountMeta::new(signer, true),
                AccountMeta::new_readonly(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(left_ata, false),
                AccountMeta::new(user_left_ata, false),
                AccountMeta::new(right_ata, false),
                AccountMeta::new(user_right_ata, false),
                AccountMeta::new_readonly(pinocchio_token::id(), false),
                AccountMeta::new_readonly(batch_clock, false),
            ],
        );

        let accounts = vec![
            (
                signer,
                Account {
                    lamports: 1_000_000,
                    owner: solana_sdk::pubkey::Pubkey::default(),
                    ..Account::default()
                },
            ),
            (pool, pool_account(&program_id, mid)),
            (
                vault,
                Account {
                    lamports: 1_000_000,
                    owner: solana_sdk::pubkey::Pubkey::default(),
                    ..Account::default()
                },
            ),
            (
                left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &vault, left_inventory),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &signer, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &vault, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &signer, amount_in),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                pinocchio_token::id(),
                mollusk_svm::program::create_program_account_loader_v3(&pinocchio_token::id()),
            ),
            (batch_clock, batch_clock_acc),
        ];

        mollusk(&program_id).process_and_validate_instruction(
            &ix,
            &accounts,
            &[
                Check::success(),
                // taker received the left tokens, less the spread
                Check::account(&user_left_ata)
                    .data_slice(64, &expected_out.to_le_bytes())
                    .build(),
                // pool received the paid-in right tokens
                Check::account(&right_ata)
                    .data_slice(64, &amount_in.to_le_bytes())
                    .build(),
            ],
        );
    }

    // Build a SOL -> jitoSOL swap with configurable pool last-update and batch
    // clock values, for exercising the staleness gate.
    fn stale_swap_fixture(
        pool_slot: u64,
        pool_ts: i64,
        batch_slot: u64,
        batch_ts: i64,
        min_tokens_out: u64,
    ) -> (Pubkey, Instruction, Vec<(Pubkey, Account)>) {
        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let (vault, vault_bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
        let mint_left = Pubkey::new_unique();
        let mint_right = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();

        let mid = ONE_Q64_64;
        let amount_in = 1_000_000u64;

        let mut data = Vec::new();
        data.extend_from_slice(&Swap::DISCRIMINATOR);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.push(0); // side: left -> right
        data.push(vault_bump);
        data.extend_from_slice(&min_tokens_out.to_le_bytes());

        let (batch_clock, batch_clock_acc) = batch_clock_account(batch_slot, batch_ts);

        let ix = Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
                AccountMeta::new(signer, true),
                AccountMeta::new_readonly(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(left_ata, false),
                AccountMeta::new(user_left_ata, false),
                AccountMeta::new(right_ata, false),
                AccountMeta::new(user_right_ata, false),
                AccountMeta::new_readonly(pinocchio_token::id(), false),
                AccountMeta::new_readonly(batch_clock, false),
            ],
        );

        let accounts = vec![
            (
                signer,
                Account {
                    lamports: 100_000_000,
                    owner: Pubkey::default(),
                    ..Account::default()
                },
            ),
            (
                pool,
                pool_account_at(&program_id, mid, pool_slot, pool_ts, [0u8; 32]),
            ),
            (
                vault,
                Account {
                    lamports: 1_000_000,
                    owner: Pubkey::default(),
                    ..Account::default()
                },
            ),
            (
                left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &vault, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &signer, amount_in),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &vault, 10_000_000),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &signer, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                pinocchio_token::id(),
                mollusk_svm::program::create_program_account_loader_v3(&pinocchio_token::id()),
            ),
            (batch_clock, batch_clock_acc),
        ];

        (program_id, ix, accounts)
    }

    #[test]
    fn deposit_moves_both_legs_into_vault() {
        use crate::instructions::DepositLiquidity;

        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let (vault, _vault_bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
        let mint_left = Pubkey::new_unique();
        let mint_right = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();

        let amount_left = 5_000_000u64;
        let amount_right = 3_000_000u64;

        let mut data = Vec::new();
        data.extend_from_slice(&DepositLiquidity::DISCRIMINATOR);
        data.extend_from_slice(&amount_left.to_le_bytes());
        data.extend_from_slice(&amount_right.to_le_bytes());

        let ix = Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
                AccountMeta::new(signer, true),
                AccountMeta::new(left_ata, false),
                AccountMeta::new(user_left_ata, false),
                AccountMeta::new(right_ata, false),
                AccountMeta::new(user_right_ata, false),
                AccountMeta::new_readonly(pinocchio_token::id(), false),
            ],
        );

        let accounts = vec![
            (
                signer,
                Account {
                    lamports: 100_000_000,
                    owner: Pubkey::default(),
                    ..Account::default()
                },
            ),
            (
                left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &vault, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &signer, amount_left),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &vault, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &signer, amount_right),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                pinocchio_token::id(),
                mollusk_svm::program::create_program_account_loader_v3(&pinocchio_token::id()),
            ),
        ];

        mollusk(&program_id).process_and_validate_instruction(
            &ix,
            &accounts,
            &[
                Check::success(),
                Check::account(&left_ata)
                    .data_slice(64, &amount_left.to_le_bytes())
                    .build(),
                Check::account(&right_ata)
                    .data_slice(64, &amount_right.to_le_bytes())
                    .build(),
            ],
        );
    }

    #[test]
    fn withdraw_moves_both_legs_out_of_vault() {
        use crate::instructions::WithdrawLiquidity;

        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let (vault, vault_bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
        let mint_left = Pubkey::new_unique();
        let mint_right = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();

        let left_inventory = 10_000_000u64;
        let right_inventory = 8_000_000u64;
        let amount_left = 4_000_000u64;
        let amount_right = 1_000_000u64;

        let mut data = Vec::new();
        data.extend_from_slice(&WithdrawLiquidity::DISCRIMINATOR);
        data.extend_from_slice(&amount_left.to_le_bytes());
        data.extend_from_slice(&amount_right.to_le_bytes());
        data.push(vault_bump);

        let ix = Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
                AccountMeta::new(signer, true),
                AccountMeta::new_readonly(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(left_ata, false),
                AccountMeta::new(user_left_ata, false),
                AccountMeta::new(right_ata, false),
                AccountMeta::new(user_right_ata, false),
                AccountMeta::new_readonly(pinocchio_token::id(), false),
            ],
        );

        let accounts = vec![
            (
                signer,
                Account {
                    lamports: 100_000_000,
                    owner: Pubkey::default(),
                    ..Account::default()
                },
            ),
            // pool with the withdraw authority set to the signer
            (pool, pool_account_at(&program_id, 0, 0, 0, signer.to_bytes())),
            (
                vault,
                Account {
                    lamports: 1_000_000,
                    owner: Pubkey::default(),
                    ..Account::default()
                },
            ),
            (
                left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &vault, left_inventory),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &signer, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &vault, right_inventory),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &signer, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                pinocchio_token::id(),
                mollusk_svm::program::create_program_account_loader_v3(&pinocchio_token::id()),
            ),
        ];

        mollusk(&program_id).process_and_validate_instruction(
            &ix,
            &accounts,
            &[
                Check::success(),
                // vault legs debited
                Check::account(&left_ata)
                    .data_slice(64, &(left_inventory - amount_left).to_le_bytes())
                    .build(),
                Check::account(&right_ata)
                    .data_slice(64, &(right_inventory - amount_right).to_le_bytes())
                    .build(),
                // withdrawer credited
                Check::account(&user_left_ata)
                    .data_slice(64, &amount_left.to_le_bytes())
                    .build(),
                Check::account(&user_right_ata)
                    .data_slice(64, &amount_right.to_le_bytes())
                    .build(),
            ],
        );
    }

    #[test]
    fn withdraw_fails_for_non_authority() {
        use crate::error::PammError;
        use crate::instructions::WithdrawLiquidity;

        let program_id = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let authority = Pubkey::new_unique(); // pool authority != signer
        let pool = Pubkey::new_unique();
        let (vault, vault_bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
        let mint_left = Pubkey::new_unique();
        let mint_right = Pubkey::new_unique();
        let left_ata = Pubkey::new_unique();
        let user_left_ata = Pubkey::new_unique();
        let right_ata = Pubkey::new_unique();
        let user_right_ata = Pubkey::new_unique();

        let mut data = Vec::new();
        data.extend_from_slice(&WithdrawLiquidity::DISCRIMINATOR);
        data.extend_from_slice(&1_000_000u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.push(vault_bump);

        let ix = Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
                AccountMeta::new(signer, true),
                AccountMeta::new_readonly(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(left_ata, false),
                AccountMeta::new(user_left_ata, false),
                AccountMeta::new(right_ata, false),
                AccountMeta::new(user_right_ata, false),
                AccountMeta::new_readonly(pinocchio_token::id(), false),
            ],
        );

        let accounts = vec![
            (
                signer,
                Account {
                    lamports: 100_000_000,
                    owner: Pubkey::default(),
                    ..Account::default()
                },
            ),
            // pool authority is someone other than the signer
            (pool, pool_account_at(&program_id, 0, 0, 0, authority.to_bytes())),
            (
                vault,
                Account {
                    lamports: 1_000_000,
                    owner: Pubkey::default(),
                    ..Account::default()
                },
            ),
            (
                left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &vault, 10_000_000),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_left_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_left, &signer, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &vault, 10_000_000),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                user_right_ata,
                Account {
                    lamports: 1_000_000,
                    data: token_account_data(&mint_right, &signer, 0),
                    owner: pinocchio_token::id(),
                    ..Account::default()
                },
            ),
            (
                pinocchio_token::id(),
                mollusk_svm::program::create_program_account_loader_v3(&pinocchio_token::id()),
            ),
        ];

        mollusk(&program_id).process_and_validate_instruction(
            &ix,
            &accounts,
            &[Check::err(solana_sdk::program_error::ProgramError::Custom(
                PammError::Unauthorized as u32,
            ))],
        );
    }

    #[test]
    fn swap_fails_on_stale_millis() {
        use crate::error::PammError;
        // Batch clock and syscall clock agree on slot 0, so age is judged in ms.
        // 200 ms elapsed since the last update (ts in nanoseconds) > 100 ms.
        let (program_id, ix, accounts) = stale_swap_fixture(0, 0, 0, 200_000_000, 0);
        mollusk(&program_id).process_and_validate_instruction(
            &ix,
            &accounts,
            &[Check::err(solana_sdk::program_error::ProgramError::Custom(
                PammError::StaleQuoteMillis as u32,
            ))],
        );
    }

    #[test]
    fn swap_fails_on_stale_slots() {
        use crate::error::PammError;
        // Batch clock slot (3) lags the warped syscall slot (10), so age is
        // judged in slots: 10 - 5 = 5 slots since the last update > 1 slot.
        let (program_id, ix, accounts) = stale_swap_fixture(5, 0, 3, 0, 0);
        let mut m = mollusk(&program_id);
        m.warp_to_slot(10);
        m.process_and_validate_instruction(
            &ix,
            &accounts,
            &[Check::err(solana_sdk::program_error::ProgramError::Custom(
                PammError::StaleQuoteSlots as u32,
            ))],
        );
    }

    #[test]
    fn swap_succeeds_with_fresh_oracle() {
        // Batch clock slot (10) is current with the warped syscall slot (10),
        // and the oracle mid was updated 50 ms ago (timestamps in ns), within
        // the 100 ms tolerance.
        let (program_id, ix, accounts) = stale_swap_fixture(10, 0, 10, 50_000_000, 0);
        let mut m = mollusk(&program_id);
        m.warp_to_slot(10);
        m.process_and_validate_instruction(&ix, &accounts, &[Check::success()]);
    }

    #[test]
    fn swap_fails_on_slippage() {
        use crate::error::PammError;
        // Fresh oracle (slot 10 current, updated 50 ms ago), so the swap clears
        // the staleness gate. amount_in = 1_000_000 at mid = 1.0 yields
        // 999_900 out after the 1 bps spread. A min_tokens_out floor of
        // 1_000_000 is unreachable, so the swap is rejected on-chain.
        let (program_id, ix, accounts) = stale_swap_fixture(10, 0, 10, 50_000_000, 1_000_000);
        let mut m = mollusk(&program_id);
        m.warp_to_slot(10);
        m.process_and_validate_instruction(
            &ix,
            &accounts,
            &[Check::err(solana_sdk::program_error::ProgramError::Custom(
                PammError::SlippageExceeded as u32,
            ))],
        );
    }

    #[test]
    fn swap_succeeds_when_output_meets_floor() {
        // Same fresh oracle; the 999_900 output exactly meets a 999_900 floor,
        // so the slippage check passes.
        let (program_id, ix, accounts) = stale_swap_fixture(10, 0, 10, 50_000_000, 999_900);
        let mut m = mollusk(&program_id);
        m.warp_to_slot(10);
        m.process_and_validate_instruction(&ix, &accounts, &[Check::success()]);
    }
}
