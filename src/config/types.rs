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
pub struct Luacode {
    pub tx_hash:   String,
    pub data_hash: String,
}

#[derive(Deserialize)]
pub struct Vars {
    pub common:   Common,
    pub nft:      Contract,
    pub wallet:   Contract,
    pub payment:  Contract,
    pub kabletop: Contract,
	pub luacodes: Vec<Luacode>
}

// ckb types format from string format "Kabletop.toml" config file
pub mod ckb {
    use std::convert::From;
    use crate::{
        config::types as conf, ckb::transaction::helper
    };
    use ckb_crypto::secp::{
		Privkey, Generator
	};
    use ckb_types::{
        packed::Byte32, H256, prelude::Pack
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
        pub code_hash: Byte32
    }

    pub struct Luacode {
        pub tx_hash:   Byte32,
        pub data_hash: Byte32
    }

    pub struct Vars {
        pub common:   Common,
        pub nft:      Contract,
        pub wallet:   Contract,
        pub payment:  Contract,
        pub kabletop: Contract,
		pub luacodes: Vec<Luacode>
    }

    fn privkey_to_keypair(privkey: &str) -> Keypair {
        let privkey = {
            let byte32 = helper::blake256_to_byte32(privkey).expect("blake2b_256 to [u8; 32]");
            Privkey::from(H256(byte32))
        };
        Keypair {
            pubhash: helper::privkey_to_pkhash(&privkey),
            privkey: privkey
        }
    }

	impl Default for Vars {
		fn default() -> Self {
			let default_key = || Keypair {
				privkey: Generator::random_privkey(),
				pubhash: [0u8; 20],
			};
			let default_contract = || Contract {
				tx_hash:   [0u8; 32].pack(),
				code_hash: [0u8; 32].pack()
			};
			Vars {
				common: Common {
					ckb_uri:         String::from("http://127.0.0.1:8115"),
					ckb_indexer_uri: String::from("http://127.0.0.1:8116"),
					composer_key:    default_key(),
					user_key:        default_key()
				},
				nft:      default_contract(),
				wallet:   default_contract(),
				payment:  default_contract(),
				kabletop: default_contract(),
				luacodes: vec![]
			}
		}
	}

    impl From<conf::Vars> for Vars {
        fn from(conf_vars: conf::Vars) -> Self {
            let contract = |conf_contract: conf::Contract| Contract {
                tx_hash:   Byte32::new(helper::blake256_to_byte32(conf_contract.tx_hash.as_str()).unwrap()),
                code_hash: Byte32::new(helper::blake256_to_byte32(conf_contract.code_hash.as_str()).unwrap())
            };
            let luacodes = |conf_luacodes: Vec<conf::Luacode>| -> Vec<Luacode> {
				conf_luacodes
					.iter()
					.map(|luacode| Luacode {
						tx_hash:   Byte32::new(helper::blake256_to_byte32(luacode.tx_hash.as_str()).unwrap()),
						data_hash: Byte32::new(helper::blake256_to_byte32(luacode.data_hash.as_str()).unwrap())
					})
					.collect::<Vec<_>>()
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
                kabletop: contract(conf_vars.kabletop),
				luacodes: luacodes(conf_vars.luacodes)
            }
        }
    }
}
