use alloy_primitives::Address;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref COINBASE: Address = "0x1f9090aaE28b8a3dCeaDf281B0F12828e676c326".parse().unwrap();
}
