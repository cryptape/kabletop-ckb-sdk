use crate::{
    config::VARS as _C,
    ckb::{
        rpc::methods as rpc,
        wallet::{
            keystore, signer
        },
        transaction::{
            genesis::GENESIS as _G, helper, channel::protocol::*
        }
    }
};
use anyhow::{
    Result, anyhow
};
use ckb_types::{
    prelude::*, bytes::Bytes,
    core::{
        Capacity, TransactionBuilder, TransactionView
    },
    packed::{
        CellOutput, OutPoint
    }
};
use molecule::{
    bytes::Bytes as MolBytes,
    prelude::{
        Entity as MolEntity, Builder as MolBuilder
    }
};
use ckb_crypto::secp::{
    Privkey, Signature, Message
};
use ckb_hash::new_blake2b;
use std::convert::TryInto;

/* CHANNEL_CELL
*
* use a combine of [prepare_channel_tx, complete_channel_tx, sign_channel_tx] to complish building an open-channel-tx 
*
* data:
* 	  none
* lock:
* 	  code_hash = kabletop_contract 
* 	  hash_type = data
* 	  args 	    = staking_ckb(u64) | deck_size(u8) | begin_blocknumber(u64) | lock_code_hash(blake256) 
* 	  			  | user1_pkhash(blake160) | user1_nfts(vec<blake160>) | user2_pkhash(blake160) | user2_nfts(vec<blake160>)
* type:
* 	  any
*/

// prepare kabletop tx with user1-part filled
pub async fn prepare_channel_tx(
    staking_ckb: u64, bet_ckb: u64, deck_size: u8, nfts: &Vec<[u8; 20]>, pkhash: &[u8; 20]
) -> Result<TransactionView> {
    // prepare lock_args
    let block_number = rpc::get_tip_block_number();
    let sighash_hash = _G.sighash_script.code_hash().clone();
    if deck_size as usize != nfts.len() {
        return Err(anyhow!("number of nft mismatch specified deck size"));
    }
    let kabletop_args = Args::new_builder()
        .user_staking_ckb(staking_ckb.into())
        .user_deck_size(deck_size.into())
        .begin_blocknumber(block_number.into())
        .lock_code_hash(sighash_hash.into())
        .user1_pkhash(pkhash.into())
        .user1_nfts(nfts.into())
        .build();
    
    // prepare output
    let kabletop_script = helper::kabletop_script(kabletop_args.as_bytes().to_vec());
    let output = CellOutput::new_builder()
        .lock(kabletop_script.clone())
        .capacity(Capacity::shannons(bet_ckb + staking_ckb).pack())
        .build();

    // prepare tx
    let tx = TransactionBuilder::default()
        .output(output)
        .output_data(Bytes::from(vec![]).pack())
        .build();
    let tx = helper::complete_tx_with_nft_cells(tx, pkhash, &keystore::COMPOSER_PUBHASH, nfts.clone()).await?;
    let tx = helper::complete_tx_with_sighash_cells(tx, pkhash, helper::fee("0.05")).await?;
    let tx = helper::add_code_celldep(tx, OutPoint::new(_C.kabletop.tx_hash.clone(), 0));

    Ok(tx)
}

// complete kabeltop tx with user2-part filled
pub async fn complete_channel_tx(
    tx: TransactionView, staking_ckb: u64, bet_ckb: u64, deck_size: u8, nfts: &Vec<[u8; 20]>, pkhash: &[u8; 20]
) -> Result<TransactionView> {
    // check and complete kabletop args
    let mut tx_outputs: Vec<CellOutput> = tx.outputs().into_iter().map(|output| output).collect();
    let output = tx_outputs.first().ok_or(anyhow!("tx's output is empty"))?;
    let kabletop_args = {
        let args: Bytes = output.lock().args().unpack();
        Args::new_unchecked(MolBytes::from(args.to_vec()))
    };
    if u64::from(kabletop_args.user_staking_ckb())  != staking_ckb
        || u8::from(kabletop_args.user_deck_size()) != deck_size {
        return Err(anyhow!("some of kabletop args mismatched"));
    }
    if deck_size as usize != nfts.len() {
        return Err(anyhow!("number of nft mismatch specified deck size"));
    }
    let kabletop_args = kabletop_args
        .as_builder()
        .user2_pkhash(pkhash.into())
        .user2_nfts(nfts.into())
        .build();

    // check and double output capacity
    let capacity = {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(output.capacity().as_bytes().to_vec().as_slice());
        u64::from_le_bytes(bytes)
    };
    if capacity != staking_ckb + bet_ckb {
        return Err(anyhow!("quantity of output capacity mismatched"));
    }
    tx_outputs[0] = output
        .clone()
        .as_builder()
        .lock(helper::kabletop_script(kabletop_args.as_bytes().to_vec()))
        .capacity(Capacity::shannons(capacity * 2).pack())
        .build();

    // complete tx
    let tx = tx
        .as_advanced_builder()
        .set_outputs(tx_outputs)
        .build();
    let tx = helper::complete_tx_with_nft_cells(tx, pkhash, &keystore::COMPOSER_PUBHASH, nfts.clone()).await?;
    let tx = helper::complete_tx_with_sighash_cells(tx, pkhash, helper::fee("0.05")).await?;

    Ok(tx)
}

// check kabletop args and sign channel tx
pub fn sign_channel_tx(
    tx: TransactionView, staking_ckb: u64, bet_ckb: u64, deck_size: u8, nfts: &Vec<[u8; 20]>, privkey: &Privkey
) -> Result<TransactionView> {
    // check kabletop args
    let output = tx.output(0).ok_or(anyhow!("tx's output is empty"))?;
    let kabletop_args = {
        let args: Bytes = output.lock().args().unpack();
        Args::new_unchecked(MolBytes::from(args.to_vec()))
    };
    let pkhash = helper::privkey_to_pkhash(&privkey);
    let user1_pkhash = <[u8; 20]>::from(kabletop_args.user1_pkhash());
    let user2_pkhash = <[u8; 20]>::from(kabletop_args.user2_pkhash());
    let mut user1_nfts = &mut Vec::from(kabletop_args.user1_nfts());
    let mut user2_nfts = &mut Vec::from(kabletop_args.user2_nfts());
    if u64::from(kabletop_args.user_staking_ckb())  != staking_ckb
        || u8::from(kabletop_args.user_deck_size()) != deck_size
        || (user1_pkhash == pkhash && user1_nfts != nfts)
        || (user2_pkhash == pkhash && user2_nfts != nfts) {
        return Err(anyhow!("some of kabletop args mismatched"));
    }

    // check kabletop output capacity
    let capacity = {
        let ckb: Capacity = output.capacity().unpack();
        ckb.as_u64()
    };
    if capacity != (staking_ckb + bet_ckb) * 2 {
        return Err(anyhow!("kabletop output capacity is incorrect"));
    }

    // check wether two nft lists from kabletop args match both their nft cells'
    let user1_lock_script = helper::sighash_script(&user1_pkhash[..]);
    let user2_lock_script = helper::sighash_script(&user2_pkhash[..]);
    let type_script = {
        let wallet = helper::wallet_script(keystore::COMPOSER_PUBHASH.clone().to_vec());
        helper::nft_script(wallet.calc_script_hash().raw_data().to_vec())
    };
    let mut user1_cell_nfts = vec![];
    let mut user2_cell_nfts = vec![];
    tx.outputs_with_data_iter()
        .for_each(|(output, data)| {
            let mut userx_cell_nfts: Option<&mut Vec<[u8; 20]>> = None;
            if let Some(script) = output.type_().to_opt() {
                if type_script == script {
                    if output.lock() == user1_lock_script {
                        userx_cell_nfts = Some(&mut user1_cell_nfts);
                    } else if output.lock() == user2_lock_script {
                        userx_cell_nfts = Some(&mut user2_cell_nfts);
                    }
                }
            }
            if let Some(cell_nfts) = userx_cell_nfts {
                let mut data = data.to_vec();
                let n = data.len() / 20;
                for _ in 0..n {
                    cell_nfts.push(data[..20].try_into().unwrap());
                    data = data[20..].to_vec();
                }
            }
        });
    helper::blake160_intersect(&mut user1_nfts, &mut user1_cell_nfts);
    helper::blake160_intersect(&mut user2_nfts, &mut user2_cell_nfts);
    if user1_nfts.len() > 0 || user2_nfts.len() > 0 {
        return Err(anyhow!("some of two users haven't supplied correct nft cells"));
    }

    // sign tx
    let tx = signer::sign(tx, &privkey, vec![], &|output| {
        let bytes: Bytes = output.lock().args().unpack();
        bytes.to_vec() == pkhash
    });
    Ok(tx)
}

// check the last one of imported kabeltop [signed_rounds] wether matches its corrensponding signature
pub fn check_channel_round(
    script_hash: &[u8; 32], capacity: u64, signed_rounds: &Vec<(Round, Signature)>, expect_pkhash: &[u8; 20]
) -> Result<bool> {
    let mut digest = [0u8; 32];
    let mut last_signature: Option<&Signature> = None;
    signed_rounds
        .iter()
        .for_each(|(round, signature)| {
            let mut hasher = new_blake2b();
            if let Some(last_signature) = last_signature {
                hasher.update(&digest);
                hasher.update(&last_signature.serialize());
            } else {
                hasher.update(script_hash);
                hasher.update(&capacity.to_le_bytes());
            }
            hasher.update(round.as_slice());
            hasher.finalize(&mut digest);
            last_signature = Some(&signature);
        });

    // recover public key from signature and compare its hash format to [expect_pkhash]
    if let Some(verify_signature) = last_signature {
        let pkhash = {
            let pubkey = verify_signature.recover(&Message::from(digest))?;
            &helper::blake160(&pubkey.serialize())
        };
        Ok(pkhash == expect_pkhash)
    } else {
        Err(anyhow!("import empty signed_rounds data"))
    }
}

// sign the new [unsiged_round] using [privkey]
pub fn sign_channel_round(
    script_hash: &[u8; 32], capacity: u64, previous_rounds: &Vec<(Round, Signature)>, unsiged_round: &Round, privkey: &Privkey
) -> Result<Signature> {
    let rounds_with_lastone_unsigned = {
        let mut rounds = previous_rounds.clone();
        rounds.push((unsiged_round.clone(), Signature::from_slice(&[0u8; 65]).unwrap()));
        rounds
    };

    let mut digest = [0u8; 32];
    let mut last_signature: Option<&Signature> = None;
    rounds_with_lastone_unsigned
        .iter()
        .for_each(|(round, signature)| {
            let mut hasher = new_blake2b();
            if let Some(last_signature) = last_signature {
                hasher.update(&digest);
                hasher.update(&last_signature.serialize());
            } else {
                hasher.update(script_hash);
                hasher.update(&capacity.to_le_bytes());
            }
            hasher.update(round.as_slice());
            hasher.finalize(&mut digest);
            last_signature = Some(signature);
        });

    Ok(privkey.sign_recoverable(&Message::from(digest))?)
}
