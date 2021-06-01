use ckb_hash::blake2b_256;
use ckb_sdk::HumanCapacity;
use hex;
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

pub fn blake256_to_byte32(hash: &str) -> Result<[u8; 32]> {
    Ok(hex::decode(hash)?.try_into().expect("transport hex to byte32"))
}

pub fn blake160_to_byte20(hash: &str) -> Result<[u8; 20]> {
    Ok(hex::decode(hash)?.try_into().expect("transport hex to byte20"))
}

pub fn blake160(data: &[u8]) -> [u8; 20] {
    let mut buf = [0u8; 20];
    let hash = blake2b_256(data);
    buf.clone_from_slice(&hash[..20]);
    buf
}

pub async fn outpoint_to_output(outpoint: OutPoint) -> Result<CellOutput> {
    let tx = rpc::get_transaction(outpoint.tx_hash()).await?;
    let out_index: u32 = outpoint.index().unpack();
    let output = tx
        .raw()
        .outputs()
        .get(out_index as usize)
        .ok_or_else(|| anyhow!("index is out-of-bound in transaction outputs"))?;
    Ok(output)
}

pub fn fee(fee: &str) -> Capacity {
    let fee = HumanCapacity::from_str(fee).unwrap().0;
    Capacity::shannons(fee)
}

pub fn nft_script(script_args: Vec<u8>) -> Script {
    Script::new_builder()
        .code_hash(_C.nft.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(script_args).pack())
        .build()
}

pub fn wallet_script(script_args: Vec<u8>) -> Script {
    Script::new_builder()
        .code_hash(_C.wallet.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(script_args).pack())
        .build()
}

pub fn payment_script(script_args: Vec<u8>) -> Script {
    Script::new_builder()
        .code_hash(_C.payment.code_hash.clone())
        .hash_type(ScriptHashType::Data.into())
        .args(Bytes::from(script_args).pack())
        .build()
}

pub fn sighash_script_with_lockargs(lock_args: &[u8]) -> Script {
    _G.sighash_script
        .clone()
        .as_builder()
        .args(Bytes::from(lock_args.to_vec()).pack())
        .build()
}
