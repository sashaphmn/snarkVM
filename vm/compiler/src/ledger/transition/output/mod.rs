// Copyright (C) 2019-2022 Aleo Systems Inc.
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

mod bytes;
mod serialize;
mod string;

use console::{
    network::prelude::*,
    program::{Ciphertext, Plaintext, Record},
    types::{Field, Group},
};

type Variant = u16;

/// The transition output.
#[derive(Clone, PartialEq, Eq)]
pub enum Output<N: Network> {
    /// The plaintext hash and (optional) plaintext.
    Constant(Field<N>, Option<Plaintext<N>>),
    /// The plaintext hash and (optional) plaintext.
    Public(Field<N>, Option<Plaintext<N>>),
    /// The ciphertext hash and (optional) ciphertext.
    Private(Field<N>, Option<Ciphertext<N>>),
    /// The commitment, checksum, and (optional) record ciphertext.
    Record(Field<N>, Field<N>, Option<Record<N, Ciphertext<N>>>),
    /// The output commitment of the external record. Note: This is **not** the record commitment.
    ExternalRecord(Field<N>),
}

impl<N: Network> Output<N> {
    /// Returns the variant of the output.
    pub const fn variant(&self) -> Variant {
        match self {
            Output::Constant(_, _) => 0,
            Output::Public(_, _) => 1,
            Output::Private(_, _) => 2,
            Output::Record(_, _, _) => 3,
            Output::ExternalRecord(_) => 4,
        }
    }

    /// Returns the ID of the output.
    pub const fn id(&self) -> &Field<N> {
        match self {
            Output::Constant(id, ..) => id,
            Output::Public(id, ..) => id,
            Output::Private(id, ..) => id,
            Output::Record(commitment, ..) => commitment,
            Output::ExternalRecord(id) => id,
        }
    }

    /// Returns the commitment and record, if the output is a record.
    #[allow(clippy::type_complexity)]
    pub const fn record(&self) -> Option<(&Field<N>, &Record<N, Ciphertext<N>>)> {
        match self {
            Output::Record(commitment, _, Some(record)) => Some((commitment, record)),
            _ => None,
        }
    }

    /// Consumes `self` and returns the commitment and record, if the output is a record.
    #[allow(clippy::type_complexity)]
    pub fn into_record(self) -> Option<(Field<N>, Record<N, Ciphertext<N>>)> {
        match self {
            Output::Record(commitment, _, Some(record)) => Some((commitment, record)),
            _ => None,
        }
    }

    /// Returns the commitment, if the output is a record.
    pub const fn commitment(&self) -> Option<&Field<N>> {
        match self {
            Output::Record(commitment, ..) => Some(commitment),
            _ => None,
        }
    }

    /// Returns the commitment, if the output is a record, and consumes `self`.
    pub fn into_commitment(self) -> Option<Field<N>> {
        match self {
            Output::Record(commitment, ..) => Some(commitment),
            _ => None,
        }
    }

    /// Returns the nonce, if the output is a record.
    pub const fn nonce(&self) -> Option<&Group<N>> {
        match self {
            Output::Record(_, _, Some(record)) => Some(record.nonce()),
            _ => None,
        }
    }

    /// Returns the nonce, if the output is a record, and consumes `self`.
    pub fn into_nonce(self) -> Option<Group<N>> {
        match self {
            Output::Record(_, _, Some(record)) => Some(record.into_nonce()),
            _ => None,
        }
    }

    /// Returns the checksum, if the output is a record.
    pub const fn checksum(&self) -> Option<&Field<N>> {
        match self {
            Output::Record(_, checksum, ..) => Some(checksum),
            _ => None,
        }
    }

    /// Returns the checksum, if the output is a record, and consumes `self`.
    pub fn into_checksum(self) -> Option<Field<N>> {
        match self {
            Output::Record(_, checksum, ..) => Some(checksum),
            _ => None,
        }
    }

    /// Returns the public verifier inputs for the proof.
    pub fn verifier_inputs(&self) -> impl '_ + Iterator<Item = N::Field> {
        // Append the output ID.
        [**self.id()].into_iter()
            // Append the checksum if it exists.
            .chain([self.checksum().map(|sum| **sum)].into_iter().flatten())
    }

    /// Returns `true` if the output is well-formed.
    /// If the optional value exists, this method checks that it hashes to the output ID.
    pub fn verify(&self) -> bool {
        // Ensure the hash of the value (if the value exists) is correct.
        let result = match self {
            Output::Constant(hash, Some(value)) => match N::hash_bhp1024(&value.to_bits_le()) {
                Ok(candidate_hash) => Ok(hash == &candidate_hash),
                Err(error) => Err(error),
            },
            Output::Public(hash, Some(value)) => match N::hash_bhp1024(&value.to_bits_le()) {
                Ok(candidate_hash) => Ok(hash == &candidate_hash),
                Err(error) => Err(error),
            },
            Output::Private(hash, Some(value)) => match N::hash_bhp1024(&value.to_bits_le()) {
                Ok(candidate_hash) => Ok(hash == &candidate_hash),
                Err(error) => Err(error),
            },
            Output::Record(_, checksum, Some(value)) => match N::hash_bhp1024(&value.to_bits_le()) {
                Ok(candidate_hash) => Ok(checksum == &candidate_hash),
                Err(error) => Err(error),
            },
            _ => Ok(true),
        };

        match result {
            Ok(is_hash_valid) => is_hash_valid,
            Err(error) => {
                eprintln!("{error}");
                false
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::*;
    use console::{network::Testnet3, program::Literal};

    type CurrentNetwork = Testnet3;

    /// Sample the transition outputs.
    pub(crate) fn sample_outputs() -> Vec<(<CurrentNetwork as Network>::TransitionID, Output<CurrentNetwork>)> {
        // Sample a transition.
        let transaction = crate::ledger::vm::test_helpers::sample_execution_transaction();
        let transition = transaction.transitions().next().unwrap();

        // Retrieve the transition ID and input.
        let transition_id = *transition.id();
        let input = transition.outputs().iter().next().unwrap().clone();

        // Initialize the RNG.
        let rng = &mut test_crypto_rng();

        // Sample a random plaintext.
        let plaintext = Plaintext::Literal(Literal::Field(Uniform::rand(rng)), Default::default());
        let plaintext_hash = CurrentNetwork::hash_bhp1024(&plaintext.to_bits_le()).unwrap();
        // Sample a random ciphertext.
        let ciphertext = Ciphertext::from_fields(&vec![Uniform::rand(rng); 10]).unwrap();
        let ciphertext_hash = CurrentNetwork::hash_bhp1024(&ciphertext.to_bits_le()).unwrap();
        // Sample a random record.
        let randomizer = Uniform::rand(rng);
        let nonce = CurrentNetwork::g_scalar_multiply(&randomizer);
        let record = Record::<CurrentNetwork, Plaintext<CurrentNetwork>>::from_str(
            &format!("{{ owner: aleo1d5hg2z3ma00382pngntdp68e74zv54jdxy249qhaujhks9c72yrs33ddah.private, gates: 5u64.private, token_amount: 100u64.private, _nonce: {nonce}.public }}"),
        ).unwrap();
        let record_ciphertext = record.encrypt(randomizer).unwrap();

        vec![
            (transition_id, input),
            (Uniform::rand(rng), Output::Constant(Uniform::rand(rng), None)),
            (Uniform::rand(rng), Output::Constant(plaintext_hash, Some(plaintext.clone()))),
            (Uniform::rand(rng), Output::Public(Uniform::rand(rng), None)),
            (Uniform::rand(rng), Output::Public(plaintext_hash, Some(plaintext))),
            (Uniform::rand(rng), Output::Private(Uniform::rand(rng), None)),
            (Uniform::rand(rng), Output::Private(ciphertext_hash, Some(ciphertext))),
            (Uniform::rand(rng), Output::Record(Uniform::rand(rng), Uniform::rand(rng), None)),
            (Uniform::rand(rng), Output::Record(Uniform::rand(rng), Uniform::rand(rng), Some(record_ciphertext))),
            (Uniform::rand(rng), Output::ExternalRecord(Uniform::rand(rng))),
        ]
    }
}
