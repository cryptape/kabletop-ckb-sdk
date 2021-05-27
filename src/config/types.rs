use serde::Deserialize;

#[derive(Deserialize)]
pub struct Common {
    pub ckb_uri:         String,
    pub ckb_indexer_uri: String,
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
    use ckb_types::packed::Byte32;
    use crate::config::types as conf;
    use hex;

    pub struct Contract {
        pub tx_hash:   Byte32,
        pub code_hash: Byte32,
    }

    pub struct Vars {
        pub common:   conf::Common,
        pub nft:      Contract,
        pub wallet:   Contract,
        pub payment:  Contract,
        pub kabletop: Contract,
    }

    impl From<conf::Vars> for Vars {
        fn from(conf_vars: conf::Vars) -> Self {
            let byte32 = |hash: String| {
                let hash = hex::decode(hash).expect("format to hex");
                let hash: [u8; 32] = hash.try_into().expect("into 32 bytes");
                Byte32::new(hash)
            };
            let contract = |conf_contract: conf::Contract| Contract {
                tx_hash:   byte32(conf_contract.tx_hash),
                code_hash: byte32(conf_contract.code_hash)
            };
            Vars {
                common:   conf_vars.common,
                nft:      contract(conf_vars.nft),
                wallet:   contract(conf_vars.wallet),
                payment:  contract(conf_vars.payment),
                kabletop: contract(conf_vars.kabletop),
            }
        }
    }
}
