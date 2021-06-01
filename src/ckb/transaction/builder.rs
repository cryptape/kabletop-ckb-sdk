use ckb_types::{
    prelude::*,
    core::{
        TransactionBuilder, TransactionView, Capacity
    },
    packed::{
        CellOutput, CellInput, OutPoint
    }
};
use crate::{
    config::VARS as _C,
    ckb::{
        transaction::helper,
        rpc::{
            methods as rpc,
            types::{
                SearchKey, ScriptType
            }
        },
        wallet::{
            signer, keystore
        }
    }
};
use anyhow::{
    Result, anyhow
};

/* CONFIG_CELL
*
* to help nft composers compose their own NFTs config cell, target output cell should only have one in CKB,
* so to create another NFTs config cell will consume the previous one
*
* data:
*     ckb_per_package(u64) | nft_count_per_package(u8) | [blake160|rate(u16)] | [blake160|rate(u16)] | ...
* lock:
*     code_hash = nft_wallet_contract 
*     hash_type = data
*     args 	    = composer_pubkey_blake160
* type:
*     code_hash = nft_payment_contract 
*     hash_type = data
*     args 	    = composer_pubkey_blake160
*/
pub async fn build_tx_compose_nft(
    package_price: u64, package_capacity: u8, nft_table: Vec<([u8; 20], u16)>
) -> Result<TransactionView> {
    // prepare scripts
    let wallet_script = helper::wallet_script(keystore::COMPOSER_PUBHASH.to_vec());
    let payment_script = helper::payment_script(keystore::COMPOSER_PUBHASH.to_vec());

    // prepare input cell
    let search_key = SearchKey::new(wallet_script.clone().into(), ScriptType::Lock).filter(payment_script.clone().into());
    let inputs = rpc::get_live_cells(search_key, 1, None).await?.objects
        .iter()
        .map(|cell| {
            CellInput::new_builder()
                .previous_output(cell.out_point.clone())
                .build()
        })
        .collect::<Vec<_>>();

    // prepare output data
    let output_data = helper::NFTConfig::new(package_price, package_capacity, nft_table).to_ckb_bytes();

    // prepare output cell
    let output = CellOutput::new_builder()
        .lock(wallet_script)
        .type_(Some(payment_script).pack())
        .build_exact_capacity(Capacity::bytes(output_data.len())?)?;

    // prepare tx
    let tx = TransactionBuilder::default()
        .inputs(inputs)
        .output(output)
        .output_data(output_data.pack())
        .build();

    // complete tx
    let tx = helper::complete_tx_with_sighash_cells(tx, keystore::COMPOSER_PUBHASH.clone(), helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.payment.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.wallet.tx_hash.clone(), 0));

    // sign tx
    let tx = signer::sign(tx, &keystore::COMPOSER_PRIVKEY, vec![]).await;
    Ok(tx)
}

/* WALLET_CELL
*
* to help other users create their own NFT store through composer's [NFT Wallet Cell] which is unique, and
* user can pay to store to purchase NFT package which could generate NFTs composed by corresponding composer
* 
* celldeps：
* 	  config_cell
* data:
* 	  0 (uint8)
* lock:
* 	  code_hash = nft_wallet_contract 
* 	  hash_type = data
* 	  args 	    = composer_pubkey_blake160
* type:
* 	  code_hash = nft_payment_contract 
* 	  hash_type = data
* 	  args 	    = user_pubkey_blake160
* capacity:
* 	  any
*/
pub async fn build_tx_create_nft_store() -> Result<TransactionView> {
    // prepare scripts
    let wallet_script           = helper::wallet_script(keystore::COMPOSER_PUBHASH.to_vec());
    let composer_payment_script = helper::payment_script(keystore::COMPOSER_PUBHASH.to_vec());
    let user_payment_script     = helper::payment_script(keystore::USER_PUBHASH.to_vec());

    // check composer if has composed nft or not
    let search_key = SearchKey::new(wallet_script.clone().into(), ScriptType::Lock)
        .filter(composer_payment_script.into());
    let config_cell = rpc::get_live_cells(search_key, 1, None).await?.objects;
    if config_cell.is_empty() {
        return Err(anyhow!("composer hasn't composed any NFTs yet."));
    }

    // check user if has created a nft store
    let search_key = SearchKey::new(wallet_script.clone().into(), ScriptType::Lock)
        .filter(user_payment_script.clone().into());
    if !rpc::get_live_cells(search_key, 1, None).await?.objects.is_empty() {
        return Err(anyhow!("user has already created this NFT store."));
    }

    // prepare output data
    let output_data = vec![0u8];

    // prepare output cell
    let output = CellOutput::new_builder()
        .lock(wallet_script)
        .type_(Some(user_payment_script).pack())
        .build_exact_capacity(Capacity::bytes(output_data.len())?)?;

    // prepare tx
    let tx = TransactionBuilder::default()
        .output(output)
        .output_data(output_data.pack())
        .build();

    // complete tx
    let tx = helper::complete_tx_with_sighash_cells(tx, keystore::USER_PUBHASH.clone(), helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.payment.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.wallet.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, config_cell[0].out_point.clone());

    // sign tx
    let tx = signer::sign(tx, &keystore::USER_PRIVKEY, vec![]).await;
    Ok(tx)
}

/* PAYMENT_CELL
*
* to help other users custom their own wallet cell to create payment cell to buy NFT packages from corresponding
* composer
*
* celldeps：
* 	  config_cell
* data:
* 	  nft_package_count (uint8)
* lock:
* 	  code_hash = nft_wallet_contract 
* 	  hash_type = data
* 	  args 	    = composer_pubkey_blake160
* type:
* 	  code_hash = nft_payment_contract 
* 	  hash_type = data
* 	args 	    = user_pubkey_blake160
* capacity:
* 	  any (must be greator than wallet_cell's)
*/
pub async fn build_tx_purchase_nft_package(package_count: u8) -> Result<TransactionView> {
    // prepare scripts
    let wallet_script           = helper::wallet_script(keystore::COMPOSER_PUBHASH.to_vec());
    let composer_payment_script = helper::payment_script(keystore::COMPOSER_PUBHASH.to_vec());
    let user_payment_script     = helper::payment_script(keystore::USER_PUBHASH.to_vec());

    // check composer if has composed nft or not
    let search_key = SearchKey::new(wallet_script.clone().into(), ScriptType::Lock)
        .filter(composer_payment_script.into());
    let config_cell = rpc::get_live_cells(search_key, 1, None).await?.objects;
    if config_cell.is_empty() {
        return Err(anyhow!("composer hasn't composed any NFTs yet."));
    }

    // check user if has created a nft store
    let search_key = SearchKey::new(wallet_script.clone().into(), ScriptType::Lock)
        .filter(user_payment_script.clone().into());
    let wallet_cell = rpc::get_live_cells(search_key, 1, None).await?.objects;
    if wallet_cell.is_empty() || wallet_cell[0].output_data.first() != Some(&0) {
        return Err(anyhow!("user hasn't owned a NFT store without payment status."));
    }

    // prepare input cell
    let input = CellInput::new_builder()
        .previous_output(wallet_cell[0].out_point.clone())
        .build();

    // parse from composed output data
    let nft_config = helper::NFTConfig::from(config_cell[0].output_data.clone());
    let packages_price = nft_config.buy_package(package_count as u64);

    // prepare output data
    let output_data = vec![package_count];

    // prepare output cell
    let mut capacity: u64 = wallet_cell[0].output.capacity().unpack();
    capacity += packages_price.as_u64();
    let output = CellOutput::new_builder()
        .lock(wallet_script)
        .type_(Some(user_payment_script).pack())
        .capacity(capacity.pack())
        .build();

    // prepare tx
    let tx = TransactionBuilder::default()
        .input(input)
        .output(output)
        .output_data(output_data.pack())
        .build();

    // complete tx
    let tx = helper::complete_tx_with_sighash_cells(tx, keystore::USER_PUBHASH.clone(), helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.payment.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.wallet.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, config_cell[0].out_point.clone());

    // sign tx
    let tx = signer::sign(tx, &keystore::USER_PRIVKEY, vec![]).await;
    Ok(tx)
}

/* WALLET_CELL + NFT_CELL
*
* to help other users rip NFT packages they purchased before, and recover payment cell to wallet cell
*
* // OUTPUT_CELL_1 (same as wallet_cell)
* celldeps：
* 	  config_cell
* data:
* 	  0 (uint8)
* lock:
* 	  code_hash = nft_wallet_contract 
* 	  hash_type = data
* 	  args 	    = composer_pubkey_blake160
* type:
* 	  code_hash = nft_payment_contract 
* 	  hash_type = data
* 	  args 	    = user_pubkey_blake160
* capacity:
* 	  any (must be greator than or equal to payment_cell's)
* 
* // OUTPUT_CELL_2 (same as nft_cell)
* headerdeps:
* 	  blockheader from payment_cell
* data:
* 	  blake160 | blake160 | ...
* lock:
* 	  any
* type:
* 	  code_hash = nft_contract 
* 	  hash_type = data
* 	  args 	    = nft_wallet_lockhash
*/
pub async fn build_tx_reveal_nft_package() -> Result<TransactionView> {
    // prepare scripts
    let wallet_script           = helper::wallet_script(keystore::COMPOSER_PUBHASH.to_vec());
    let nft_script              = helper::nft_script(wallet_script.calc_script_hash().raw_data().to_vec());
    let composer_payment_script = helper::payment_script(keystore::COMPOSER_PUBHASH.to_vec());
    let user_payment_script     = helper::payment_script(keystore::USER_PUBHASH.to_vec());

    // check composer if has composed nft or not
    let search_key = SearchKey::new(wallet_script.clone().into(), ScriptType::Lock)
        .filter(composer_payment_script.into());
    let config_cell = rpc::get_live_cells(search_key, 1, None).await?.objects;
    if config_cell.is_empty() {
        return Err(anyhow!("composer hasn't composed any NFTs yet."));
    }

    // check user if has created a nft store
    let search_key = SearchKey::new(wallet_script.clone().into(), ScriptType::Lock)
        .filter(user_payment_script.clone().into());
    let wallet_cell = rpc::get_live_cells(search_key, 1, None).await?.objects;
    if wallet_cell.is_empty() 
        || wallet_cell[0].output_data.first() == None
        || wallet_cell[0].output_data.first() == Some(&0) {
        return Err(anyhow!("user hasn't owned a NFT payment certificate."));
    }

    // prepare input cell
    let input = CellInput::new_builder()
        .previous_output(wallet_cell[0].out_point.clone())
        .build();

    // prepare output data
    let nft_config = helper::NFTConfig::from(config_cell[0].output_data.clone());
    let block = wallet_cell[0].block.clone().into_view();
    let count = wallet_cell[0].output_data[0];
    let output_wallet_data = vec![0];
    let output_nft_data = nft_config.rip_package(block.transactions_root(), block.uncles_hash(), count);

    // prepare output cell
    let output_wallet = CellOutput::new_builder()
        .lock(wallet_script)
        .type_(Some(user_payment_script).pack())
        .capacity(wallet_cell[0].output.capacity())
        .build();

    let output_nft = CellOutput::new_builder()
        .lock(helper::sighash_script_with_lockargs(&keystore::USER_PUBHASH.to_vec()))
        .type_(Some(nft_script).pack())
        .build_exact_capacity(Capacity::bytes(output_nft_data.len())?)?;

    // prepare tx
    let tx = TransactionBuilder::default()
        .input(input)
        .output(output_wallet)
        .output(output_nft)
        .output_data(output_wallet_data.pack())
        .output_data(output_nft_data.pack())
        .build();

    // complete tx
    let tx = helper::complete_tx_with_sighash_cells(tx, keystore::USER_PUBHASH.clone(), helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.payment.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.wallet.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.nft.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, config_cell[0].out_point.clone());
    let tx = helper::add_headerdep(tx, block.header());

    // sign tx
    let tx = signer::sign(tx, &keystore::USER_PRIVKEY, vec![]).await;
    Ok(tx)
}

#[cfg(test)]
mod test {
    use ckb_sdk::rpc::HttpRpcClient;
    use futures::executor::block_on;
    use ckb_types::core::TransactionView;
    use ckb_jsonrpc_types::TransactionView as JsonTxView;
    use crate::{
        config::VARS as _C, 
        ckb::transaction::{
            builder, helper
        }
    };

    fn default_nfts() -> Vec<([u8; 20], u16)> {
        vec![
            (helper::blake160(&[1u8]), 936),
            (helper::blake160(&[2u8]), 25456),
            (helper::blake160(&[3u8]), 26771),
            (helper::blake160(&[4u8]), 27034),
            (helper::blake160(&[5u8]), 30470),
            (helper::blake160(&[6u8]), 62600),
        ]
    }

    fn send_transaction(tx: TransactionView, name: &str) {
        let mut ckb_rpc = HttpRpcClient::new(String::from(_C.common.ckb_uri.clone()));
        write_tx_to_file(tx.clone(), format!("{}.json", name));
        match ckb_rpc.send_transaction(tx.data()) {
            Ok(tx_hash) => println!("success: {:?}", hex::encode(tx_hash.as_bytes())),
            Err(err)    => panic!("failure: {:?}", err)
        }
    }

    fn write_tx_to_file(tx: TransactionView, path: String) {
        let tx = JsonTxView::from(tx);
        let json = serde_json::to_string_pretty(&tx).expect("jsonify");
        std::fs::write(path, json).expect("write json file");
    }

    #[test]
    fn test_build_tx_compose_nft() {
        let tx = block_on(builder::build_tx_compose_nft(helper::fee("150").as_u64(), 5, default_nfts())).expect("compose nft");
        send_transaction(tx, "compose_nft");
    }

    #[test]
    fn test_build_tx_create_nft_store() {
        let tx = block_on(builder::build_tx_create_nft_store()).expect("create nft store");
        send_transaction(tx, "create_nft_store");
    }

    #[test]
    fn test_build_tx_purchase_nft_package() {
        let tx = block_on(builder::build_tx_purchase_nft_package(10)).expect("purchase nft package");
        send_transaction(tx, "purchase_nft_package");
    }

    #[test]
    fn test_build_tx_reveal_nft_package() {
        let tx = block_on(builder::build_tx_reveal_nft_package()).expect("reveal nft package");
        send_transaction(tx, "reveal_nft_package");
    }
}
