extern crate aes;
extern crate sha2;
extern crate hex;
extern crate eyre;

use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;
use sha2::{Digest, Sha512};
use hex;
use eyre::Result;
use aes::cipher::generic_array::GenericArray;

const BLOCK_SIZE: usize = 16;

fn encrypt_key(pwd: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut hasher = Sha512::new();
    hasher.update(pwd);
    let pwd_hash = hasher.finalize();

    let cipher = Aes128::new_from_slice(&pwd_hash[0..16])?;

    let mut ret = Vec::new();
    let mut block = [0u8; BLOCK_SIZE];

    let mut a = 0;
    while a + BLOCK_SIZE <= data.len() {
        block.copy_from_slice(&data[a..a + BLOCK_SIZE]);
        let mut block_array = GenericArray::clone_from_slice(&block);
        cipher.encrypt_block(&mut block_array);
        ret.extend_from_slice(&block_array);
        a += BLOCK_SIZE;
    }

    let mut sha = Sha512::new();
    sha.update(&ret);
    let crc = &sha.finalize()[0..4];

    ret.extend_from_slice(crc);

    Ok(ret)
}

fn main() {
    // Replace this password with your encryption password or use the default from the app
    let password = b"your_password_here";

    // Replace this with your raw private key hex string
    let private_key_hex = "87b9c2f432538c706b11c803258efc0b6e931381cd7e70d3ef1ec498dfee2b06";

    let private_key_bytes = match hex::decode(private_key_hex) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Failed to decode private key hex: {}", e);
            return;
        }
    };

    match encrypt_key(password, &private_key_bytes) {
        Ok(encrypted) => {
            println!("Encrypted key (hex): {}", hex::encode(encrypted));
        }
        Err(e) => {
            eprintln!("Encryption failed: {}", e);
        }
    }
}
