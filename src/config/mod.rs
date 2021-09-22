use toml;
use anyhow::Result;
use std::{
    fs::File, io::prelude::*
};

mod types;
use types::{
    Vars, ckb
};

lazy_static! {
    pub static ref VARS: ckb::Vars = load().expect("loading config");
}

fn load() -> Result<ckb::Vars> {
    let mut file = File::open("Kabletop.toml")?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let vars: Vars = toml::from_str(content.as_str())?;
    let vars = ckb::Vars::from(vars);
    Ok(vars)
}

#[cfg(test)]
mod test {
    use super::load;
    use hex;
	use ckb_types::prelude::Entity;

    #[test]
    fn test_load() {
        let vars = load().expect("load");
        let pubkey_hash = vars.common.user_key.pubhash.clone();
        let expected_pubkey_hash = hex::decode("40e88263ef526a8248570e931cfb2f2fb3ed044f").expect("hex");
        assert_eq!(&pubkey_hash[..20], expected_pubkey_hash.as_slice(), "bad private key");
    }

    #[test]
    fn test_load_luacodes() {
        let vars = load().expect("load");
        let luacode = vars.luacodes.get(0).expect("get luacode");
        let expected_tx_hash = hex::decode("e55ae885933943744c12b85de591f41e970fb46bc99043d89e6bbfefad2a2586").expect("hex");
        assert_eq!(luacode.tx_hash.as_slice(), expected_tx_hash.as_slice(), "bad luacode tx_hash");
    }
}
