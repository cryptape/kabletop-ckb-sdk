use ckb_types::{
    bytes::Bytes, core::Capacity, packed::Byte32
};
use std::{
    mem::size_of, convert::TryInto
};

// composers use this data to represent their NFT creations
pub struct NFTConfig {
    package_price:    u64,                  // ckb price per nft package
    package_capacity: u8,                   // nft count that one package could contain
    nft_config_table: Vec<([u8; 20], u16)>  // array for nft blake160/rate pair (rate means the probability a nft revealed)
}

impl NFTConfig {
    pub fn new(
        package_price: u64, package_capacity: u8, nft_config_table: Vec<([u8; 20], u16)>
    ) -> NFTConfig {
        // limit the basic params
        if package_price < 1
            || package_capacity < 1 
            || package_capacity > 32 
            || nft_config_table.is_empty() {
            panic!("bad nft config params");
        }
        // rates in nft_config_table should be ASC order
        let mut last_rate = 0u16;
        nft_config_table.iter().for_each(|&(_, rate)| {
            if last_rate > rate {
                panic!("bad nft config rate param");
            }
            last_rate = rate;
        });
        NFTConfig {
            package_price, package_capacity, nft_config_table
        }
    }

    // get the total ckb price of [package_count] packages
    pub fn buy_package(&self, package_count: u64) -> Capacity {
        Capacity::shannons(self.package_price * package_count)
    }

    // reveal [package_count] nft packages with the random seed which made from the mix of [transaction_root] and [uncles_hash]
    pub fn rip_package(&self, transactions_root: Byte32, uncles_hash: Byte32, package_count: u8) -> Bytes {
        let mut lotteries = vec![];
        for &tr in transactions_root.raw_data().iter() {
            for &ur in uncles_hash.raw_data().iter() {
                let lottery = (tr as u16) << 8 | ur as u16;
                lotteries.push(lottery);
            }
        }
        let mut collection: Vec<u8> = vec![];
        for i in 0..package_count {
            let lottery = lotteries[i as usize];
            let mut expect_nft: Option<[u8; 20]> = None;
            for &(nft, rate) in self.nft_config_table.iter() {
                if lottery < rate {
                    expect_nft = Some(nft);
                    break;
                }
            }
            if let Some(nft) = expect_nft {
                collection.append(&mut nft.to_vec());
            } else {
                let &(nft, _) = self.nft_config_table.iter().last().unwrap();
                collection.append(&mut nft.to_vec());
            }
        }
        Bytes::from(collection)
    }

    pub fn to_ckb_bytes(&self) -> Bytes {
        let mut bytes = vec![];
        bytes.append(&mut self.package_price.to_le_bytes().to_vec());
        bytes.append(&mut self.package_capacity.to_le_bytes().to_vec());
        for &(nft, rate) in self.nft_config_table.iter() {
            bytes.append(&mut nft.to_vec());
            bytes.append(&mut rate.to_le_bytes().to_vec());
        }
        Bytes::from(bytes)
    }
}

impl From<Bytes> for NFTConfig {
    fn from(ckb_bytes: Bytes) -> Self {
        let mut stream = StreamFetcher{ index: 0, stream: &ckb_bytes.to_vec() };
        let package_price    = stream.get_u64();
        let package_capacity = stream.get_u8();
        let mut nft_config_table = vec![];
        let item_size = size_of::<[u8; 20]>() + size_of::<u16>();
        for _ in 0..stream.count(item_size) {
            let nft  = stream.get_blake160();
            let rate = stream.get_u16();
            nft_config_table.push((nft, rate));
        }
        NFTConfig {
            package_capacity, package_price, nft_config_table
        }
    }
}

// internal struct for fetching bytes from stream
struct StreamFetcher<'load> {
    index: usize,
    stream: &'load Vec<u8>
}

impl<'load> StreamFetcher<'load> {
    fn next<T>(&mut self) -> (usize, usize) {
        let s = self.index;
        let e = s + size_of::<T>();
        self.index = e;
        return (s, e);
    }

    fn get_u64(&mut self) -> u64 {
        let (s, e) = self.next::<u64>();
        return u64::from_le_bytes(self.stream[s..e].try_into().unwrap());
    }

    fn get_u16(&mut self) -> u16 {
        let (s, e) = self.next::<u16>();
        return u16::from_le_bytes(self.stream[s..e].try_into().unwrap());
    }
    
    fn get_u8(&mut self) -> u8 {
        let (s, e) = self.next::<u8>();
        return u8::from_le_bytes(self.stream[s..e].try_into().unwrap());
    }

    fn get_blake160(&mut self) -> [u8; 20] {
        let (s, e) = self.next::<[u8; 20]>();
        return self.stream[s..e].try_into().unwrap();
    }

    fn count(&self, size: usize) -> usize {
        (self.stream.len() - self.index) / size
    }
}
