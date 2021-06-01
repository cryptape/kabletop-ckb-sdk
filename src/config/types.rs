use serde::Deserialize;

#[derive(Deserialize)]
pub struct Common {
    pub ckb_uri:          String,
    pub ckb_indexer_uri:  String,
    pub composer_privkey: String,
    pub user_privkey:     String,
}

#[derive(Deserialize)]
pub struct Contract {
    pub tx_hash:   String,
    pub code_hash: String,
}

#[derive(Deserialize)]
pub struct Vars {
    pub common:   Common,
    pub nft:      Contract,
    pub wallet:   Contract,
    pub payment:  Contract,
    pub kabletop: Contract,
}

pub mod ckb {
    use std::convert::{
        From, TryInto
    };
    use crate::{
        config::types as conf, ckb::transaction::helper
    };
    use ckb_crypto::secp::Privkey;
    use ckb_hash::new_blake2b;
    use ckb_types::{
        packed::Byte32, H256
    };

    pub struct Keypair {
        pub privkey: Privkey,
        pub pubhash: [u8; 20]
    }

    pub struct Common {
        pub ckb_uri:         String,
        pub ckb_indexer_uri: String,
        pub composer_key:    Keypair,
        pub user_key:        Keypair
    }

    pub struct Contract {
        pub tx_hash:   Byte32,
        pub code_hash: Byte32,
    }

    pub struct Vars {
        pub common:   Common,
        pub nft:      Contract,
        pub wallet:   Contract,
        pub payment:  Contract,
        pub kabletop: Contract,
    }

    fn privkey_to_keypair(privkey: &str) -> Keypair {
        let privkey = helper::blake256_to_byte32(privkey).expect("blake2b_256 to [u8; 32]");
        let privkey = Privkey::from(H256(privkey));
        let pubkey = privkey.pubkey().expect("private key to public key");
        let mut hasher = new_blake2b();
        hasher.update(pubkey.serialize().as_slice());
        let mut pubkey_hash = [0u8; 32];
        hasher.finalize(&mut pubkey_hash);
        Keypair {
            privkey: privkey,
            pubhash: pubkey_hash[..20].try_into().unwrap()
        }
    }

    impl From<conf::Vars> for Vars {
        fn from(conf_vars: conf::Vars) -> Self {
            let contract = |conf_contract: conf::Contract| Contract {
                tx_hash:   Byte32::new(helper::blake256_to_byte32(conf_contract.tx_hash.as_str()).unwrap()),
                code_hash: Byte32::new(helper::blake256_to_byte32(conf_contract.code_hash.as_str()).unwrap())
            };
            let common = |conf_common: conf::Common| Common {
                ckb_uri:         conf_common.ckb_uri,
                ckb_indexer_uri: conf_common.ckb_indexer_uri,
                composer_key:    privkey_to_keypair(conf_common.composer_privkey.as_str()),
                user_key:        privkey_to_keypair(conf_common.user_privkey.as_str()),
            };
            Vars {
                common:   common(conf_vars.common),
                nft:      contract(conf_vars.nft),
                wallet:   contract(conf_vars.wallet),
                payment:  contract(conf_vars.payment),
                kabletop: contract(conf_vars.kabletop)
            }
        }
    }
}
