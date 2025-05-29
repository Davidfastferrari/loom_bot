use aes::cipher::{Block, BlockDecrypt, KeyInit};
use aes::Aes128;
use eyre::{ErrReport, Result};
use generic_array::GenericArray;
use sha2::{Digest, Sha512};
use std::convert::TryInto;

use crate::private::KEY_ENCRYPTION_PWD;

const BLOCK_SIZE: usize = 16;

#[derive(Clone, Default)]
pub struct KeyStore {
    pwd: Vec<u8>,
}

impl KeyStore {
    pub fn new() -> KeyStore {
        KeyStore { pwd: KEY_ENCRYPTION_PWD.to_vec() }
    }

    pub fn new_from_string(pwd: String) -> KeyStore {
        KeyStore { pwd: pwd.as_bytes().to_vec() }
    }
    pub fn new_from_bytes(pwd: Vec<u8>) -> KeyStore {
        KeyStore { pwd }
    }

    pub fn encrypt_once(&self, data: &[u8]) -> Result<Vec<u8>> {
        if self.pwd.is_empty() {
            return Err(ErrReport::msg("NOT_INITIALIZED"));
        }

        let mut hasher = Sha512::new();
        hasher.update(&self.pwd);
        let pwd_hash = hasher.finalize();

        // Create a GenericArray from the first 16 bytes of the hash
        let key_array: [u8; 16] = pwd_hash[0..16].try_into().expect("slice with incorrect length");
        let key = GenericArray::from_array(key_array);
        let cipher = Aes128::new(&key);

        //println!("{:?}", pwd_hash);

        let mut ret = Vec::new();
        let mut block: Block<Aes128> = [0u8; BLOCK_SIZE].into();

        let mut a = 0;
        while a + BLOCK_SIZE <= data.len() {
            block.copy_from_slice(&data[a..a + BLOCK_SIZE]);
            cipher.decrypt_block(&mut block);
            ret.extend_from_slice(&block);
            a += BLOCK_SIZE;
        }

        let mut sha = Sha512::new();
        sha.update(&ret);
        let crc = &sha.finalize()[0..4];

        if data.len() < a + 4 {
            return Err(ErrReport::msg("DATA_TOO_SHORT"));
        }
        if &data[a..a + 4] != crc {
            return Err(ErrReport::msg("BAD_CHECKSUM"));
        }

        Ok(ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_once_not_initialized() {
        let key_store = KeyStore::new_from_string(String::from(""));
        let data = vec![0u8; 36];

        match key_store.encrypt_once(&data) {
            Ok(_) => panic!("Expected an error, but didn't get one"),
            Err(e) => assert_eq!(format!("{}", e), "NOT_INITIALIZED"),
        }
    }

    #[test]
    fn test_encrypt_once_bad_checksum() {
        let key_store = KeyStore::new_from_string(String::from("password"));
        let data = vec![0u8; 36];

        match key_store.encrypt_once(&data) {
            Ok(_) => panic!("Expected an error, but didn't get one"),
            Err(e) => assert_eq!(format!("{}", e), "BAD_CHECKSUM"),
        }
    }

    #[test]
    fn test_encrypt_once_data_too_short() {
        let key_store = KeyStore::new_from_string(String::from("password"));
        // Data length less than BLOCK_SIZE * n + 4 (e.g., 32 bytes only)
        let data = vec![0u8; 32];

        match key_store.encrypt_once(&data) {
            Ok(_) => panic!("Expected an error, but didn't get one"),
            Err(e) => assert_eq!(format!("{}", e), "DATA_TOO_SHORT"),
        }
    }

    // For this test, you'll need some valid encrypted data to pass and a correct password.
    #[test]
    fn test_encrypt_once_valid_data() {
        let key: Vec<u8> = vec![0x41, 0x8f, 0x2, 0xe4, 0x7e, 0xe4, 0x6, 0xaa, 0xee, 0x71, 0x9e, 0x30, 0xea, 0xe6, 0x64, 0x23];
        let key_store = KeyStore::new_from_bytes(key);
        //let encrypted_data = vec![0u8;36]; // Provide valid encrypted data here

        let encrypted_data = match hex::decode("51d9dc302b02a02a94d3c7f3057549cd0c990f4c7cc822b61af584fb85afdf209084f48a") {
            Ok(data) => data,
            Err(e) => panic!("Hex decode error in test: {}", e),
        };

        match key_store.encrypt_once(&encrypted_data) {
            Ok(decrypted_data) => {
                println!("{}", hex::encode(decrypted_data));
            }
            Err(_) => {
                //println!("{}", hex::encode(decrypted_data));
                panic!("BAD_CHECKSUM")
            }
        }
    }
}
