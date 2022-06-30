//! Evm circuit benchmarks
use eth_types::Field;
use halo2_proofs::{
    circuit::{floor_planner::V1, AssignedCell, Layouter},
    plonk::{Circuit, ConstraintSystem, Error},
};
use keccak256::{circuit::KeccakConfig, common::NEXT_INPUTS_LANES, keccak_arith::KeccakFArith};

#[derive(Default, Clone)]
struct KeccakTestCircuit {
    input: Vec<Vec<u8>>,
    output: [u8; 32],
}

impl<F: Field> Circuit<F> for KeccakTestCircuit {
    type Config = KeccakConfig<F>;
    type FloorPlanner = V1;

    fn without_witnesses(&self) -> Self {
        self.clone()
    }

    fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
        Self::Config::configure(meta)
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<F>,
    ) -> Result<(), Error> {
        // Load the table
        config.load(&mut layouter)?;
        let mut config = config.clone();

        for input in self.input.iter() {
            config.assign_hash(&mut layouter, input.as_slice(), self.output)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_std::{end_timer, start_timer};
    use halo2_proofs::plonk::{create_proof, keygen_pk, keygen_vk, verify_proof, SingleVerifier};
    use halo2_proofs::{
        pairing::bn256::{Bn256, Fr, G1Affine},
        poly::commitment::{Params, ParamsVerifier},
        transcript::{Blake2bRead, Blake2bWrite, Challenge255},
    };
    use itertools::Itertools;
    use keccak256::common::PERMUTATION;
    use keccak256::{
        arith_helpers::*,
        common::{State, ROUND_CONSTANTS},
        gate_helpers::biguint_to_f,
    };
    use rand::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use std::env::var;

    #[test]
    fn bench_keccak_round() {
        let input = vec![
            vec![
                65u8, 108, 105, 99, 101, 32, 119, 97, 115, 32, 98, 101, 103, 105, 110, 110, 105,
                110, 103, 32, 116, 111, 32, 103, 101, 116, 32, 118, 101, 114, 121, 32, 116, 105,
                114, 101, 100, 32, 111, 102, 32, 115, 105, 116, 116, 105, 110, 103, 32, 98, 121,
                32, 104, 101, 114, 32, 115, 105, 115, 116, 101, 114, 32, 111, 110, 32, 116, 104,
                101, 32, 98, 97, 110, 107, 44, 32, 97, 110, 100, 32, 111, 102, 32, 104, 97, 118,
                105, 110, 103, 32, 110, 111, 116, 104, 105, 110, 103, 32, 116, 111, 32, 100, 111,
                58, 32, 111, 110, 99, 101, 32, 111, 114, 32, 116, 119, 105, 99, 101, 32, 115, 104,
                101, 32, 104, 97, 100, 32, 112, 101, 101, 112, 101, 100, 32, 105, 110, 116, 111,
                32, 116, 104, 101, 32, 98, 111, 111, 107, 32, 104, 101, 114, 32, 115, 105, 115,
                116, 101, 114, 32, 119, 97, 115, 32, 114, 101, 97, 100, 105, 110, 103, 44, 32, 98,
                117, 116, 32, 105, 116, 32, 104, 97, 100, 32, 110, 111, 32, 112, 105, 99, 116, 117,
                114, 101, 115, 32, 111, 114, 32, 99, 111, 110, 118, 101, 114, 115, 97, 116, 105,
                111, 110, 115, 32, 105, 110, 32, 105, 116, 44, 32, 97, 110, 100, 32, 119, 104, 97,
                116, 32, 105, 115, 32, 116, 104, 101, 32, 117, 115, 101, 32, 111, 102, 32, 97, 32,
                98, 111, 111, 107, 44, 32, 116, 104, 111, 117, 103, 104, 116, 32, 65, 108, 105, 99,
                101, 32, 119, 105, 116, 104, 111, 117, 116, 32, 112, 105, 99, 116, 117, 114, 101,
                115, 32, 111, 114, 32, 99, 111, 110, 118, 101, 114, 115, 97, 116, 105, 111, 110,
                115, 63,
            ];
            3000
        ];
        let output = [
            60u8, 227, 142, 8, 143, 135, 108, 85, 13, 254, 190, 58, 30, 106, 153, 194, 188, 6, 208,
            49, 16, 102, 150, 120, 100, 130, 224, 177, 64, 98, 53, 252,
        ];

        let constants_b13: Vec<Fr> = ROUND_CONSTANTS
            .iter()
            .map(|num| biguint_to_f(&convert_b2_to_b13(*num)))
            .collect();

        let constants_b9: Vec<Fr> = ROUND_CONSTANTS
            .iter()
            .map(|num| biguint_to_f(&convert_b2_to_b9(*num)))
            .collect();

        // Build the circuit
        let circuit = KeccakTestCircuit { input, output };

        let degree: u32 = var("DEGREE")
            .expect("No DEGREE env var was provided")
            .parse()
            .expect("Cannot parse DEGREE env var as u32");

        let rng = XorShiftRng::from_seed([
            0x59, 0x62, 0xbe, 0x5d, 0x76, 0x3d, 0x31, 0x8d, 0x17, 0xdb, 0x37, 0x32, 0x54, 0x06,
            0xbc, 0xe5,
        ]);

        // Bench setup generation
        let setup_message = format!("Setup generation with degree = {}", degree);
        let start1 = start_timer!(|| setup_message);
        let general_params: Params<G1Affine> = Params::<G1Affine>::unsafe_setup::<Bn256>(degree);
        end_timer!(start1);

        let vk = keygen_vk(&general_params, &circuit).unwrap();
        let pk = keygen_pk(&general_params, vk, &circuit).unwrap();

        // Prove
        let mut transcript = Blake2bWrite::<_, _, Challenge255<_>>::init(vec![]);

        // Bench proof generation time
        let proof_message = format!("Keccak Proof generation with {} degree", degree);
        let start2 = start_timer!(|| proof_message);
        create_proof(
            &general_params,
            &pk,
            &[circuit],
            &[&[constants_b9.as_slice(), constants_b13.as_slice()]],
            rng,
            &mut transcript,
        )
        .unwrap();
        let proof = transcript.finalize();
        end_timer!(start2);

        // Verify
        let verifier_params: ParamsVerifier<Bn256> =
            general_params.verifier(PERMUTATION * 2).unwrap();
        let mut verifier_transcript = Blake2bRead::<_, _, Challenge255<_>>::init(&proof[..]);
        let strategy = SingleVerifier::new(&verifier_params);

        // Bench verification time
        let start3 = start_timer!(|| "Keccak Proof verification");
        verify_proof(
            &verifier_params,
            pk.get_vk(),
            strategy,
            &[],
            &mut verifier_transcript,
        )
        .unwrap();
        end_timer!(start3);
    }
}
