// Copyright (C) 2019-2023 Aleo Systems Inc.
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

use super::*;

use circuit::AleoV0;

impl<N: Network, C: ConsensusStorage<N>> VM<N, C> {
    /// Executes a fee for the given private key, fee record, and fee amount (in microcredits).
    /// Returns the fee transaction.
    #[inline]
    pub fn execute_fee<R: Rng + CryptoRng>(
        &self,
        private_key: &PrivateKey<N>,
        fee_record: Record<N, Plaintext<N>>,
        fee_in_microcredits: u64,
        query: Option<Query<N, C::BlockStorage>>,
        rng: &mut R,
    ) -> Result<Transaction<N>> {
        // Compute the fee.
        let fee = self.execute_fee_raw(private_key, fee_record, fee_in_microcredits, query, rng)?.1;
        // Return the fee transaction.
        Transaction::from_fee(fee)
    }

    /// Executes a fee for the given private key, fee record, and fee amount (in microcredits).
    /// Returns the response, fee, and call metrics.
    #[inline]
    pub fn execute_fee_raw<R: Rng + CryptoRng>(
        &self,
        private_key: &PrivateKey<N>,
        fee_record: Record<N, Plaintext<N>>,
        fee_in_microcredits: u64,
        query: Option<Query<N, C::BlockStorage>>,
        rng: &mut R,
    ) -> Result<(Response<N>, Fee<N>, Vec<CallMetrics<N>>)> {
        let timer = timer!("VM::execute_fee_raw");

        // Prepare the query.
        let query = match query {
            Some(query) => query,
            None => Query::VM(self.block_store().clone()),
        };
        lap!(timer, "Prepare the query");

        // TODO (raychu86): Ensure that the fee record is associated with the `credits.aleo` program
        // Ensure that the record has enough balance to pay the fee.
        match fee_record.find(&[Identifier::from_str("microcredits")?]) {
            Ok(Entry::Private(Plaintext::Literal(Literal::U64(amount), _))) => {
                if *amount < fee_in_microcredits {
                    bail!("Fee record does not have enough balance to pay the fee")
                }
            }
            _ => bail!("Fee record does not have microcredits"),
        }

        // Compute the core logic.
        macro_rules! logic {
            ($process:expr, $network:path, $aleo:path) => {{
                type RecordPlaintext<NetworkMacro> = Record<NetworkMacro, Plaintext<NetworkMacro>>;

                // Prepare the private key and fee record.
                let private_key = cast_ref!(&private_key as PrivateKey<$network>);
                let fee_record = cast_ref!(fee_record as RecordPlaintext<$network>);
                lap!(timer, "Prepare the private key and fee record");

                // Execute the call to fee.
                let (response, fee_transition, inclusion, mut fee_assignments, metrics) =
                    $process.prepare_fee::<$aleo, _>(private_key, fee_record.clone(), fee_in_microcredits, rng)?;
                lap!(timer, "Execute the call to fee");

                // Prepare the assignments.
                let inclusion_assignments = {
                    let fee_transition = cast_ref!(fee_transition as Transition<N>);
                    let inclusion = cast_ref!(inclusion as Inclusion<N>);
                    inclusion.prepare_fee(fee_transition, query)?
                };
                let inclusion_assignments = cast_ref!(inclusion_assignments as Vec<InclusionAssignment<$network>>);

                let global_state_root = Inclusion::fee_global_state_root(inclusion_assignments)?;

                let inclusion_assignments = inclusion_assignments
                    .into_iter()
                    .map(|ia| ia.to_circuit_assignment::<AleoV0>().unwrap())
                    .collect_vec();
                let inclusion_assignments =
                    if inclusion_assignments.len() == 0 { None } else { Some(inclusion_assignments) };
                lap!(timer, "Prepare the assignments");

                let mut fee = Fee::from(fee_transition, global_state_root, None);

                // Execute the call.
                $process.execute_fee::<$aleo, _>(
                    &mut fee,
                    fee_assignments.make_contiguous(),
                    inclusion_assignments,
                    rng,
                )?;
                lap!(timer, "Execute the call");

                // Prepare the return.
                let response = cast_ref!(response as Response<N>).clone();
                let fee = cast_ref!(fee as Fee<N>).clone();
                let metrics = cast_ref!(metrics as Vec<CallMetrics<N>>).clone();

                // Return the response, fee, metrics.
                Ok((response, fee, metrics))
            }};
        }
        // Process the logic.
        process!(self, logic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::helpers::memory::ConsensusMemory;
    use console::{account::ViewKey, network::Testnet3, program::Ciphertext};
    // use snarkvm_fields::Field;
    use console::types::Field;

    use indexmap::IndexMap;

    type CurrentNetwork = Testnet3;

    fn prepare_vm(
        rng: &mut TestRng,
    ) -> Result<(
        VM<CurrentNetwork, ConsensusMemory<CurrentNetwork>>,
        IndexMap<Field<CurrentNetwork>, Record<CurrentNetwork, Ciphertext<CurrentNetwork>>>,
    )> {
        // Initialize the genesis block.
        let genesis = crate::vm::test_helpers::sample_genesis_block(rng);

        // Fetch the unspent records.
        let records = genesis.transitions().cloned().flat_map(Transition::into_records).collect::<IndexMap<_, _>>();

        // Initialize the genesis block.
        let genesis = crate::vm::test_helpers::sample_genesis_block(rng);

        // Initialize the VM.
        let vm = crate::vm::test_helpers::sample_vm();
        // Update the VM.
        vm.add_next_block(&genesis).unwrap();

        Ok((vm, records))
    }

    #[test]
    fn test_fee_transition_size() {
        let rng = &mut TestRng::default();

        // Initialize a new caller.
        let caller_private_key = crate::vm::test_helpers::sample_genesis_private_key(rng);
        let caller_view_key = ViewKey::try_from(&caller_private_key).unwrap();

        // Prepare the VM and records.
        let (vm, records) = prepare_vm(rng).unwrap();

        // Fetch the unspent record.
        let record = records.values().next().unwrap().decrypt(&caller_view_key).unwrap();

        // Execute.
        let (_, fee, _) = vm.execute_fee_raw(&caller_private_key, record, 1, None, rng).unwrap();

        // Assert the size of the transition.
        let fee_size_in_bytes = fee.to_bytes_le().unwrap().len();
        assert_eq!(1867, fee_size_in_bytes, "Update me if serialization has changed");
    }
}
