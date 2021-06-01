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

    #[test]
    fn test_load() {
        let vars = load().expect("load");
        let pubkey_hash = vars.common.user_key.pubhash.clone();
        let expected_pubkey_hash = hex::decode("40e88263ef526a8248570e931cfb2f2fb3ed044f").expect("hex");
        assert_eq!(&pubkey_hash[..20], expected_pubkey_hash.as_slice(), "bad private key");
    }
}
