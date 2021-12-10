use async_jsonrpc_client::{
    HttpClient, Output, Transport, Params
};
use ckb_types::{
    prelude::*, core::BlockView, H256, packed::{
        Block, Transaction, Byte32, Script
    }
};
use serde_json::{
    from_value, json
};
use anyhow::{
    Result, anyhow
};
use crate::{
    config::VARS as _C, ckb::{
		transaction::helper::sighash_script, rpc::types::{
			Pagination, Cell, SearchKey, Order, ckb, ScriptType, CellsCapacity
		}
	}
};
use ckb_jsonrpc_types::{
    JsonBytes, Status, Uint32, OutputsValidator
};
use std::{
	sync::Mutex, collections::HashMap
};
use ckb_sdk::rpc::HttpRpcClient;

lazy_static! {
    static ref INDEXER_CLIENT: HttpClient = HttpClient::new(_C.common.ckb_indexer_uri.as_str()).expect("indexer");
    static ref CKB_CLIENT: Mutex<HttpRpcClient> = Mutex::new(HttpRpcClient::new(_C.common.ckb_uri.clone()));
}

pub fn get_genesis_block() -> Result<Block> {
	let mut result = Err(anyhow!("fetch genesis block failed over 5 times"));
	for _ in 0..5 {
		match get_block(0) {
			Ok(block) => {
				result = Ok(block);
				break
			},
			Err(error) => {
				println!("{} [retry]", error);
				result = Err(error);
			}
		}
	}
	result
}

pub fn get_block(block_number: u64) -> Result<Block> {
	let mut error = String::new();
    let block = CKB_CLIENT
        .lock()
        .unwrap()
        .get_block_by_number(block_number)
        .unwrap_or_else(|err| {
			error = err.to_string();
            None
        });
    let block = {
        let genesis = block.ok_or(anyhow!(format!("fetch block #{} error: {}", block_number, error)))?;
        let block: BlockView = genesis.into();
        Block::new_unchecked(block.data().as_bytes())
    };
    Ok(block)
}

pub fn get_transaction(tx_hash: Byte32) -> Result<Transaction> {
	let mut error = String::new();
    let tx = CKB_CLIENT
        .lock()
        .unwrap()
        .get_transaction(H256(tx_hash.unpack()))
        .unwrap_or_else(|err| {
			error = err.to_string();
            None
        });
    let tx = tx.ok_or(anyhow!(error))?;
    if tx.tx_status.status == Status::Committed {
		if let Some(transaction) = tx.transaction {
			Ok(transaction.inner.into())
		} else {
			Err(anyhow!("empty transaction"))
		}
    } else {
        Err(anyhow!("not committed"))
    }
}

pub fn send_transaction(tx: Transaction) -> Result<H256> {
    let result = CKB_CLIENT
        .lock()
        .unwrap()
		.send_transaction(tx, Some(OutputsValidator::Passthrough));
	match result {
		Ok(hash) => Ok(hash),
		Err(err) => Err(anyhow!(err))
	}
}

pub fn get_tip_block_number() -> Result<u64> {
    let result = CKB_CLIENT
        .lock()
        .unwrap()
        .get_tip_block_number();
	match result {
		Ok(number) => Ok(number),
		Err(err)   => Err(anyhow!(err))
	}
}

pub async fn get_live_cells(search_key: SearchKey, limit: u32, cursor: Option<JsonBytes>) -> Result<Pagination<ckb::Cell>> {
    let output = INDEXER_CLIENT.request("get_cells", Some(Params::Array(vec![
        json!(search_key),
        json!(Order::Asc),
        json!(Uint32::from(limit)),
        json!(cursor)
    ]))).await?;
    match output {
        Output::Success(value) => {
            let pagination: Pagination<Cell> = from_value(value.result)?;
            let cells = pagination
                .objects
                .into_iter()
                .map(|cell| ckb::Cell::from(cell))
                .collect::<Vec<ckb::Cell>>();
            let pagination: Pagination<ckb::Cell> = Pagination::<ckb::Cell> {
                objects:     cells,
                last_cursor: pagination.last_cursor
            };
            Ok(pagination)
        },
        Output::Failure(err) => Err(anyhow!(err))
    }
}

pub async fn get_total_capacity(lock_args: Vec<u8>) -> Result<ckb::CellsCapacity> {
	let lock_script = sighash_script(lock_args.as_slice());
	let search_key = SearchKey::new(lock_script.into(), ScriptType::Lock);
    let output = INDEXER_CLIENT.request("get_cells_capacity", Some(Params::Array(vec![
        json!(search_key)
    ]))).await?;
	match output {
		Output::Success(value) => {
			let capacity: CellsCapacity = from_value(value.result)?;
			Ok(capacity.into())
		},
		Output::Failure(err) => Err(anyhow!(err))
	}
}

pub async fn get_live_nfts(lock_script: Script, type_script: Option<Script>, cellstep: u32) -> Result<HashMap<[u8; 20], u32>> {
    let mut cursor = None;
	let mut live_nfts = HashMap::new();
    loop {
		let mut search_key = SearchKey::new(lock_script.clone().into(), ScriptType::Lock);
		if let Some(type_script) = &type_script {
			search_key = search_key.filter(type_script.clone().into());
		}
		let live_cells = get_live_cells(search_key, cellstep, cursor).await?;
		live_cells.objects
			.iter()
			.for_each(|cell| {
				let mut data = cell.output_data.to_vec();
				let mut nft = [0u8; 20];
				let n = data.len() / 20;
				for _ in 0..n {
					nft.copy_from_slice(&data[..20]);
					data = data[20..].to_vec();
					if let Some(count) = live_nfts.get_mut(&nft) {
						*count += 1;
					} else {
						live_nfts.insert(nft, 1);
					}
				}
			});
        if live_cells.last_cursor.is_empty() {
            break;
        } 
        cursor = Some(live_cells.last_cursor);
	}
	Ok(live_nfts)
}
