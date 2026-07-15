# bAMM
## (B)atch-level Automated Market Maker

bAMM is a proof-of-concept implementation of an automated market maker that exclusively uses sub-slot block builder wallclock time to determine the staleness of oracle updates. The batch clock account read by this program is owned by a permissionless batch clock program. Block builders need to write to this program continuously over the block packing process to provide value to consumers of the data.

Due to the permissionless nature of the batch clock program, this AMM only enables swaps on trusted leader slots. The block builder can optionally claim write permissions to the batch clock account at the begging of a slot and becomes the oracle for sub-slot time for the duration of that slot.

By leveraging the batch clock account data on-chain programs can unlock time within a slot, accelerating the speed at which Internet Capital Markets tick on Solana.

## Oracle Staleness Checks

bAMM will write sub-slot timestamps to the batch clock account if the batch clock writer is a trusted block builder.

For demonstration purposes swaps are only allowed against this program's liquidity pool if:

* A trusted block builder has write access to the batch clock account
* The latest update is no more than 100ms old

## Optimal Performance with Maker Prioritization

Sub-slot timing logic work best with sub-slot scheduling features. Jito's BAM Maker Prioritization Plugin make it easy for liquidity providers to update their quotes at sub-slot frequencies. 
