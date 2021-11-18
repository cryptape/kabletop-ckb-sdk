use molecule::{
    prelude::*, bytes::Bytes
};
use ckb_types::packed::Byte32;
use ckb_crypto::secp::Signature as CkbSignature;
use std::convert::TryInto;
use super::protocol::{
	*, self, Bytes as ProtoBytes
};

///////////////////////////////////////////
/// Into Functions
/////////////////////////////////////////// 

impl Into<Uint64T> for &u64 {
    fn into(self) -> Uint64T {
        let bytes = Bytes::from(self.to_le_bytes().to_vec());
        Uint64T::new_unchecked(bytes)
    }
}

impl Into<Uint64T> for u64 {
    fn into(self) -> Uint64T {
		(&self).into()
    }
}

impl Into<Uint8T> for &u8 {
    fn into(self) -> Uint8T {
        let bytes = Bytes::from(vec![self.clone()]);
        Uint8T::new_unchecked(bytes)
    }
}

impl Into<Uint8T> for u8 {
    fn into(self) -> Uint8T {
		(&self).into()
    }
}

impl Into<protocol::Bytes> for &[u8] {
	fn into(self) -> protocol::Bytes {
		let bytes = self
			.to_vec()
			.iter()
			.map(|byte| Byte::new(byte.clone()))
			.collect::<Vec<Byte>>();
		protocol::Bytes::new_builder()
			.set(bytes)
			.build()
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

impl Into<Signature> for &[u8; 65] {
    fn into(self) -> Signature {
        let bytes = Bytes::from(self.to_vec());
        Signature::new_unchecked(bytes)
    }
}

impl Into<Signature> for [u8; 65] {
    fn into(self) -> Signature {
        (&self).into()
    }
}

impl Into<Signature> for &CkbSignature {
	fn into(self) -> Signature {
		let signature: [u8; 65] = self.serialize().try_into().unwrap();
		signature.into()
	}
}

impl Into<Signature> for CkbSignature {
	fn into(self) -> Signature {
		let signature: [u8; 65] = self.serialize().try_into().unwrap();
		signature.into()
	}
}

impl Into<Hashes> for &Vec<Byte32> {
	fn into(self) -> Hashes {
		let hash_vec = self
			.iter()
			.map(|hash| hash.into())
			.collect::<Vec<Blake256>>();
		let mut hashes = Hashes::default();
		for hash in hash_vec {
			hashes = hashes
				.as_builder()
				.push(hash)
				.build();
		}
		hashes
	}
}

impl Into<Hashes> for Vec<Byte32> {
	fn into(self) -> Hashes {
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

impl Into<Operations> for &Vec<String> {
	fn into(self) -> Operations {
		let operations = self
			.iter()
			.map(|bytes| bytes.as_bytes().into())
			.collect::<Vec<ProtoBytes>>();
		Operations::new_builder()
			.set(operations)
			.build()
	}
}

impl Into<Operations> for Vec<String> {
	fn into(self) -> Operations {
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
        Self::from(&blake160)
    }
}

impl From<&Blake160> for [u8; 20] {
    fn from(blake160: &Blake160) -> Self {
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(blake160.raw_data().to_vec().as_slice());
        bytes
    }
}

impl From<Signature> for CkbSignature {
	fn from(signature: Signature) -> Self {
		CkbSignature::from_slice(signature.as_slice()).unwrap()
	}
}

impl From<Blake256> for Byte32 {
    fn from(blake256: Blake256) -> Self {
        Self::from(&blake256)
    }
}

impl From<&Blake256> for Byte32 {
    fn from(blake256: &Blake256) -> Self {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(blake256.raw_data().to_vec().as_slice());
        Self::new(bytes)
    }
}

impl From<Hashes> for Vec<Byte32> {
	fn from(hashes: Hashes) -> Self {
		hashes
			.into_iter()
			.map(|hash| hash.into())
			.collect::<Vec<_>>()
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

impl From<Operations> for Vec<Vec<u8>> {
	fn from(operations: Operations) -> Self {
		operations
			.into_iter()
			.map(|operation| operation.raw_data().to_vec())
			.collect::<Vec<_>>()
	}
}
