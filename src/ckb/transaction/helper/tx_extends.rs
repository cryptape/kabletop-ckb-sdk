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
use crate::{
	config::VARS as _C,
	ckb::{
		transaction::genesis::GENESIS as _G,
		rpc::{
			methods as rpc,
			types::{
				ScriptType, SearchKey
			}
		}
	}
};
use super::utils::*;

// add sighash_blake160 cell deps into [tx] which represents the basic lock script for ckb
pub fn add_sighash_celldep(mut tx: TransactionView) -> TransactionView {
    let celldep = tx
        .cell_deps_iter()
        .find(|dep| dep.out_point() == _G.sighash_celldep.out_point());
    if celldep.is_none() {
        tx = tx
            .as_advanced_builder()
            .cell_dep(_G.sighash_celldep.clone())
            .build();
    }
    tx
}

// add multisig cell deps into [tx] which helps check signature from multi-parts
pub fn add_multisig_celldep(mut tx: TransactionView) -> TransactionView {
    let celldep = tx
        .cell_deps_iter()
        .find(|dep| dep.out_point() == _G.multisig_celldep.out_point());
    if celldep.is_none() {
        tx = tx
            .as_advanced_builder()
            .cell_dep(_G.multisig_celldep.clone())
            .build();
    }
    tx
}

// add custom [outpoint] as a cell dep into [tx]
pub fn add_code_celldep(mut tx: TransactionView, outpoint: OutPoint) -> TransactionView {
    let celldep = tx
        .cell_deps_iter()
        .find(|dep| dep.out_point() == outpoint);
    if celldep.is_none() {
        let celldep  = CellDep::new_builder()
            .out_point(outpoint)
            .dep_type(DepType::Code.into())
            .build();
        tx = tx.as_advanced_builder()
            .cell_dep(celldep)
            .build();
    }
    tx
}

// add [header] as a header dep into [tx]
pub fn add_headerdep(mut tx: TransactionView, header: HeaderView) -> TransactionView {
    let headerdep = tx
        .header_deps_iter()
        .find(|hash| hash.raw_data() == header.hash().raw_data());
    if headerdep.is_none() {
        tx = tx
            .as_advanced_builder()
            .header_dep(header.hash())
            .build();
    }
    tx
}

// the original inputs and outputs from [tx] may not be valid for ckb "capaicity checking", so there should be a
// function to handle this, the function complete_tx_with_sighash_cells will search and add normal sighash_blake160
// cells into inputs from [tx] to expand capacity in input part, and then generate new sighash_blake160 cells into
// outputs to receive the remain capacity (already subtracts [fee]) for next use.
//
// the script_args from every sighash_blake160 cells from inputs and outputs are all filled with [pubkey_hash]
pub async fn complete_tx_with_sighash_cells(tx: TransactionView, pubkey_hash: &[u8; 20], fee: Capacity) -> Result<TransactionView> {
    // determin current minimum capacity from transaction's outputs
    let required_capacity = fee.safe_add(tx.outputs().total_capacity()?)?;
    // prepare secp256k1 cells until required capacity is reached
    let mut offered_capacity = Capacity::zero();
    for input in tx.inputs().into_iter() {
        let input = outpoint_to_output(input.previous_output())?;
        let input_capacity = Capacity::shannons(input.capacity().unpack());
        offered_capacity = offered_capacity.safe_add(input_capacity)?;
    }
    let mut cursor = None;
    let mut tx_inputs = vec![];
    let secp256k1_script = sighash_script(&pubkey_hash[..]);
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

// collect and apply nft cells locked by [pubkey_hash] to [tx], the APPLY means put collected nft cells into input part
// and transfer them all into output part, the tx won't obtain any nft cells if [destory] is false
//
// all nft cells are collected by [nfts]
pub async fn complete_tx_with_nft_cells(
    tx: TransactionView, user_pkhash: &[u8; 20], composer_pkhash: &[u8; 20], mut required_nfts: Vec<[u8; 20]>, discard: bool
) -> Result<TransactionView> {
    let lock_script = sighash_script(&user_pkhash[..]);
    let type_script = {
        let wallet = wallet_script(composer_pkhash.to_vec());
        nft_script(wallet.calc_script_hash().raw_data().to_vec())
    };

    // search live nft cells using serach_key
    let mut cursor = None;
    let mut tx_inputs = vec![];
    let mut tx_output_data = vec![];
    let mut capacity = 0u64;
    while !required_nfts.is_empty() {
        let search_key = SearchKey::new(lock_script.clone().into(), ScriptType::Lock).filter(type_script.clone().into());
        let live_cells = rpc::get_live_cells(search_key, 10, cursor).await?;
        let mut inputs = live_cells.objects
            .iter()
            .filter(|cell| {
                let mut data = cell.output_data.to_vec();
                let mut nft = [0u8; 20];
                let mut nfts = vec![];
                let n = data.len() / 20;
                for _ in 0..n {
                    nft.copy_from_slice(&data[..20]);
                    data = data[20..].to_vec();
                    nfts.push(nft);
                }
				let mut intersected = false;
				if blake160_intersect(&mut nfts, &mut required_nfts).len() > 0 {
					// check whether destory shared nfts
					if discard {
						data = vec![];
						nfts.iter().for_each(|nft| data.append(&mut nft.to_vec()));
					} else {
						data = cell.output_data.to_vec();
					}
					tx_output_data.append(&mut data);
					intersected = true;
				}
				intersected
            })
            .map(|cell| {
                let ckb: Capacity = cell.output.capacity().unpack();
                capacity += ckb.as_u64();
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
    if required_nfts.len() > 0 {
        return Err(anyhow!("all owned nft cells cannot cover required nfts ({} left)", required_nfts.len()));
    }

    // turn all searched nft cells into one output cell
    let tx_output = CellOutput::new_builder()
        .lock(lock_script)
        .type_(Some(type_script).pack())
        .capacity(Capacity::shannons(capacity).pack())
        .build();

    // generate new transaction
    let tx = tx
        .as_advanced_builder()
        .inputs(tx_inputs)
        .output(tx_output)
        .output_data(Bytes::from(tx_output_data).pack())
        .build();
    let tx = add_code_celldep(tx, OutPoint::new(_C.nft.tx_hash.clone(), 0));
    Ok(tx)
}
