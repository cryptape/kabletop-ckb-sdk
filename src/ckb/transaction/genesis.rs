use ckb_types::{
    prelude::*,
    core::{
        DepType, ScriptHashType
    },
    packed::{
        CellDep, OutPoint, Transaction, Script
    },
};
use anyhow::{
    Result, anyhow
};
use ckb_hash::new_blake2b;
use crate::ckb::rpc;

lazy_static! {
    pub static ref GENESIS: Genesis = get_genesis_from_block().expect("get genesis");
}

const SIGHASH_OUTPUT:        (usize, usize) = (0, 1);
const SIGHASH_GROUP_OUTPUT:  (usize, usize) = (1, 0);
const MULTISIG_GROUP_OUTPUT: (usize, usize) = (1, 1);

pub struct Genesis {
    pub sighash_script:   Script,
    pub sighash_celldep:  CellDep,
    pub multisig_celldep: CellDep,
}

pub fn get_genesis_from_block() -> Result<Genesis> {
    let block = rpc::methods::get_genesis_block()?;
    let sighash_tx = block
        .transactions()
        .get(SIGHASH_OUTPUT.0)
        .ok_or_else(|| anyhow!("no sighash transaction found"))?;
    let sighash_group_tx = block
        .transactions()
        .get(SIGHASH_GROUP_OUTPUT.0)
        .ok_or_else(|| anyhow!("no sighash group transaction found"))?;
    let multisig_group_tx = block
        .transactions()
        .get(MULTISIG_GROUP_OUTPUT.0)
        .ok_or_else(|| anyhow!("no multisig group transaction found"))?;
    let genesis = Genesis {
        sighash_script:   build_script(sighash_tx, SIGHASH_OUTPUT.1)?,
        sighash_celldep:  build_celldep(sighash_group_tx, SIGHASH_GROUP_OUTPUT.1 as u32),
        multisig_celldep: build_celldep(multisig_group_tx, MULTISIG_GROUP_OUTPUT.1 as u32)
    };
    Ok(genesis)
}

fn build_celldep(tx: Transaction, tx_index: u32) -> CellDep {
    let tx_hash = {
        let mut hasher = new_blake2b();
        hasher.update(tx.raw().as_slice());
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash
    };
    let outpoint = OutPoint::new_builder()
        .tx_hash(tx_hash.pack())
        .index(tx_index.pack())
        .build();
    CellDep::new_builder()
        .out_point(outpoint)
        .dep_type(DepType::DepGroup.into())
        .build()
}

fn build_script(tx: Transaction, tx_index: usize) -> Result<Script> {
    let output = tx
        .raw()
        .outputs()
        .get(tx_index)
        .ok_or_else(|| anyhow!("can't find cell output"))?;
    let type_hash = output
        .type_()
        .to_opt()
        .map(|script| script.calc_script_hash())
        .ok_or_else(|| anyhow!("can't calc typescript hash"))?;
    Ok(
        Script::new_builder()
            .code_hash(type_hash)
            .hash_type(ScriptHashType::Type.into())
            .build()
    )
}

#[cfg(test)]
mod test {
    use super::get_genesis_from_block;

    #[test]
    fn test_make_genesis() {
        get_genesis_from_block().expect("test geting genesis");
    }
}
