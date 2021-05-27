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
    use ckb_types::prelude::*;
    use hex;

    #[test]
    fn test_load() {
        let vars = load().expect("load");
        let hash = vars.nft.tx_hash;
        let expect_hash = hex::decode("a01b827feb4a09a319ff4db7a563fc9601da6384ba633a7c5f2825d15baab2a0").expect("hex");
        assert_eq!(hash.as_slice(), expect_hash.as_slice(), "bad nft transaction hash");
    }
}
