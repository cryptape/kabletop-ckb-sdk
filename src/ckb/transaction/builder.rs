use ckb_types::{
    prelude::*, bytes::Bytes,
    core::{
        TransactionBuilder, TransactionView, Capacity, ScriptHashType
    },
    packed::{
        CellOutput, CellInput, OutPoint, Script, WitnessArgs
    }
};
use crate::{
    config::VARS as _C,
    ckb::{
        transaction::{
            helper, channel::protocol
        },
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
use molecule::{
    prelude::Entity as MolEntity, bytes::Bytes as MolBytes
};
use ckb_crypto::secp::Signature;

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
    package_price: u64, package_capacity: u8, nft_table: Vec<([u8; 20], u8)>
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
    let tx = helper::complete_tx_with_sighash_cells(tx, &keystore::COMPOSER_PUBHASH, helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.payment.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.wallet.tx_hash.clone(), 0));

    // sign tx
    let tx = signer::sign(tx, &keystore::COMPOSER_PRIVKEY, vec![], &|_| true);
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
    let tx = helper::complete_tx_with_sighash_cells(tx, &keystore::USER_PUBHASH, helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.payment.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.wallet.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, config_cell[0].out_point.clone());

    // sign tx
    let tx = signer::sign(tx, &keystore::USER_PRIVKEY, vec![], &|_| true);
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
    let tx = helper::complete_tx_with_sighash_cells(tx, &keystore::USER_PUBHASH, helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.payment.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.wallet.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, config_cell[0].out_point.clone());

    // sign tx
    let tx = signer::sign(tx, &keystore::USER_PRIVKEY, vec![], &|_| true);
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
    let package_count = wallet_cell[0].output_data[0];
    let output_wallet_data = vec![0];
    let output_nft_data = nft_config.rip_package(block.header().hash(), package_count);

    // prepare output cell
    let output_wallet = CellOutput::new_builder()
        .lock(wallet_script)
        .type_(Some(user_payment_script).pack())
        .capacity(wallet_cell[0].output.capacity())
        .build();

    let output_nft = CellOutput::new_builder()
        .lock(helper::sighash_script(&keystore::USER_PUBHASH.to_vec()))
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
    let tx = helper::complete_tx_with_sighash_cells(tx, &keystore::USER_PUBHASH, helper::fee("0.1")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.payment.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.wallet.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.nft.tx_hash.clone(), 0));
    let tx = helper::add_code_celldep(tx, config_cell[0].out_point.clone());
    let tx = helper::add_headerdep(tx, block.header());

    // sign tx
    let tx = signer::sign(tx, &keystore::USER_PRIVKEY, vec![], &|_| true);
    Ok(tx)
}

/* CHALLENGE_CELL
*
* to help user create a channel challenge tx which will consume previous channel cell no matter it's in original
* state or challenge state
* 
* // INPUT_CELL
* on-chain channel_cell
* 	
* // OUTPUT_CELL
* data:
* 	  round_count (uint8) | user_round_signature | user_type (uint8) | operations (vec<string>)
* lock:
* 	  (same as channel_cell)
* type:
* 	  (same as channel_cell)
* capacity:
* 	  (same as channel_cell)
* 
* // WITNESSES
* [
* 	  lock: user1_or_user2_input_signature
* 	  lock: user1_round_signature, input_type: user2_type (uint8) | operations (vec<string>)
* 	  lock: user2_round_signature, input_type: user1_type (uint8) | operations (vec<string>)
* 	  ...
* ]
*/
pub async fn build_tx_challenge_channel(
    channel_script: Script, challenge_data: protocol::Challenge, rounds: &Vec<(protocol::Round, Signature)>
) -> Result<TransactionView> {
    // make sure channel stays open
    let search_key = SearchKey::new(channel_script.clone().into(), ScriptType::Lock);
    let channel_cell = rpc::get_live_cells(search_key, 1, None).await?.objects;
    if channel_cell.is_empty() {
        return Err(anyhow!("channel with specified channel_script is non-existent"));
    }

    // prepare input/output and witnesses
    let input = CellInput::new_builder()
        .previous_output(channel_cell[0].out_point.clone())
        .build();
    let output = CellOutput::new_builder()
        .lock(channel_script)
        .build_exact_capacity(Capacity::shannons(challenge_data.as_slice().len() as u64))?;
    let witnesses = rounds
        .iter()
        .map(|(round, signature)| {
            WitnessArgs::new_builder()
                .lock(Some(Bytes::from(signature.serialize())).pack())
                .input_type(Some(Bytes::from(round.as_slice().to_vec())).pack())
                .build()
        })
        .collect::<Vec<_>>();
    
    // turn channel to challenge state
    let tx = TransactionBuilder::default()
        .input(input)
        .output(output)
        .output_data(Bytes::from(challenge_data.as_slice().to_vec()).pack())
        .build();
    let tx = helper::complete_tx_with_sighash_cells(tx, &keystore::USER_PUBHASH, helper::fee("0.1")).await?;
    let tx = signer::sign(tx, &keystore::USER_PRIVKEY, witnesses, &|_| true);

    Ok(tx)
}

/* SETTLEMENT_CELL (from CHANNEL_CELL or CHALLENGE_CELL)
*
* to help user close an opened kabletop channel from original state or challenge state, this function will
* consume current channel cell and generate two sighash cells for two users separately
*
* // INPUT_CELL
* since:
* 	  tip blocknumber (uint64, only from challenge)
* others:
* 	  (same as channel_cell or challenge_cell)
* 	
* // OUTPUT_CELL_1
* data:
* 	  any
* lock:
* 	  code_hash = lock_code_hash (from kabletop_args)
* 	  hash_type = data
* 	  args 	    = user1_pkhash   (from kabletop_args)
* type:
* 	  any
* capacity:
* 	  any
* 
* // OUTPUT_CELL_2
* data:
* 	  any
* lock:
* 	  code_hash = lock_code_hash (from kabletop_args)
* 	  hash_type = data
* 	  args 	    = user2_pkhash   (from kabletop_args)
* type:
* 	  any
* capacity:
* 	  any
* 
* // WITNESSES
* [
* 	lock: sender_input_signature
* 	lock: user1_round_signature, input_type: user2_type (uint8) | operations (vec<string>)
* 	lock: user2_round_signature, input_type: user1_type (uint8) | operations (vec<string>)
* 	...
* ]
*/
pub async fn build_tx_close_channel(
    channel_script: Script, rounds: &Vec<(protocol::Round, Signature)>, winner: u8, from_challenge: bool
) -> Result<TransactionView> {
    // make sure channel stays open
    let search_key = SearchKey::new(channel_script.clone().into(), ScriptType::Lock);
    let channel_cell = rpc::get_live_cells(search_key, 1, None).await?.objects;
    if channel_cell.is_empty() {
        return Err(anyhow!("channel with specified channel_script is non-existent"));
    }

    // prepare input and witnesses
    let mut input = CellInput::new_builder().previous_output(channel_cell[0].out_point.clone());
    if from_challenge {
        let block_number = rpc::get_tip_block_number();
        input = input.since(block_number.pack());
    }
    let witnesses = rounds
        .iter()
        .map(|(round, signature)| {
            WitnessArgs::new_builder()
                .lock(Some(Bytes::from(signature.serialize())).pack())
                .input_type(Some(Bytes::from(round.as_slice().to_vec())).pack())
                .build()
        })
        .collect::<Vec<_>>();

    // prepare outputs
    let kabletop_args = {
        let args: Bytes = channel_cell[0].output.lock().args().unpack();
        protocol::Args::new_unchecked(MolBytes::from(args.to_vec()))
    };
    let channel_ckb: u64 = channel_cell[0].output.capacity().unpack();
    let staking_ckb: u64 = kabletop_args.user_staking_ckb().into();
    if channel_ckb <= staking_ckb * 2 {
        return Err(anyhow!("broken channel with wrong cell capacity"));
    }
    let bet_ckb = channel_ckb / 2 - staking_ckb;
    let mut user1_capacity = staking_ckb;
    let mut user2_capacity = staking_ckb;
    match winner {
        1 => user1_capacity += bet_ckb,
        2 => user2_capacity += bet_ckb,
        _ => return Err(anyhow!("winner must be 1 or 2"))
    }
    let outputs = 
    vec![(&kabletop_args.user1_pkhash(), user1_capacity), (&kabletop_args.user2_pkhash(), user2_capacity)]
        .iter()
        .map(|&(pkhash, ckb)| {
            let lock_script = Script::new_builder()
                .code_hash(kabletop_args.lock_code_hash().into())
                .hash_type(ScriptHashType::Data.into())
                .args(Bytes::from(<[u8; 20]>::from(pkhash).to_vec()).pack())
                .build();
            CellOutput::new_builder()
                .lock(lock_script)
                .build_exact_capacity(Capacity::shannons(ckb))
        })
        .collect::<Result<Vec<_>, _>>()?;
    
    // cloen kabletop channel
    let tx = TransactionBuilder::default()
        .input(input.build())
        .outputs(outputs)
        .outputs_data(vec![Bytes::default(), Bytes::default()].pack())
        .build();
    let tx = helper::complete_tx_with_sighash_cells(tx, &keystore::USER_PUBHASH, helper::fee("0.1")).await?;
    let tx = signer::sign(tx, &keystore::USER_PRIVKEY, witnesses, &|_| true);

    Ok(tx)
}

///////////////////////////////////////////////////////
/// TX BUILDING FUNCTIONS TEST
///////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use ckb_sdk::rpc::HttpRpcClient;
    use futures::executor::block_on;
    use ckb_types::core::TransactionView;
    use ckb_jsonrpc_types::TransactionView as JsonTxView;
    use ckb_crypto::secp::Privkey;
    use crate::{
        config::VARS as _C, ckb::wallet::keystore,
        ckb::transaction::{
            builder, helper, channel::interact
        }
    };

    fn default_nfts() -> Vec<([u8; 20], u8)> {
        vec![
            (helper::blake160(&[1u8]), 56),
            (helper::blake160(&[2u8]), 86),
            (helper::blake160(&[3u8]), 101),
            (helper::blake160(&[4u8]), 134),
            (helper::blake160(&[5u8]), 180),
            (helper::blake160(&[6u8]), 255),
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
        let tx = block_on(builder::build_tx_purchase_nft_package(1)).expect("purchase nft package");
        send_transaction(tx, "purchase_nft_package");
    }

    #[test]
    fn test_build_tx_reveal_nft_package() {
        let tx = block_on(builder::build_tx_reveal_nft_package()).expect("reveal nft package");
        send_transaction(tx, "reveal_nft_package");
    }

    #[test]
    fn test_build_tx_open_channel() {
        let user1_privkey = keystore::USER_PRIVKEY.clone();
        let user2_privkey = {
            let byte32 = helper::blake256_to_byte32("d44955b4770247b233c284268c961085e622febb61d364c9a5cabe0c238f08d4")
                .expect("blake2b_256 to [u8; 32]");
            Privkey::from(ckb_types::H256(byte32))
        };
        let user1_pkhash = keystore::USER_PUBHASH.clone();
        let user2_pkhash = helper::privkey_to_pkhash(&user2_privkey);

        let staking_ckb = helper::fee("500").as_u64();
        let bet_ckb = helper::fee("2000").as_u64();
        let deck_size = 1u8;
        let (user1_nfts, user2_nfts) = {
            let nfts = default_nfts().iter().map(|&(nft, _)| nft).collect::<Vec<[u8; 20]>>();
            // println!("default_nfts = {:?}", nfts.iter().map(|nft| hex::encode(nft)).collect::<Vec<String>>());
            (vec![nfts[1]], vec![nfts[1]])
        };

        // user1 prepare
        let tx = block_on(interact::prepare_channel_tx(staking_ckb, bet_ckb, deck_size, &user1_nfts, &user1_pkhash))
            .expect("prepare_channel_tx");
        // user2 complete
        let tx = block_on(interact::complete_channel_tx(tx, staking_ckb, bet_ckb, deck_size, &user2_nfts, &user2_pkhash))
            .expect("complete_channel_tx");
        // user2 sign
        let tx = interact::sign_channel_tx(tx, staking_ckb, bet_ckb, deck_size, &user2_nfts, &user2_privkey)
            .expect("user2 sign_channel_tx");
        // user1 sign
        let tx = interact::sign_channel_tx(tx, staking_ckb, bet_ckb, deck_size, &user1_nfts, &user1_privkey)
            .expect("user1 sign_channel_tx");

        write_tx_to_file(tx, format!("{}.json", "open_channel"));
    }
}
