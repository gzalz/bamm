# bAMM
## (B)atch-level Automated Market Maker

bAMM is a proof-of-concept implementation of an automated market maker that exclusively uses sub-slot block builder wallclock time to determine the staleness of oracle updates. The batch clock account read by this program is owned by a permissionless batch clock program. Block builders need to write to this program continuously over the block packing process to provide value to consumers of the data.

Due to the permissionless nature of the batch clock program, this AMM only enables swaps on trusted leader slots. The block builder can optionally claim write permissions to the batch clock account at the begging of a slot and becomes the oracle for sub-slot time for the duration of that slot.

## Oracle Staleness Checks

bAMM will write sub-slot timestamps to the batch clock account if the batch clock writer is a trusted block builder.

For demonstration purposes swaps are only allowed against this program's liquidity pool if:
* The batch clock account is being written to this slot by a trusted signer
* The latest update is no more than 100ms old

## CLI

`cli/` is a `pamm` binary that builds the program's instructions (via `sdk/`),
signs them with a local keypair, and submits or simulates them against any RPC.

```bash
cargo build --manifest-path cli/Cargo.toml
BIN=cli/target/debug/pamm
```

Global options (all support env vars and apply to every subcommand):

| Flag | Env | Default | Meaning |
|------|-----|---------|---------|
| `-u, --rpc-url` | `SOLANA_RPC_URL` | `devnet` | URL or moniker (`mainnet-beta`/`devnet`/`testnet`/`localhost`) |
| `-k, --keypair` | `SOLANA_KEYPAIR` | `~/.config/solana/id.json` | Fee-payer / signer |
| `-p, --program-id` | `PAMM_PROGRAM_ID` | SDK built-in | Target program |
| `--commitment` | | `confirmed` | `processed`/`confirmed`/`finalized` |
| `--simulate` | | off | Simulate instead of submitting |

Subcommands:

```bash
# Show the derived PDAs (offline)
$BIN -p <PROGRAM_ID> addresses

# Read and decode the on-chain pool account
$BIN -p <PROGRAM_ID> -u devnet show-pool

# Initialize the pool + vault + both leg token accounts
$BIN -p <PROGRAM_ID> init-pool --mint-left <MINT> --mint-right <MINT>

# Write a new oracle mid (price is converted to Q64.64; slot/timestamp default
# to the current RPC slot and local clock)
$BIN -p <PROGRAM_ID> update-oracle --price 1.5
$BIN -p <PROGRAM_ID> update-oracle --mid <RAW_U128>

# Swap around the mid
$BIN -p <PROGRAM_ID> swap \
  --amount-in 1000000 --side left-to-right \
  --user-left-ata <PK> --user-right-ata <PK>
```

Add `--simulate` to any submitting subcommand to dry-run it (prints compute
units and program logs) before sending for real. Point `--rpc-url` at a
different cluster to run the same command elsewhere.
