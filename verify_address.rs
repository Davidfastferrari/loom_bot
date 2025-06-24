use alloy_primitives::hex;
use alloy_signer_local::PrivateKeySigner;

fn main() {
    let private_key = "87b9c2f432538c706b11c803258efc0b6e931381cd7e70d3ef1ec498dfee2b06";
    
    match hex::decode(private_key) {
        Ok(bytes) => {
            match PrivateKeySigner::from_bytes(&bytes) {
                Ok(signer) => {
                    println!("Private key: {}", private_key);
                    println!("Derived address: {}", signer.address());
                    println!("Expected address: 0x6E3b634eBd2EbBffb41a49fA6edF6df6bFe8c0Ee");
                }
                Err(e) => println!("Error creating signer: {}", e),
            }
        }
        Err(e) => println!("Error decoding private key: {}", e),
    }
}