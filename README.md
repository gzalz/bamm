# bAMM
## (B)atch-level Automated Market Maker

This repo serves as an educational implementation for awareness of an open and permissionless builder batch clock program on Solana to enable out-of-protocol intra-slot timing to the SVM runtime.

bAMM is a proof-of-concept implementation of an automated market maker that exclusively uses sub-slot block builder wallclock time to determine the staleness of oracle updates. The batch clock account read by this program is owned by a permissionless batch clock program. Block builders need to write to this program continuously over the block packing process to provide value to consumers of the data.

Due to the permissionless nature of the batch clock program, this AMM only enables swaps on trusted leader slots. The block builder can optionally claim write permissions to the batch clock account at the begging of a slot and becomes the oracle for sub-slot time for the duration of that slot.

By leveraging the batch clock account data on-chain programs can unlock time within a slot, accelerating the speed at which Internet Capital Markets tick on Solana.

## Time within a Slot

bAMM will write intra-slot timestamps to the AMM pool state if the batch clock writer is a trusted block builder.

For demonstration purposes swaps are only allowed against this program's liquidity pool if:

* A trusted block builder has write access to the batch clock account
* The latest update is no more than 100ms old based on batch clock data

## Optimal Performance with Maker Prioritization

Sub-slot timing logic work best with sub-slot scheduling features. [Jito's BAM Maker Prioritization Plugin](https://bam.dev/plugins/) make it easy for liquidity providers to update their quotes at sub-slot frequencies.

## Impact
* Use time within a slot as inputs to pricing curves
* Use millisecond length TTL for quotes / orders when available
* Enhanced shred / pre-conf data, notion of builder batches propagated in real-time

## On-chain Data
[(MPP + Batch Clock) per Batch Scheduling](https://explorer.solana.com/block/422144963?cluster=testnet)\
[BAM Block Builder Signer](https://explorer.solana.com/address/BAMgx3XPWrXkNUuQiVWUZU6eB2HQZdwz9HNnT4tpo8LG?cluster=testnet)\
[Batch Clock Update Block](https://explorer.solana.com/block/422143302?cluster=testnet)\
[Successful Swap (47ms quote age)](https://explorer.solana.com/tx/3KAZujeaZFHZpbTjZFjU15SVYTuXtMjujHpgLUyYZyojPHbBSDNoQ7XiSgZkaST5kextazMeZURkNpJJBqLJPT11?cluster=testnet)\
[Failed Swap (250ms quote age)](https://explorer.solana.com/tx/26bAy6Ax8kg65jDDTYsf22hXkCAeFP765KG7W2tuoqdn8QeVC5Pezq6AwatXJR7kXrjRBfoP933PUWQvkUnNeurX?cluster=testnet)
