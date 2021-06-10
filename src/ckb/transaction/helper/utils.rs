use ckb_sdk::HumanCapacity;
use hex;
use ckb_crypto::secp::Privkey;
use ckb_hash::{
    blake2b_256, new_blake2b
};
use ckb_types::{
    prelude::*, bytes::Bytes,
    core::{
        Capacity, ScriptHashType,
    },
    packed::{
        OutPoint, Script, CellOutput
    }
};
use anyhow::{
    Result, anyhow
};
use std::{
    str::FromStr, convert::TryInto
};
use crate::{
    config::VARS as _C,
    ckb::{
        transaction::genesis::GENESIS as _G, rpc::methods as rpc,
    }
};

// turn a hex format of blake2b_256 [hash] into [u8; 32] format
pub fn blake256_to_byte32(hash: &str) -> Result<[u8; 32]> {
    Ok(hex::decode(hash)?.try_into().expect("transport hex to byte32"))
}

// turn a hex format of blake2b_160 [hash] into [u8; 20] format
pub fn blake160_to_byte20(hash: &str) -> Result<[u8; 20]> {
    Ok(hex::decode(hash)?.try_into().expect("transport hex to byte20"))
}

// apply blake2b hasher on [data] and remain the first 20 bytes of the result
pub fn blake160(data: &[u8]) -> [u8; 20] {
    let mut buf = [0u8; 20];
    let hash = blake2b_256(data);
    buf.clone_from_slice(&hash[..20]);
    buf
}

// find and remove the intersection from two nft collections
pub fn blake160_intersect(nfts1: &mut Vec<[u8; 20]>, nfts2: &mut Vec<[u8; 20]>) -> Vec<[u8; 20]> {
    let mut inter_nfts = vec![];
    *nfts1 = nfts1
        .iter()
        .filter_map(|&nft1| {
            let mut i2 = 0;
            if nfts2.iter().enumerate().any(|(i, nft2)| { i2 = i; nft1[..] == nft2[..] }) {
                inter_nfts.push(nfts2.remove(i2));
                None
            } else {
                Some(nft1)
            }
        })
        .collect();
    inter_nfts
}

// search the transaction hash from [outpoint] and find the complete transaction info on chain
pub fn outpoint_to_output(outpoint: OutPoint) -> Result<CellOutput> {
    let tx = rpc::get_transaction(outpoint.tx_hash())?;
    let out_index: u32 = outpoint.index().unpack();
    let output = tx
        .raw()
        .outputs()
        .get(out_index as usize)
        .ok_or_else(|| anyhow!("index is out-of-bound in transaction outputs"))?;
    Ok(output)
}

// transform a private secret key to the hash format of its public key
pub fn privkey_to_pkhash(privkey: &Privkey) -> [u8; 20] {
    let pubkey = privkey.pubkey().expect("private key to public key");
    let mut hasher = new_blake2b();
    hasher.update(pubkey.serialize().as_slice());
    let mut pubkey_hash = [0u8; 32];
    hasher.finalize(&mut pubkey_hash);
    pubkey_hash[..20].try_into().unwrap()
}

// turn a human-readable string into ckb capacity format 
pub fn fee(fee: &str) -> Capacity {
    let fee = HumanCapacity::from_str(fee).unwrap().0;
    Capacity::shannons(fee)
}

// get a nft contract script data with [script_args] fills into args part
pub fn nft_script(script_args: Vec<u8>) -> Script {
    Script::new_builder()
        .code_hash(_C.nft.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(script_args).pack())
        .build()
}

// get a wallet (or ownerlock) contract script data with [script_args] fills into args part
pub fn wallet_script(script_args: Vec<u8>) -> Script {
    Script::new_builder()
        .code_hash(_C.wallet.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(script_args).pack())
        .build()
}

// get a payment contract script data with [script_args] fills into args part
pub fn payment_script(script_args: Vec<u8>) -> Script {
    Script::new_builder()
        .code_hash(_C.payment.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(script_args).pack())
        .build()
}

// get a kabletop (or game) contract script data with [script_args] fills into args part
pub fn kabletop_script(script_args: Vec<u8>) -> Script {
    Script::new_builder()
        .code_hash(_C.kabletop.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(script_args).pack())
        .build()
}

// get a sighash_blake160 script with [lock_args] fills into args part
pub fn sighash_script(lock_args: &[u8]) -> Script {
    _G.sighash_script
        .clone()
        .as_builder()
        .args(Bytes::from(lock_args.to_vec()).pack())
        .build()
}
