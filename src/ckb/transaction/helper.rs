use ckb_types::{
    prelude::*, bytes::Bytes,
    core::{
        DepType, TransactionView, Capacity
    },
    packed::{
        Byte32, OutPoint, CellDep, Script, CellInput, CellOutput
    }
};
use anyhow::{
    Result, anyhow
};
use crate::ckb::{
    transaction::genesis::GENESIS as _G, rpc::methods as rpc
};
use std::{
    str::FromStr, convert::TryInto
};
use ckb_sdk::HumanCapacity;
use hex;

pub fn hex_to_byte32(hash: &str) -> Result<Byte32> {
    let hash: [u8; 32] = hex::decode(hash)?.try_into().expect("transport hex to byte32");
    Ok(Byte32::new(hash))
}

pub async fn outpoint_to_output(outpoint: &OutPoint) -> Result<CellOutput> {
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

pub fn add_sighash_celldep(tx: TransactionView) -> TransactionView {
    tx
        .as_advanced_builder()
        .cell_dep(_G.sighash_celldep.clone())
        .build()
}

pub fn add_multisig_celldep(tx: TransactionView) -> TransactionView {
    tx
        .as_advanced_builder()
        .cell_dep(_G.multisig_celldep.clone())
        .build()
}

pub fn add_code_celldep(tx: TransactionView, tx_hash: Byte32) -> TransactionView {
    let outpoint = OutPoint::new(tx_hash, 0);
    let celldep  = CellDep::new_builder()
        .out_point(outpoint)
        .dep_type(DepType::Code.into())
        .build();
    tx
        .as_advanced_builder()
        .cell_dep(celldep)
        .build()
}

pub fn sighash_script_with_lockargs(lock_args: &[u8]) -> Script {
    _G.sighash_script
        .clone()
        .as_builder()
        .args(Bytes::from(lock_args.to_vec()).pack())
        .build()
}

pub async fn complete_tx_with_sighash_cells(tx: TransactionView, pubkey_hash: [u8; 20], fee: Capacity) -> Result<TransactionView> {
    // determin current minimum capacity from transaction's outputs
    let mut required_capacity = fee;
    for output in tx.outputs().into_iter() {
        let output_capacity = Capacity::shannons(output.capacity().unpack());
        required_capacity = required_capacity.safe_add(output_capacity)?;
    }
    // prepare secp256k1 cells until required capacity is reached
    let mut offered_capacity = Capacity::zero();
    for input in tx.inputs().into_iter() {
        let input = outpoint_to_output(&input.previous_output()).await?;
        let input_capacity = Capacity::shannons(input.capacity().unpack());
        offered_capacity = offered_capacity.safe_add(input_capacity)?;
    }
    let mut cursor = None;
    let mut tx_inputs = vec![];
    while offered_capacity.as_u64() < required_capacity.as_u64() {
        let live_cells = rpc::get_secp256k1_live_cells(&pubkey_hash[..], 5, cursor).await?;
        let mut inputs = live_cells
            .objects
            .iter()
            .filter(|cell| {
                if offered_capacity.as_u64() >= required_capacity.as_u64() {
                    return false;
                }
                let input_capacity = Capacity::shannons(cell.output.capacity().unpack());
                offered_capacity = offered_capacity.safe_add(input_capacity).expect("offered add input");
                return true;
            })
            .map(|cell| {
                CellInput::new_builder()
                    .previous_output(cell.out_point.clone())
                    .build()
            })
            .collect::<Vec<CellInput>>();
        tx_inputs.append(&mut inputs);
        if live_cells.last_cursor.is_empty() {
            break;
        } 
        cursor = Some(live_cells.last_cursor);
    }
    if offered_capacity.as_u64() < required_capacity.as_u64() {
        return Err(anyhow!("required live secp256k1 cells are NOT enough"));
    }
    // prepare secp256k1 output cells to contain extra capacity
    let mut tx_outputs = vec![];
    let mut tx_outputs_data = vec![];
    if offered_capacity.as_u64() > required_capacity.as_u64() {
        let secp256k1_script = sighash_script_with_lockargs(&pubkey_hash[..]);
        let extra_capacity = offered_capacity.as_u64() - required_capacity.as_u64();
        let output = CellOutput::new_builder()
            .lock(secp256k1_script)
            .capacity(extra_capacity.pack())
            .build();
        tx_outputs.push(output);
        tx_outputs_data.push(Bytes::new());
    }
    // generate new transaction
    let tx = tx
        .as_advanced_builder()
        .inputs(tx_inputs)
        .outputs(tx_outputs)
        .outputs_data(tx_outputs_data.pack())
        .build();
    let tx = add_sighash_celldep(tx);
    Ok(tx)
}
