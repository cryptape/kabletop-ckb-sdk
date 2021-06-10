use molecule::{
    prelude::*, bytes::Bytes
};
use ckb_types::packed::Byte32;
use super::kabletop::*;

///////////////////////////////////////////
/// Into Functions
/////////////////////////////////////////// 

impl Into<Uint64T> for u64 {
    fn into(self) -> Uint64T {
        let bytes = Bytes::from(self.to_le_bytes().to_vec());
        Uint64T::new_unchecked(bytes)
    }
}

impl Into<Uint8T> for u8 {
    fn into(self) -> Uint8T {
        let bytes = Bytes::from(vec![self]);
        Uint8T::new_unchecked(bytes)
    }
}

impl Into<Blake256> for &Byte32 {
    fn into(self) -> Blake256 {
        let bytes = Bytes::from(self.raw_data().to_vec());
        Blake256::new_unchecked(bytes)
    }
}

impl Into<Blake256> for Byte32 {
    fn into(self) -> Blake256 {
        (&self).into()
    }
}

impl Into<Blake160> for &[u8; 20] {
    fn into(self) -> Blake160 {
        let bytes = Bytes::from(self.to_vec());
        Blake160::new_unchecked(bytes)
    }
}

impl Into<Blake160> for [u8; 20] {
    fn into(self) -> Blake160 {
        (&self).into()
    }
}

impl Into<Nfts> for &Vec<[u8; 20]> {
    fn into(self) -> Nfts {
        let nft_vec = self
            .iter()
            .map(|&nft| nft.into())
            .collect::<Vec<Blake160>>();
        let mut nfts = Nfts::default();
        for nft in nft_vec {
            nfts = nfts
                .as_builder()
                .push(nft)
                .build();
        }
        nfts
    }
}

impl Into<Nfts> for Vec<[u8; 20]> {
    fn into(self) -> Nfts {
        (&self).into()
    }
}

///////////////////////////////////////////
/// From Functions
/////////////////////////////////////////// 

impl From<Uint64T> for u64 {
    fn from(uint64: Uint64T) -> Self {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(uint64.raw_data().to_vec().as_slice());
        Self::from_le_bytes(bytes)
    }
}

impl From<Uint8T> for u8 {
    fn from(uint8: Uint8T) -> Self {
        uint8.nth0().into()
    }
}

impl From<Blake160> for [u8; 20] {
    fn from(blake160: Blake160) -> Self {
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(blake160.raw_data().to_vec().as_slice());
        bytes
    }
}

impl From<Nfts> for Vec<[u8; 20]> {
    fn from(nfts: Nfts) -> Self {
        nfts
            .into_iter()
            .map(|nft| nft.into())
            .collect::<Vec<_>>()
    }
}
