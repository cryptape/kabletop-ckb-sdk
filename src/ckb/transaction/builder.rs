use ckb_types::{
    bytes::Bytes, prelude::*, H256,
    core::{
        TransactionBuilder, TransactionView, ScriptHashType, Capacity
    },
    packed::{
        self, CellOutput, Script, WitnessArgs
    }
};
use ckb_hash::{
    blake2b_256, new_blake2b
};
use crate::{
    config::VARS as _C, ckb::transaction::helper
};
use anyhow::Result;
use std::convert::TryInto;
use ckb_crypto::secp::Privkey;
use hex;

fn build_nft_config(price: u64, count: u8, config: Vec<([u8; 20], u16)>) -> Bytes {
    let mut data = vec![];
    data.append(&mut price.to_le_bytes().to_vec());
    data.append(&mut count.to_le_bytes().to_vec());
    for &(nft, rate) in config.iter() {
        data.append(&mut nft.to_vec());
        data.append(&mut rate.to_le_bytes().to_vec());
    }
    Bytes::from(data)
}

fn default_nfts() -> Vec<([u8; 20], u16)> {
    vec![
        (blake160(&[1u8]), 936),
        (blake160(&[2u8]), 25456),
        (blake160(&[3u8]), 26771),
        (blake160(&[4u8]), 27034),
        (blake160(&[5u8]), 30470),
        (blake160(&[6u8]), 62600),
    ]
}

fn blake160(data: &[u8]) -> [u8; 20] {
    let mut buf = [0u8; 20];
    let hash = blake2b_256(data);
    buf.clone_from_slice(&hash[..20]);
    buf
}

fn sign_tx(tx: TransactionView, key: &Privkey) -> TransactionView {
    const SIGNATURE_SIZE: usize = 65;
    let witnesses_len = tx.witnesses().len();
    let tx_hash = tx.hash();
    let mut signed_witnesses: Vec<packed::Bytes> = Vec::new();
    let mut blake2b = new_blake2b();
    let mut message = [0u8; 32];
    blake2b.update(&tx_hash.raw_data());
    // digest the first witness
    let witness = WitnessArgs::default();
    let zero_lock: Bytes = {
        let mut buf = Vec::new();
        buf.resize(SIGNATURE_SIZE, 0);
        buf.into()
    };
    let witness_for_digest = witness
        .clone()
        .as_builder()
        .lock(Some(zero_lock).pack())
        .build();
    let witness_len = witness_for_digest.as_bytes().len() as u64;
    blake2b.update(&witness_len.to_le_bytes());
    blake2b.update(&witness_for_digest.as_bytes());
    blake2b.finalize(&mut message);
    let message = H256::from(message);
    let sig = key.sign_recoverable(&message).expect("sign");
    signed_witnesses.push(
        witness
            .as_builder()
            .lock(Some(Bytes::from(sig.serialize())).pack())
            .build()
            .as_bytes()
            .pack(),
    );
    for i in 1..witnesses_len {
        signed_witnesses.push(tx.witnesses().get(i).unwrap());
    }
    tx.as_advanced_builder()
        .set_witnesses(signed_witnesses)
        .build()
}

pub async fn make_tx_compose_nft() -> Result<TransactionView> {
    // prepare pubkey blake160
    let pubkey_hash: [u8; 20] = hex::decode("58b85c196e5fe80e25b4dab596e7121d219f79fb")?.try_into().unwrap();
    
    // prepare output data
    let output_data = build_nft_config(100, 5, default_nfts());

    // prepare output
    let wallet_script = Script::new_builder()
        .code_hash(_C.wallet.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(pubkey_hash.to_vec()).pack())
        .build();
    
    let payment_script = Script::new_builder()
        .code_hash(_C.payment.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(pubkey_hash.to_vec()).pack())
        .build();
    
    let capacity = Capacity::bytes(output_data.len())?;
    let output = CellOutput::new_builder()
        .lock(wallet_script)
        .type_(Some(payment_script).pack())
        .build_exact_capacity(capacity)?;

    // prepare tx
    let tx = TransactionBuilder::default()
        .output(output)
        .output_data(output_data.pack())
        .build();

    // complete tx
    let tx = helper::complete_tx_with_sighash_cells(tx, pubkey_hash, helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, _C.payment.tx_hash.clone());
    let tx = helper::add_code_celldep(tx, _C.wallet.tx_hash.clone());

    // sign tx
    let hash: [u8; 32] = hex::decode("8d929e962f940f75aa32054f19a5ea2ce70ae30bfe4ff7cf2dbed70d556265df")?.try_into().unwrap();
    let privkey = Privkey::from(H256(hash));
    let tx = sign_tx(tx, &privkey);

    Ok(tx)
}

#[cfg(test)]
mod test {
    use ckb_sdk::rpc::HttpRpcClient;
    use futures::executor::block_on;
    use crate::{
        config::VARS as _C, ckb::transaction::builder
    };

    #[test]
    fn test_make_tx_compose_nft() {
        let mut ckb_rpc = HttpRpcClient::new(String::from(_C.common.ckb_uri.clone()));
        let tx = block_on(builder::make_tx_compose_nft()).expect("compose nft");
        match ckb_rpc.send_transaction(tx.data()) {
            Ok(tx_hash) => println!("success: {:?}", hex::encode(tx_hash.as_bytes())),
            Err(err)    => println!("failure: {:?}", err)
        }
    }
}
