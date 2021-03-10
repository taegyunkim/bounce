pub use cubesat::*;
pub mod cubesat;

tonic::include_proto!("bounce"); // The string specified here must match the proto package name

#[derive(Clone, Debug)]
pub struct BounceConfig {
    num_cubesats: usize,
    slot_duration: u64,   // in seconds
    phase1_duration: u64, // in seconds
    phase2_duration: u64, // in seconds
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommitType {
    Precommit,
    Noncommit,
}

#[derive(Clone, Debug)]
pub struct Commit {
    typ: CommitType,
    // The id of signer
    id: usize,
    // signer's public key
    msg: Vec<u8>,
    public_key: Vec<u8>,
    signature: Vec<u8>,
}
