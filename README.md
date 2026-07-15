# bAMM
## (B)atch-level Automated Market Maker

This repo serves as an educational implementation for awareness of an open and permissionless builder batch clock program on Solana to enable out-of-protocol intra-slot timing to the SVM runtime.

bAMM is a proof-of-concept implementation of an automated market maker that exclusively uses sub-slot block builder wallclock time to determine the staleness of oracle updates. The batch clock account read by this program is owned by a permissionless batch clock program. Block builders need to write to this program continuously over the block packing process to provide value to consumers of the data.

Due to the permissionless nature of the batch clock program, this AMM only enables swaps on trusted leader slots. The block builder can optionally claim write permissions to the batch clock account at the begging of a slot and becomes the oracle for sub-slot time for the duration of that slot.

By leveraging the batch clock account data on-chain programs can unlock time within a slot, accelerating the speed at which Internet Capital Markets tick on Solana.

## Time within a Slot

bAMM will write intra-slot timestamps to the AMM pool state if the batch clock writer is a trusted block builder and is actively writing to the batch clock for the current slot.

For demonstration purposes swaps are only allowed against this program's liquidity pool if:

* A trusted block builder has write access to the batch clock account
* The latest update is no more than 100ms old based on batch clock data
* Minimum tokens out specified in the instruction can be satisfied by the pool's current state

## Optimal Performance with Maker Prioritization

Sub-slot timing logic work best with sub-slot scheduling features. [Jito's BAM Maker Prioritization Plugin](https://bam.dev/plugins/) make it easy for liquidity providers to update their quotes at sub-slot frequencies.

[(MPP + Batch Clock) per Batch Scheduling](https://explorer.solana.com/block/422144963?cluster=testnet)

## Impact
* Use time within a slot as inputs to pricing curves
* Use millisecond length TTL for quotes / orders when available
* Enhanced shred / pre-conf data, notion of builder batches propagated in real-time

## On-chain Data
[BAM Block Builder Signer](https://explorer.solana.com/address/BAMgx3XPWrXkNUuQiVWUZU6eB2HQZdwz9HNnT4tpo8LG?cluster=testnet)\
[Batch Clock Update Block](https://explorer.solana.com/block/422143302?cluster=testnet)\
[Successful Swap (47ms quote age)](https://explorer.solana.com/tx/3KAZujeaZFHZpbTjZFjU15SVYTuXtMjujHpgLUyYZyojPHbBSDNoQ7XiSgZkaST5kextazMeZURkNpJJBqLJPT11?cluster=testnet)\
[Failed Swap (202ms quote age)](https://explorer.solana.com/tx/26bAy6Ax8kg65jDDTYsf22hXkCAeFP765KG7W2tuoqdn8QeVC5Pezq6AwatXJR7kXrjRBfoP933PUWQvkUnNeurX?cluster=testnet)

### Program

| | address |
| --- | --- |
| Program ID | `BAMMyteHxGnsekYnLMBUwKySutG2N7EV9GhkoDwg73Yd` |
| BAM Block Builder | `BAMgx3XPWrXkNUuQiVWUZU6eB2HQZdwz9HNnT4tpo8LG` |
| Testnet Pool Authority | `oRaB5ZkeBvGqbmvCHwX9nvzzu9FeBNC6xuzTLeQvY3p` |

### Instructions

Each instruction is selected by an 8-byte ASCII discriminator at the head of the
instruction data. All multi-byte integers are little-endian. Discriminators are
matched in the order below (see `src/lib.rs`).

#### `InitPool` — `initpool`

Creates the pool state account, seeds the vault PDA, and creates + initializes
the two program-owned leg token accounts.

Instruction data (12 bytes):

| off | size | field            | type |
|-----|------|------------------|------|
| 0   | 8    | discriminator    | `[u8; 8]` = `"initpool"` |
| 8   | 1    | `pool_bump`      | `u8` |
| 9   | 1    | `vault_bump`     | `u8` |
| 10  | 1    | `left_ata_bump`  | `u8` |
| 11  | 1    | `right_ata_bump` | `u8` |

Accounts (9):

| # | account          | signer | writable | notes |
|---|------------------|--------|----------|-------|
| 0 | `payer`          | yes    | yes      | funds account creation; becomes the pool withdraw authority |
| 1 | `pool`           | no     | yes      | PDA `[b"pool"]`, created program-owned |
| 2 | `vault`          | no     | yes      | PDA `[b"vault"]`, common authority over both legs |
| 3 | `mint_left`      | no     | no       | left-token mint |
| 4 | `left_ata`       | no     | yes      | PDA `[b"leftata"]`, left leg (owner = `vault`) |
| 5 | `mint_right`     | no     | no       | right-token mint |
| 6 | `right_ata`      | no     | yes      | PDA `[b"rightata"]`, right leg (owner = `vault`) |
| 7 | `token_program`  | no     | no       | SPL Token program |
| 8 | `system_program` | no     | no       | System program |

#### `UpdateOracle` — `setmid00`

Writes a fresh Q64.64 mid onto the pool, stamping it with the slot and timestamp
read from the trusted batch clock. Rejected unless the batch clock's `slot_owner`
is a trusted signer and its slot equals the syscall `Clock::slot`.

Instruction data (≥ 24 bytes; trailing bytes accepted for client compatibility):

| off | size | field         | type |
|-----|------|---------------|------|
| 0   | 8    | discriminator | `[u8; 8]` = `"setmid00"` |
| 8   | 16   | `mid`         | `u128` (Q64.64, must be non-zero) |

Accounts (3):

| # | account       | signer | writable | notes |
|---|---------------|--------|----------|-------|
| 0 | `authority`   | yes    | no       | update signer |
| 1 | `pool`        | no     | yes      | pool state, mid written in place |
| 2 | `slot_source` | no     | no       | must equal `SLOT_SOURCE` (the batch clock) |

#### `Swap` — `swap0000`

Exchanges the left token for the right (or vice versa) at the oracle mid less a
fixed 1 bps spread. Gated on the batch clock being current and the mid being no
older than `MAX_QUOTE_AGE_MS` (100 ms).

Instruction data (26 bytes):

| off | size | field            | type |
|-----|------|------------------|------|
| 0   | 8    | discriminator    | `[u8; 8]` = `"swap0000"` |
| 8   | 8    | `amount_in`      | `u64` (must be non-zero) |
| 16  | 1    | `side`           | `u8` (0 = left→right, 1 = right→left) |
| 17  | 1    | `vault_bump`     | `u8` |
| 18  | 8    | `min_tokens_out` | `u64` (slippage floor) |

Accounts (9):

| # | account          | signer | writable | notes |
|---|------------------|--------|----------|-------|
| 0 | `signer`         | yes    | yes      | the taker |
| 1 | `pool`           | no     | no       | supplies the oracle mid + last-update stamp |
| 2 | `vault`          | no     | yes      | PDA `[b"vault", bump]`, signs the payout leg |
| 3 | `left_ata`       | no     | yes      | program-owned left leg (owner = `vault`) |
| 4 | `user_left_ata`  | no     | yes      | taker's left-token account |
| 5 | `right_ata`      | no     | yes      | program-owned right leg (owner = `vault`) |
| 6 | `user_right_ata` | no     | yes      | taker's right-token account |
| 7 | `token_program`  | no     | no       | SPL Token program |
| 8 | `batch_clock`    | no     | no       | must equal `SLOT_SOURCE`; supplies slot + timestamp |

#### `DepositLiquidity` — `deposit0`

Moves left and/or right tokens from the caller's accounts into the program-owned
vault legs. The caller signs for their own accounts, so no vault signature is
required. Either amount may be zero to deposit a single side.

Instruction data (24 bytes):

| off | size | field         | type |
|-----|------|---------------|------|
| 0   | 8    | discriminator | `[u8; 8]` = `"deposit0"` |
| 8   | 8    | `amount_left` | `u64` |
| 16  | 8    | `amount_right`| `u64` |

Accounts (6):

| # | account          | signer | writable | notes |
|---|------------------|--------|----------|-------|
| 0 | `signer`         | yes    | yes      | the liquidity provider |
| 1 | `left_ata`       | no     | yes      | program-owned left leg |
| 2 | `user_left_ata`  | no     | yes      | provider's left-token account |
| 3 | `right_ata`      | no     | yes      | program-owned right leg |
| 4 | `user_right_ata` | no     | yes      | provider's right-token account |
| 5 | `token_program`  | no     | no       | SPL Token program |

#### `WithdrawLiquidity` — `withdrw0`

Moves left and/or right tokens out of the vault legs back to the caller. The
vault PDA is the leg authority and signs the transfers. Only the pool authority
recorded at `InitPool` may withdraw. Either amount may be zero.

Instruction data (25 bytes):

| off | size | field         | type |
|-----|------|---------------|------|
| 0   | 8    | discriminator | `[u8; 8]` = `"withdrw0"` |
| 8   | 8    | `amount_left` | `u64` |
| 16  | 8    | `amount_right`| `u64` |
| 24  | 1    | `vault_bump`  | `u8` |

Accounts (8):

| # | account          | signer | writable | notes |
|---|------------------|--------|----------|-------|
| 0 | `signer`         | yes    | yes      | must equal the pool authority |
| 1 | `pool`           | no     | no       | holds the withdraw authority |
| 2 | `vault`          | no     | yes      | PDA `[b"vault", bump]`, signs the transfers |
| 3 | `left_ata`       | no     | yes      | program-owned left leg |
| 4 | `user_left_ata`  | no     | yes      | withdrawer's left-token account |
| 5 | `right_ata`      | no     | yes      | program-owned right leg |
| 6 | `user_right_ata` | no     | yes      | withdrawer's right-token account |
| 7 | `token_program`  | no     | no       | SPL Token program |

#### `SetAuthority` — `setauth0`

Reassigns the pool's withdraw authority. Only the current authority may call it;
the field is overwritten in place at `Pool::AUTHORITY_OFFSET`.

Instruction data (40 bytes):

| off | size | field           | type |
|-----|------|-----------------|------|
| 0   | 8    | discriminator   | `[u8; 8]` = `"setauth0"` |
| 8   | 32   | `new_authority` | `Pubkey` |

Accounts (2):

| # | account   | signer | writable | notes |
|---|-----------|--------|----------|-------|
| 0 | `signer`  | yes    | no       | must equal the current pool authority |
| 1 | `pool`    | no     | yes      | authority field updated in place |

##### Custom errors

Program errors surface to clients as `ProgramError::Custom(code)`:

| code | error | meaning |
|------|-------|---------|
| 0 | `StaleQuoteMillis` | mid older than `MAX_QUOTE_AGE_MS` |
| 1 | `StaleQuoteSlots` | batch clock slot lags the syscall clock |
| 2 | `Unauthorized` | signer is not the pool authority |
| 3 | `SlippageExceeded` | output below `min_tokens_out` |
| 4 | `BlockBuilderNotTrusted` | batch clock `slot_owner` not in `trusted_signers` |

### Accounts

Both accounts are (de)serialized with [`wincode`](https://docs.rs/wincode) in
field order (a bincode-compatible layout), each led by an 8-byte ASCII
discriminator. All integers are little-endian.

#### `Pool` — `pool0000`

The proactive market-maker state: a single oracle mid plus its freshness stamp
and the withdraw authority. `SIZE = 72` bytes. `UpdateOracle` rewrites `mid`,
`last_updated_slot`, and `last_updated_timestamp` in place at their fixed offsets
to save compute.

| off | size | field                    | type      | notes |
|-----|------|--------------------------|-----------|-------|
| 0   | 8    | `discriminator`          | `[u8; 8]` | `"pool0000"` |
| 8   | 16   | `mid`                    | `u128`    | mid-price, Q64.64 (right token in left units) |
| 24  | 8    | `last_updated_slot`      | `u64`     | slot of the last mid update |
| 32  | 8    | `last_updated_timestamp` | `i64`     | ns UNIX of the last mid update |
| 40  | 32   | `authority`              | `Pubkey`  | withdraw authority (set at `InitPool`) |

Price conversions:

```
right_units = (left_units * mid) >> 64
left_units  = (right_units << 64) / mid
```

#### `BatchClock` — `BATCHCLK`

The open batch-clock standard published by a block builder: a fixed 16-byte-aligned
header followed by a per-tick body. Total 96 bytes. This program only honors a
batch clock whose `slot_owner` is in `trusted_signers`, and only when `slot`
equals the syscall `Clock::slot`.

Header:

| off | size | field           | type      | notes |
|-----|------|-----------------|-----------|-------|
| 0   | 8    | `discriminator` | `[u8; 8]` | `"BATCHCLK"` |
| 8   | 2    | `version`       | `u16`     | ABI version (1) |
| 10  | 6    | `_pad`          | `[u8; 6]` | aligns `slot_owner` to offset 16 |
| 16  | 32   | `slot_owner`    | `Pubkey`  | writer that opened the current slot |

Tick:

| off | size | field                  | type  | notes |
|-----|------|------------------------|-------|-------|
| 48  | 8    | `slot`                 | `u64` | validated `== Clock::slot` |
| 56  | 8    | `slot_start_timestamp` | `i64` | ns UNIX, slot-constant |
| 64  | 8    | `timestamp_ns`         | `i64` | ns UNIX at this tick |
| 72  | 8    | `sequence`             | `u64` | monotonic within the slot |
| 80  | 8    | `compute_units_used`   | `u64` | cumulative CU at tick time |
| 88  | 8    | `compute_unit_limit`   | `u64` | block CU cap, slot-constant |
