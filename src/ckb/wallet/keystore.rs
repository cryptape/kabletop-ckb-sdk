use ckb_crypto::secp::Privkey;
use crate::config::VARS as _C;

lazy_static! {
    pub static ref COMPOSER_PRIVKEY: Privkey = _C.common.composer_key.privkey.clone();
    pub static ref COMPOSER_PUBHASH: [u8; 20] = _C.common.composer_key.pubhash;
    pub static ref USER_PRIVKEY: Privkey = _C.common.user_key.privkey.clone();
    pub static ref USER_PUBHASH: [u8; 20] = _C.common.user_key.pubhash;
}
