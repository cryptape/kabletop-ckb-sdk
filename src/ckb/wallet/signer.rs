use ckb_types::{
    bytes::Bytes, prelude::*, H256, core::TransactionView,
    packed::{
        self, WitnessArgs, Byte32, CellOutput
    }
};
use ckb_hash::new_blake2b;
use ckb_crypto::secp::Privkey;
use crate::ckb::transaction::helper;
use std::collections::HashMap;

// sign a whole [tx] using private [key], the [extra_witnesses] is some external args which just placed into witness part
// the function just supposes two or more cells that are in one group are all close together
pub fn sign(
    tx: TransactionView, key: &Privkey, extra_witnesses: Vec<WitnessArgs>, enable_sign: &dyn Fn(&CellOutput) -> bool
) -> TransactionView {
    let inputs = tx
        .inputs()
        .into_iter()
        .map(|input| helper::outpoint_to_output(input.previous_output()).expect("sign"))
        .collect::<Vec<_>>();
	let mut last_lockhashes: HashMap<Byte32, (WitnessArgs, usize, Vec<packed::Bytes>)> = HashMap::new();
    let mut signed_witnesses = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
			let mut witness = {
				if let Some(witness) = tx.witnesses().get(i) {
					witness
				} else {
					Bytes::new().pack()
				}
			};
            if enable_sign(input) {
				let lockhash = input.lock().calc_script_hash();
                if let Some((_, _, group_witnesses)) = last_lockhashes.get_mut(&lockhash) {
					group_witnesses.push(witness.clone());
                } else {
                    let witness_args = {
                        if witness.as_slice() == Bytes::new().pack().as_slice() {
                            WitnessArgs::default()
                        } else {
                            let witness: Bytes = witness.unpack();
                            WitnessArgs::from_slice(witness.to_vec().as_slice()).unwrap_or_default()
                        }
                    };
                    last_lockhashes.insert(lockhash, (witness_args, i, vec![]));
                    witness = Bytes::new().pack();
                }
            }
			witness
        })
        .collect::<Vec<_>>();
	for (_, (witness, i, group_witnesses)) in last_lockhashes {
		signed_witnesses[i] = sign_input(tx.hash(), key, &witness, &group_witnesses, &extra_witnesses)
	}
    let mut extra_witnesses = extra_witnesses
        .iter()
        .map(|witness| witness.as_bytes().pack())
        .collect::<Vec<_>>();
    signed_witnesses.append(&mut extra_witnesses);
    tx.as_advanced_builder()
        .set_witnesses(signed_witnesses)
        .build()
}

// sign the every single input data in [tx] and get the signed bytes
fn sign_input(
    tx_hash: Byte32, key: &Privkey, witness: &WitnessArgs, group_witnesses: &Vec<packed::Bytes>, extra_witnesses: &Vec<WitnessArgs>
) -> packed::Bytes {
    let mut blake2b = new_blake2b();
    blake2b.update(&tx_hash.raw_data());
    let signed_witness = witness
        .clone()
        .as_builder()
        .lock(Some(Bytes::from(vec![0u8; 65])).pack())
        .build();
    let witness_len = signed_witness.as_bytes().len() as u64;
    blake2b.update(&witness_len.to_le_bytes());
    blake2b.update(&signed_witness.as_bytes());
    for group_witness in group_witnesses {
        let witness_len = group_witness.raw_data().len() as u64;
        blake2b.update(&witness_len.to_le_bytes());
        blake2b.update(&group_witness.raw_data().to_vec());
    }
    for extra_witness in extra_witnesses {
        let witness_len = extra_witness.as_bytes().len() as u64;
        blake2b.update(&witness_len.to_le_bytes());
        blake2b.update(&extra_witness.as_bytes());
    }
    let mut digest = [0u8; 32];
    blake2b.finalize(&mut digest);
    let signature = key.sign_recoverable(&H256::from(digest)).expect("sign tx");
    signed_witness
        .as_builder()
        .lock(Some(Bytes::from(signature.serialize())).pack())
        .build()
        .as_bytes()
        .pack()
}
