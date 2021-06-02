use ckb_types::{
    prelude::*, bytes::Bytes,
    core::{
        DepType, TransactionView, Capacity, HeaderView
    },
    packed::{
        OutPoint, CellDep, CellInput, CellOutput
    }
};
use anyhow::{
    Result, anyhow
};
use crate::ckb::{
    transaction::genesis::GENESIS as _G,
    rpc::{
        methods as rpc,
        types::{
            ScriptType, SearchKey
        }
    }
};
use super::utils::*;

// add sighash_blake160 cell deps into [tx] which represents the basic lock script for ckb
pub fn add_sighash_celldep(tx: TransactionView) -> TransactionView {
    return tx
        .as_advanced_builder()
        .cell_dep(_G.sighash_celldep.clone())
        .build()
}

// add multisig cell deps into [tx] which helps check signature from multi-parts
pub fn add_multisig_celldep(tx: TransactionView) -> TransactionView {
    return tx
        .as_advanced_builder()
        .cell_dep(_G.multisig_celldep.clone())
        .build()
}

// add custom [outpoint] as a cell dep into [tx]
pub fn add_code_celldep(tx: TransactionView, outpoint: OutPoint) -> TransactionView {
    let celldep  = CellDep::new_builder()
        .out_point(outpoint)
        .dep_type(DepType::Code.into())
        .build();
    return tx
        .as_advanced_builder()
        .cell_dep(celldep)
        .build()
}

// add [header] as a header dep into [tx]
pub fn add_headerdep(tx: TransactionView, header: HeaderView) -> TransactionView {
    return tx
        .as_advanced_builder()
        .header_dep(header.hash())
        .build()
}

// the original inputs and outputs from [tx] may not be valid for ckb "capaicity checking", so there should be a
// function to handle this, the function complete_tx_with_sighash_cells will search and add normal sighash_blake160
// cells into inputs from [tx] to expand capacity in input part, and then generate new sighash_blake160 cells into
// outputs to receive the remain capacity (already subtracts [fee]) for next use.
//
// the script_args from every sighash_blake160 cells from inputs and outputs are all filled with [pubkey_hash]
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
        let input = outpoint_to_output(input.previous_output()).await?;
        let input_capacity = Capacity::shannons(input.capacity().unpack());
        offered_capacity = offered_capacity.safe_add(input_capacity)?;
    }
    let mut cursor = None;
    let mut tx_inputs = vec![];
    let secp256k1_script = sighash_script_with_lockargs(&pubkey_hash[..]);
    while offered_capacity.as_u64() < required_capacity.as_u64() {
        let search_key = SearchKey::new(secp256k1_script.clone().into(), ScriptType::Lock);
        let live_cells = rpc::get_live_cells(search_key, 5, cursor).await?;
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
