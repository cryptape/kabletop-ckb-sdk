use ckb_jsonrpc_types::{
    Uint32, Uint64, Script, BlockNumber, JsonBytes, CellOutput, OutPoint
};
use serde::{
    Deserialize, Serialize
};

//////////////////////////////////////////////////
// Json Types
//////////////////////////////////////////////////

#[derive(Deserialize, Serialize, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ScriptType {
    Lock,
    Type,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Desc,
    Asc,
}

#[derive(Deserialize, Serialize)]
pub struct SearchKey {
    pub script:      Script,
    pub script_type: ScriptType,
    pub filter:      Option<SearchKeyFilter>,
}

#[derive(Deserialize, Serialize, Default, Clone)]
pub struct SearchKeyFilter {
    pub script:                Option<Script>,
    pub output_data_len_range: Option<[Uint64; 2]>,
    pub output_capacity_range: Option<[Uint64; 2]>,
    pub block_range:           Option<[BlockNumber; 2]>,
}

#[derive(Deserialize, Serialize)]
pub struct Pagination<T> {
    pub objects:     Vec<T>,
    pub last_cursor: JsonBytes,
}

#[derive(Serialize, Deserialize)]
pub struct Cell {
    pub output:       CellOutput,
    pub output_data:  JsonBytes,
    pub out_point:    OutPoint,
    pub block_number: BlockNumber,
    pub tx_index:     Uint32,
}

impl SearchKey {
    pub fn new(script: Script, script_type: ScriptType) -> SearchKey {
        SearchKey {
            script,
            script_type,
            filter: None
        }
    }

    pub fn filter(&self, script: Script) -> Self {
        let mut filter: SearchKeyFilter = self.filter.clone().unwrap_or(SearchKeyFilter::default());
        filter.script = Some(script);
        SearchKey {
            script:      self.script.clone(),
            script_type: self.script_type,
            filter:      Some(filter)
        }
    }
}

// ckb types format for a better use that turned from previous json types
pub mod ckb {
    use ckb_types::{
        prelude::*, bytes::Bytes,
        packed::{
            CellOutput, OutPoint, Uint32, Block
        }
    };
    use std::convert::From;
    use crate::ckb::{
        rpc::types as json, rpc::methods as rpc
    };

    pub struct Cell {
        pub output:      CellOutput,
        pub output_data: Bytes,
        pub out_point:   OutPoint,
        pub block:       Block,
        pub tx_index:    Uint32,
    }

    impl From<json::Cell> for Cell {
        fn from(json_cell: json::Cell) -> Self {
            let output = {
                let output: CellOutput = json_cell.output.into();
                CellOutput::new_unchecked(output.as_bytes())
            };
            let out_point = {
                let out_point: OutPoint = json_cell.out_point.into();
                OutPoint::new_unchecked(out_point.as_bytes())
            };
            let block = {
                let block_number: u64 = json_cell.block_number.into();
                rpc::get_block(block_number).unwrap()
            };
            let tx_index = {
                let tx_index: u32 = json_cell.tx_index.into();
                tx_index.pack()
            };
            let output_data = json_cell.output_data.into_bytes();
            Cell {
                output, out_point, block, tx_index, output_data,
            }
        }
    }
}
