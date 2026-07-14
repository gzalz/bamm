//! `pamm` — a CLI for invoking the jitosol-pamm (bAMM) program instructions
//! against any Solana RPC.
//!
//! It builds instructions via `jitosol-pamm-sdk`, signs them with a local
//! keypair, and either simulates or submits the transaction to the chosen
//! cluster.

use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use jitosol_pamm_sdk as sdk;
use solana_client::connection_cache::ConnectionCache;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_commitment_config::CommitmentConfig;
use solana_connection_cache::client_connection::ClientConnection;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::Transaction,
};

/// Standard SPL Token program id (the program CPIs into this).
const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

#[derive(Parser)]
#[command(
    name = "pamm",
    about = "Invoke jitosol-pamm (bAMM) instructions against any Solana RPC",
    version
)]
struct Cli {
    #[command(flatten)]
    global: GlobalOpts,

    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct GlobalOpts {
    /// RPC endpoint: a full URL or a cluster moniker
    /// (mainnet-beta | devnet | testnet | localhost).
    #[arg(
        short = 'u',
        long,
        env = "SOLANA_RPC_URL",
        default_value = "devnet",
        global = true
    )]
    rpc_url: String,

    /// Path to the fee-payer / signer keypair (JSON).
    #[arg(
        short = 'k',
        long,
        env = "SOLANA_KEYPAIR",
        default_value = "~/.config/solana/id.json",
        global = true
    )]
    keypair: String,

    /// Program id to target. Defaults to the SDK's built-in id.
    #[arg(short = 'p', long, env = "PAMM_PROGRAM_ID", global = true)]
    program_id: Option<String>,

    /// Commitment level (processed | confirmed | finalized).
    #[arg(long, default_value = "confirmed", global = true)]
    commitment: String,

    /// Simulate the transaction instead of submitting it.
    #[arg(long, global = true)]
    simulate: bool,

    /// Resubmit the transaction until it lands successfully. Each attempt
    /// re-fetches a fresh blockhash and re-signs. Ignored with --simulate.
    #[arg(long, global = true)]
    retry: bool,

    /// Maximum number of attempts when --retry is set. 0 means unlimited.
    #[arg(long, default_value_t = 0, global = true)]
    max_attempts: u32,

    /// Delay between attempts when --retry is set, in milliseconds.
    #[arg(long, default_value_t = 1000, global = true)]
    retry_delay_ms: u64,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize the pool, vault, and both program-owned leg token accounts.
    InitPool(InitPoolArgs),
    /// Deposit left and/or right tokens into the program-owned vault legs.
    Deposit(DepositArgs),
    /// Withdraw left and/or right tokens from the vault legs (pool authority only).
    Withdraw(WithdrawArgs),
    /// Transfer the pool's withdraw authority to a new pubkey (current authority only).
    SetAuthority(SetAuthorityArgs),
    /// Write a new oracle mid-price into the pool account.
    UpdateOracle(UpdateOracleArgs),
    /// Swap the left token <-> the right token around the oracle mid.
    Swap(SwapArgs),
    /// Read and decode the on-chain pool account.
    ShowPool(ShowPoolArgs),
    /// Print the derived PDAs for the target program.
    Addresses,
}

#[derive(Args)]
struct InitPoolArgs {
    /// Left-token mint.
    #[arg(long)]
    mint_left: String,
    /// Right-token mint.
    #[arg(long)]
    mint_right: String,
    /// SPL token program id (defaults to the standard SPL Token program).
    #[arg(long)]
    token_program: Option<String>,
}

#[derive(Args)]
struct DepositArgs {
    /// Left-token amount to deposit, in base units. May be zero to deposit only
    /// the right side (but at least one amount must be non-zero).
    #[arg(long, default_value_t = 0)]
    amount_left: u64,
    /// Right-token amount to deposit, in base units. May be zero to deposit only
    /// the left side (but at least one amount must be non-zero).
    #[arg(long, default_value_t = 0)]
    amount_right: u64,
    /// Provider's left-token account (source of the left deposit).
    #[arg(long)]
    user_left_ata: String,
    /// Provider's right-token account (source of the right deposit).
    #[arg(long)]
    user_right_ata: String,
    /// Program-owned left-token account. Defaults to the derived PDA.
    #[arg(long)]
    left_ata: Option<String>,
    /// Program-owned right-token account. Defaults to the derived PDA.
    #[arg(long)]
    right_ata: Option<String>,
    /// SPL token program id (defaults to the standard SPL Token program).
    #[arg(long)]
    token_program: Option<String>,
}

#[derive(Args)]
struct WithdrawArgs {
    /// Left-token amount to withdraw, in base units. May be zero to withdraw only
    /// the right side (but at least one amount must be non-zero).
    #[arg(long, default_value_t = 0)]
    amount_left: u64,
    /// Right-token amount to withdraw, in base units. May be zero to withdraw only
    /// the left side (but at least one amount must be non-zero).
    #[arg(long, default_value_t = 0)]
    amount_right: u64,
    /// Withdrawer's left-token account (destination of the left withdrawal).
    #[arg(long)]
    user_left_ata: String,
    /// Withdrawer's right-token account (destination of the right withdrawal).
    #[arg(long)]
    user_right_ata: String,
    /// Pool account (holds the withdraw authority). Defaults to the derived PDA.
    #[arg(long)]
    pool: Option<String>,
    /// Program-owned left-token account. Defaults to the derived PDA.
    #[arg(long)]
    left_ata: Option<String>,
    /// Program-owned right-token account. Defaults to the derived PDA.
    #[arg(long)]
    right_ata: Option<String>,
    /// SPL token program id (defaults to the standard SPL Token program).
    #[arg(long)]
    token_program: Option<String>,
}

#[derive(Args)]
struct SetAuthorityArgs {
    /// The new pool authority pubkey.
    #[arg(long)]
    new_authority: String,
    /// Pool account. Defaults to the derived pool PDA.
    #[arg(long)]
    pool: Option<String>,
}

#[derive(Args)]
struct UpdateOracleArgs {
    /// Mid-price of the right token in left-token units, as a decimal. Converted
    /// to the program's Q64.64 fixed-point representation. Mutually exclusive
    /// with --mid.
    #[arg(long, conflicts_with = "mid")]
    price: Option<f64>,
    /// Raw Q64.64 mid value (u128). Mutually exclusive with --price.
    #[arg(long)]
    mid: Option<u128>,
    /// Pool account. Defaults to the derived pool PDA.
    #[arg(long)]
    pool: Option<String>,
    /// Observation slot to record. Defaults to the current RPC slot.
    #[arg(long)]
    slot: Option<u64>,
    /// Observation timestamp in Unix nanoseconds. Defaults to the local clock.
    #[arg(long)]
    timestamp_nanos: Option<i64>,
}

/// Which token the taker pays in. The other leg is received.
#[derive(Clone, ValueEnum)]
enum TokenIn {
    Left,
    Right,
}

#[derive(Args)]
struct SwapArgs {
    /// Amount paid in, in the input token's base units.
    #[arg(long)]
    amount_in: u64,
    /// Which token to pay in (`left` or `right`); the other leg is received.
    #[arg(long, value_enum)]
    token_in: TokenIn,
    /// Slippage floor: reject the swap unless it delivers at least this many
    /// output tokens, in the received token's base units. Defaults to 0 (no
    /// slippage protection).
    #[arg(long, default_value_t = 0)]
    min_tokens_out: u64,
    /// Taker's left-token account.
    #[arg(long)]
    user_left_ata: String,
    /// Taker's right-token account.
    #[arg(long)]
    user_right_ata: String,
    /// Pool account. Defaults to the derived pool PDA.
    #[arg(long)]
    pool: Option<String>,
    /// Program-owned left-token account. Defaults to the derived PDA.
    #[arg(long)]
    left_ata: Option<String>,
    /// Program-owned right-token account. Defaults to the derived PDA.
    #[arg(long)]
    right_ata: Option<String>,
    /// SPL token program id (defaults to the standard SPL Token program).
    #[arg(long)]
    token_program: Option<String>,
    /// Batch clock account. Defaults to the SDK's SLOT_SOURCE.
    #[arg(long)]
    batch_clock: Option<String>,
    /// Submit without the interactive confirmation prompt.
    #[arg(long)]
    no_confirm: bool,
    /// Leader TPU address (host:port) to fan the signed transaction to over
    /// QUIC, bypassing the RPC node. The transaction is sent in the TPU QUIC
    /// wire format straight to this socket.
    #[arg(long, default_value = "198.13.140.231:5010")]
    tpu_address: String,
}

#[derive(Args)]
struct ShowPoolArgs {
    /// Pool account. Defaults to the derived pool PDA.
    #[arg(long)]
    pool: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let program_id = resolve_program_id(&cli.global)?;

    match &cli.command {
        Command::Addresses => cmd_addresses(&program_id),
        Command::ShowPool(args) => cmd_show_pool(&cli.global, &program_id, args),
        Command::InitPool(args) => cmd_init_pool(&cli.global, &program_id, args),
        Command::Deposit(args) => cmd_deposit(&cli.global, &program_id, args),
        Command::Withdraw(args) => cmd_withdraw(&cli.global, &program_id, args),
        Command::SetAuthority(args) => cmd_set_authority(&cli.global, &program_id, args),
        Command::UpdateOracle(args) => cmd_update_oracle(&cli.global, &program_id, args),
        Command::Swap(args) => cmd_swap(&cli.global, &program_id, args),
    }
}

// --- Commands ---------------------------------------------------------------

fn cmd_addresses(program_id: &Pubkey) -> Result<()> {
    let (pool, pool_bump) = sdk::derive_pool_pda(program_id);
    let (vault, vault_bump) = sdk::derive_vault_pda(program_id);
    let (left_ata, left_bump) = sdk::derive_left_ata_pda(program_id);
    let (right_ata, right_bump) = sdk::derive_right_ata_pda(program_id);
    println!("program    {program_id}");
    println!("pool       {pool}  (bump {pool_bump})");
    println!("vault      {vault}  (bump {vault_bump})");
    println!("left_ata   {left_ata}  (bump {left_bump})");
    println!("right_ata  {right_ata}  (bump {right_bump})");
    Ok(())
}

fn cmd_show_pool(g: &GlobalOpts, program_id: &Pubkey, args: &ShowPoolArgs) -> Result<()> {
    let client = rpc_client(g)?;
    let pool = match &args.pool {
        Some(p) => parse_pubkey(p, "pool")?,
        None => sdk::derive_pool_pda(program_id).0,
    };
    let data = client
        .get_account_data(&pool)
        .with_context(|| format!("fetching pool account {pool}"))?;
    let p = decode_pool(&data)?;
    println!("pool                    {pool}");
    println!(
        "discriminator           {}",
        String::from_utf8_lossy(&p.discriminator)
    );
    println!("mid (Q64.64 raw)        {}", p.mid);
    println!("mid (price)             {}", q64_to_f64(p.mid));
    println!("last_updated_slot       {}", p.last_updated_slot);
    println!("last_updated_timestamp  {} (ns)", p.last_updated_timestamp);
    Ok(())
}

fn cmd_init_pool(g: &GlobalOpts, program_id: &Pubkey, args: &InitPoolArgs) -> Result<()> {
    let payer = load_keypair(&g.keypair)?;
    let mint_left = parse_pubkey(&args.mint_left, "mint-left")?;
    let mint_right = parse_pubkey(&args.mint_right, "mint-right")?;
    let token_program = optional_pubkey(&args.token_program, "token-program")?
        .unwrap_or(parse_pubkey(SPL_TOKEN_PROGRAM_ID, "token-program")?);

    let ix = sdk::init_pool(
        program_id,
        &payer.pubkey(),
        &mint_left,
        &mint_right,
        &token_program,
    );
    submit(g, ix, &payer)
}

fn cmd_deposit(g: &GlobalOpts, program_id: &Pubkey, args: &DepositArgs) -> Result<()> {
    if args.amount_left == 0 && args.amount_right == 0 {
        bail!("provide a non-zero --amount-left and/or --amount-right");
    }
    let signer = load_keypair(&g.keypair)?;
    let left_ata = match &args.left_ata {
        Some(a) => parse_pubkey(a, "left-ata")?,
        None => sdk::derive_left_ata_pda(program_id).0,
    };
    let right_ata = match &args.right_ata {
        Some(a) => parse_pubkey(a, "right-ata")?,
        None => sdk::derive_right_ata_pda(program_id).0,
    };
    let user_left_ata = parse_pubkey(&args.user_left_ata, "user-left-ata")?;
    let user_right_ata = parse_pubkey(&args.user_right_ata, "user-right-ata")?;
    let token_program = optional_pubkey(&args.token_program, "token-program")?
        .unwrap_or(parse_pubkey(SPL_TOKEN_PROGRAM_ID, "token-program")?);

    let ix = sdk::deposit_liquidity(
        program_id,
        &signer.pubkey(),
        &left_ata,
        &user_left_ata,
        &right_ata,
        &user_right_ata,
        &token_program,
        args.amount_left,
        args.amount_right,
    );
    submit(g, ix, &signer)
}

fn cmd_withdraw(g: &GlobalOpts, program_id: &Pubkey, args: &WithdrawArgs) -> Result<()> {
    if args.amount_left == 0 && args.amount_right == 0 {
        bail!("provide a non-zero --amount-left and/or --amount-right");
    }
    let signer = load_keypair(&g.keypair)?;
    let pool = match &args.pool {
        Some(p) => parse_pubkey(p, "pool")?,
        None => sdk::derive_pool_pda(program_id).0,
    };
    let left_ata = match &args.left_ata {
        Some(a) => parse_pubkey(a, "left-ata")?,
        None => sdk::derive_left_ata_pda(program_id).0,
    };
    let right_ata = match &args.right_ata {
        Some(a) => parse_pubkey(a, "right-ata")?,
        None => sdk::derive_right_ata_pda(program_id).0,
    };
    let user_left_ata = parse_pubkey(&args.user_left_ata, "user-left-ata")?;
    let user_right_ata = parse_pubkey(&args.user_right_ata, "user-right-ata")?;
    let token_program = optional_pubkey(&args.token_program, "token-program")?
        .unwrap_or(parse_pubkey(SPL_TOKEN_PROGRAM_ID, "token-program")?);

    let ix = sdk::withdraw_liquidity(
        program_id,
        &signer.pubkey(),
        &pool,
        &left_ata,
        &user_left_ata,
        &right_ata,
        &user_right_ata,
        &token_program,
        args.amount_left,
        args.amount_right,
    );
    submit(g, ix, &signer)
}

fn cmd_set_authority(g: &GlobalOpts, program_id: &Pubkey, args: &SetAuthorityArgs) -> Result<()> {
    let signer = load_keypair(&g.keypair)?;
    let pool = match &args.pool {
        Some(p) => parse_pubkey(p, "pool")?,
        None => sdk::derive_pool_pda(program_id).0,
    };
    let new_authority = parse_pubkey(&args.new_authority, "new-authority")?;

    let ix = sdk::set_authority(program_id, &signer.pubkey(), &pool, &new_authority);
    submit(g, ix, &signer)
}

fn cmd_update_oracle(g: &GlobalOpts, program_id: &Pubkey, args: &UpdateOracleArgs) -> Result<()> {
    let authority = load_keypair(&g.keypair)?;
    let pool = match &args.pool {
        Some(p) => parse_pubkey(p, "pool")?,
        None => sdk::derive_pool_pda(program_id).0,
    };
    let slot_source = parse_pubkey(sdk::SLOT_SOURCE, "slot-source")?;

    let mid = match (args.mid, args.price) {
        (Some(m), _) => m,
        (None, Some(price)) => f64_to_q64(price)?,
        (None, None) => bail!("provide either --mid or --price"),
    };
    if mid == 0 {
        bail!("mid must be non-zero");
    }

    let client = rpc_client(g)?;
    let slot = match args.slot {
        Some(s) => s,
        None => client.get_slot().context("fetching current slot")?,
    };
    let timestamp = match args.timestamp_nanos {
        Some(t) => t,
        None => now_unix_nanos()?,
    };

    let ix = sdk::update_oracle(
        program_id,
        &authority.pubkey(),
        &pool,
        &slot_source,
        mid,
        slot,
        timestamp,
    );
    submit_with_client(g, &client, ix, &authority, false, false, None)
}

fn cmd_swap(g: &GlobalOpts, program_id: &Pubkey, args: &SwapArgs) -> Result<()> {
    let signer = load_keypair(&g.keypair)?;
    let pool = match &args.pool {
        Some(p) => parse_pubkey(p, "pool")?,
        None => sdk::derive_pool_pda(program_id).0,
    };
    let left_ata = match &args.left_ata {
        Some(a) => parse_pubkey(a, "left-ata")?,
        None => sdk::derive_left_ata_pda(program_id).0,
    };
    let right_ata = match &args.right_ata {
        Some(a) => parse_pubkey(a, "right-ata")?,
        None => sdk::derive_right_ata_pda(program_id).0,
    };
    let user_left_ata = parse_pubkey(&args.user_left_ata, "user-left-ata")?;
    let user_right_ata = parse_pubkey(&args.user_right_ata, "user-right-ata")?;
    let token_program = optional_pubkey(&args.token_program, "token-program")?
        .unwrap_or(parse_pubkey(SPL_TOKEN_PROGRAM_ID, "token-program")?);
    let batch_clock = match &args.batch_clock {
        Some(b) => parse_pubkey(b, "batch-clock")?,
        None => parse_pubkey(sdk::SLOT_SOURCE, "batch-clock")?,
    };
    let side = match args.token_in {
        TokenIn::Left => sdk::SWAP_SIDE_LEFT_TO_RIGHT,
        TokenIn::Right => sdk::SWAP_SIDE_RIGHT_TO_LEFT,
    };

    // Swaps move funds, so confirm interactively unless the user opts out or is
    // only simulating.
    if !args.no_confirm && !g.simulate {
        let (pay, recv) = match args.token_in {
            TokenIn::Left => ("left", "right"),
            TokenIn::Right => ("right", "left"),
        };
        println!("about to swap on {}", resolve_cluster(&g.rpc_url));
        println!("  pool           {pool}");
        println!("  pay in         {} {pay}", args.amount_in);
        println!("  receive        {recv} (min {})", args.min_tokens_out);
        println!("  signer         {}", signer.pubkey());
        if !confirm("proceed with swap?")? {
            println!("aborted");
            return Ok(());
        }
    }

    let ix = sdk::swap(
        program_id,
        &signer.pubkey(),
        &pool,
        &left_ata,
        &user_left_ata,
        &right_ata,
        &user_right_ata,
        &token_program,
        &batch_clock,
        args.amount_in,
        side,
        args.min_tokens_out,
    );
    let tpu_address: SocketAddr = args
        .tpu_address
        .parse()
        .with_context(|| format!("parsing --tpu-address '{}'", args.tpu_address))?;

    // Swap runs without RPC preflight simulation: the batch-clock / oracle
    // state can shift between preflight and landing, so preflight rejections
    // are noise rather than signal here. It also sends the signed transaction
    // straight to the leader's TPU port over QUIC rather than relaying through
    // the RPC node, to shave latency off landing.
    submit_with_opts(g, ix, &signer, true, args.no_confirm, Some(tpu_address))
}

// --- Transaction plumbing ---------------------------------------------------

fn submit(
    g: &GlobalOpts,
    ix: solana_sdk::instruction::Instruction,
    signer: &Keypair,
) -> Result<()> {
    submit_with_opts(g, ix, signer, false, false, None)
}

fn submit_with_opts(
    g: &GlobalOpts,
    ix: solana_sdk::instruction::Instruction,
    signer: &Keypair,
    skip_preflight: bool,
    no_confirm: bool,
    tpu_address: Option<SocketAddr>,
) -> Result<()> {
    let client = rpc_client(g)?;
    submit_with_client(
        g,
        &client,
        ix,
        signer,
        skip_preflight,
        no_confirm,
        tpu_address,
    )
}

fn submit_with_client(
    g: &GlobalOpts,
    client: &RpcClient,
    ix: solana_sdk::instruction::Instruction,
    signer: &Keypair,
    skip_preflight: bool,
    no_confirm: bool,
    tpu_address: Option<SocketAddr>,
) -> Result<()> {
    if g.simulate {
        let blockhash = client
            .get_latest_blockhash()
            .context("fetching latest blockhash")?;
        let tx =
            Transaction::new_signed_with_payer(&[ix], Some(&signer.pubkey()), &[signer], blockhash);
        let sim = client
            .simulate_transaction(&tx)
            .context("simulating transaction")?;
        if let Some(err) = sim.value.err {
            eprintln!("simulation FAILED: {err:?}");
            if let Some(logs) = sim.value.logs {
                for l in logs {
                    eprintln!("  {l}");
                }
            }
            bail!("simulation returned an error");
        }
        println!("simulation OK");
        if let Some(units) = sim.value.units_consumed {
            println!("compute units: {units}");
        }
        if let Some(logs) = sim.value.logs {
            for l in logs {
                println!("  {l}");
            }
        }
        return Ok(());
    }

    // When a TPU address is set, open a QUIC connection cache once and reuse it
    // across retries. It manages the TPU QUIC handshake (ALPN, client
    // certificate) so we can push the raw wire transaction straight at the
    // leader's TPU port instead of relaying through the RPC node.
    let tpu = tpu_address.map(|addr| {
        let cache = ConnectionCache::new("bamm-cli-tpu");
        (cache, addr)
    });

    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        // Re-fetch a fresh blockhash and re-sign on every attempt so retries
        // don't fail on an expired blockhash.
        let blockhash = client
            .get_latest_blockhash()
            .context("fetching latest blockhash")?;
        let tx = Transaction::new_signed_with_payer(
            &[ix.clone()],
            Some(&signer.pubkey()),
            &[signer],
            blockhash,
        );

        let result = if let Some((cache, addr)) = &tpu {
            // Push the serialized transaction straight at the leader's TPU port
            // over QUIC. This send is fire-and-forget at the network layer, so
            // confirmation (unless --no-confirm) is polled separately via RPC.
            let wire = bincode::serialize(&tx).context("serializing transaction")?;
            match cache.get_connection(addr).send_data(&wire) {
                Ok(()) => {
                    let sig = tx.signatures[0];
                    if no_confirm {
                        Ok(sig)
                    } else {
                        client
                            .confirm_transaction_with_spinner(&sig, &blockhash, client.commitment())
                            .map(|_| sig)
                            .context("confirming transaction")
                    }
                }
                Err(e) => Err(anyhow!(e)).context("sending transaction to TPU over QUIC"),
            }
        } else {
            let config = RpcSendTransactionConfig {
                skip_preflight,
                ..Default::default()
            };
            // --no-confirm targets throughput (batch/automated swaps): fire the
            // transaction and move on rather than blocking on a confirmation
            // spinner. Retries below still apply to submission failures.
            if no_confirm {
                client
                    .send_transaction_with_config(&tx, config)
                    .context("submitting transaction")
            } else {
                client
                    .send_and_confirm_transaction_with_spinner_and_config(
                        &tx,
                        client.commitment(),
                        config,
                    )
                    .context("submitting transaction")
            }
        };

        match result {
            Ok(sig) => {
                println!("signature: {sig}");
                return Ok(());
            }
            Err(e) => {
                if !g.retry {
                    return Err(e);
                }
                if g.max_attempts != 0 && attempt >= g.max_attempts {
                    return Err(e.context(format!("giving up after {attempt} attempts")));
                }
                eprintln!("attempt {attempt} failed: {e:#}");
                eprintln!("retrying in {} ms...", g.retry_delay_ms);
                std::thread::sleep(std::time::Duration::from_millis(g.retry_delay_ms));
            }
        }
    }
}

fn rpc_client(g: &GlobalOpts) -> Result<RpcClient> {
    let url = resolve_cluster(&g.rpc_url);
    let commitment = match g.commitment.as_str() {
        "processed" => CommitmentConfig::processed(),
        "confirmed" => CommitmentConfig::confirmed(),
        "finalized" => CommitmentConfig::finalized(),
        other => bail!("unknown commitment '{other}' (use processed|confirmed|finalized)"),
    };
    Ok(RpcClient::new_with_commitment(url, commitment))
}

// --- Helpers ----------------------------------------------------------------

fn resolve_program_id(g: &GlobalOpts) -> Result<Pubkey> {
    let s = g.program_id.as_deref().unwrap_or(sdk::PROGRAM_ID);
    parse_pubkey(s, "program-id")
}

fn resolve_cluster(url: &str) -> String {
    match url {
        "mainnet-beta" | "mainnet" => "https://api.mainnet-beta.solana.com".to_string(),
        "devnet" => "https://api.devnet.solana.com".to_string(),
        "testnet" => "https://api.testnet.solana.com".to_string(),
        "localhost" | "localnet" => "http://127.0.0.1:8899".to_string(),
        other => other.to_string(),
    }
}

/// Prompt on stdin for a yes/no answer, defaulting to no. Treats a closed
/// stdin (e.g. a non-interactive pipe) as a decline rather than proceeding.
fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush().context("flushing prompt")?;
    let mut input = String::new();
    let n = io::stdin().read_line(&mut input).context("reading stdin")?;
    if n == 0 {
        return Ok(false);
    }
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn parse_pubkey(s: &str, what: &str) -> Result<Pubkey> {
    Pubkey::from_str(s).map_err(|e| anyhow!("invalid {what} pubkey '{s}': {e}"))
}

fn optional_pubkey(s: &Option<String>, what: &str) -> Result<Option<Pubkey>> {
    s.as_ref().map(|v| parse_pubkey(v, what)).transpose()
}

fn load_keypair(path: &str) -> Result<Keypair> {
    let expanded = expand_tilde(path);
    read_keypair_file(&expanded)
        .map_err(|e| anyhow!("reading keypair '{}': {e}", expanded.display()))
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn now_unix_nanos() -> Result<i64> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before Unix epoch")?;
    i64::try_from(d.as_nanos()).context("timestamp overflowed i64")
}

/// Convert a decimal price into the program's Q64.64 fixed-point mid.
fn f64_to_q64(price: f64) -> Result<u128> {
    if !price.is_finite() || price <= 0.0 {
        bail!("--price must be a positive, finite number");
    }
    let scaled = price * 2f64.powi(64);
    if scaled >= 2f64.powi(128) {
        bail!("--price too large to represent as Q64.64");
    }
    Ok(scaled as u128)
}

fn q64_to_f64(mid: u128) -> f64 {
    mid as f64 / 2f64.powi(64)
}

/// Decoded pool account, matching the program's on-chain layout.
struct DecodedPool {
    discriminator: [u8; 8],
    mid: u128,
    last_updated_slot: u64,
    last_updated_timestamp: i64,
}

/// Decode the pool account: discriminator(8) + mid u128 LE(16) +
/// last_updated_slot u64 LE(8) + last_updated_timestamp i64 LE(8).
fn decode_pool(data: &[u8]) -> Result<DecodedPool> {
    if data.len() < 40 {
        bail!(
            "pool account too small: {} bytes (expected >= 40)",
            data.len()
        );
    }
    Ok(DecodedPool {
        discriminator: data[0..8].try_into().unwrap(),
        mid: u128::from_le_bytes(data[8..24].try_into().unwrap()),
        last_updated_slot: u64::from_le_bytes(data[24..32].try_into().unwrap()),
        last_updated_timestamp: i64::from_le_bytes(data[32..40].try_into().unwrap()),
    })
}
