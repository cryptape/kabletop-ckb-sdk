use async_jsonrpc_client::{
    HttpClient, Output, Transport, Params
};
use ckb_types::{
    prelude::*, core::BlockView, H256,
    packed::{
        Block, Transaction, Byte32
    }
};
use serde_json::{
    from_value, json
};
use anyhow::{
    Result, anyhow
};
use crate::{
    ckb::{
        transaction::helper,
        rpc::types::{
            Tip, Pagination, Cell, SearchKey, ScriptType, Order, ckb
        }
    },
    config::VARS as _C,
};
use ckb_jsonrpc_types::{
    JsonBytes, Status, Uint32
};
use std::sync::Mutex;
use ckb_sdk::rpc::HttpRpcClient;

lazy_static! {
    static ref INDEXER_CLIENT: HttpClient = HttpClient::new(_C.common.ckb_indexer_uri.as_str()).expect("indexer");
    static ref CKB_CLIENT: Mutex<HttpRpcClient> = Mutex::new(HttpRpcClient::new(_C.common.ckb_uri.clone()));
}

pub fn get_genesis_block() -> Result<Block> {
    let block = CKB_CLIENT
        .lock()
        .unwrap()
        .get_block_by_number(0)
        .unwrap_or_else(|err| {
            eprintln!("{}", err);
            None
        });
    let block = {
        let genesis = block.ok_or_else(|| anyhow!("genesis block is non-existent"))?;
        let block: BlockView = genesis.into();
        Block::new_unchecked(block.data().as_bytes())
    };
    Ok(block)
}

pub async fn get_transaction(tx_hash: Byte32) -> Result<Transaction> {
    let tx = CKB_CLIENT
        .lock()
        .unwrap()
        .get_transaction(H256(tx_hash.unpack()))
        .unwrap_or_else(|err| {
            eprintln!("{}", err);
            None
        });
    let tx = tx.ok_or_else(|| anyhow!("tx is non-existent"))?;
    if tx.tx_status.status == Status::Committed {
        Ok(tx.transaction.inner.into())
    } else {
        Err(anyhow!("transaction is not committed"))
    }
}

pub async fn get_tip_info() -> Result<Tip> {
    let output = INDEXER_CLIENT.request("get_tip", None).await?;
    match output {
        Output::Success(value) => return Ok(from_value(value.result)?),
        Output::Failure(err)   => return Err(anyhow!(err))
    }
}

pub async fn get_secp256k1_live_cells(lock_args: &[u8], limit: u32, cursor: Option<JsonBytes>) -> Result<Pagination<ckb::Cell>> {
    let secp256k1_script = helper::sighash_script_with_lockargs(lock_args);
    let search_key = SearchKey {
        script: secp256k1_script.into(),
        script_type: ScriptType::Lock,
        filter: None,
    };
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
            return Ok(pagination);
        },
        Output::Failure(err) => return Err(anyhow!(err))
    }
}
